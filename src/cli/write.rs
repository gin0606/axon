use super::{
    Output, Publication,
    args::{Body, Change, Condition, Create, Dependency, Import, LabelCommand, Parent},
    display, output,
    render::{confirmation, converted, declaration_changes, declaration_ids, situation_label},
    store::{context, fresh_entity_id, open, resolve},
};
use axon::{
    Result,
    lifecycle::{
        Context, EntityId, Kind, Lifecycle, Operation,
        record::{Current, Entry, Record, RecordId, Store, View},
    },
};
use chrono::Utc;
use std::{collections::BTreeSet, path::PathBuf};

/// One ordinary mutation: resolve against the locked record set, produce at most one record,
/// publish it and confirm from the result.
fn mutate(
    change: impl FnOnce(&Store, &View) -> Result<(Option<Record>, String)>,
) -> Result<Output> {
    let (_, mut store) = open()?;
    let (text, saved) = store.update(|_, records, view| {
        let (record, text) = change(records, view)?;
        let saved = record.is_some();
        Ok((
            record.into_iter().map(Entry::Record).collect(),
            (text, saved),
        ))
    })?;
    Ok(output(text, saved))
}

pub(super) fn import(command: Import) -> Result<Output> {
    let (_, mut store) = open()?;
    match command {
        Import::Apply { file } => {
            let outcome = axon::declaration_file::apply(&mut store, &file, context())?;
            Ok(Output {
                text: display::human_text(format!(
                    "Applied: {}; declaration updated: {}\n{}",
                    if outcome.changed {
                        "storage applied"
                    } else {
                        "storage unchanged (no-op)"
                    },
                    display::line(file.display()),
                    declaration_ids(&outcome.new_ids)
                )),
                publication: if outcome.changed {
                    Publication::StorageAndDeclaration
                } else {
                    Publication::Declaration
                },
                diagnostic: String::new(),
            })
        }
        command => {
            let (header, records, view) = store.read()?;
            let file = match &command {
                Import::Prepare { file } | Import::Check { file } => file,
                Import::Apply { .. } => unreachable!(),
            };
            let bytes = std::fs::read(file)?;
            let input = std::str::from_utf8(&bytes)
                .map_err(|e| axon::Error::Invalid(format!("Declaration schema: {e}")))?;
            let mut declaration =
                axon::declaration::parse(input).map_err(|e| axon::Error::Invalid(e.to_string()))?;
            let publication = if matches!(&command, Import::Prepare { .. }) {
                Publication::Declaration
            } else {
                Publication::None
            };
            let text = match command {
                Import::Apply { .. } => unreachable!(),
                Import::Prepare { ref file } => {
                    declaration
                        .prepare(&records, &header.prefix)
                        .map_err(|e| axon::Error::Invalid(e.to_string()))?;
                    let text = declaration
                        .serialize(&view)
                        .map_err(|e| axon::Error::Invalid(e.to_string()))?;
                    axon::declaration_file::rewrite(file, &bytes, text.as_bytes())?;
                    format!(
                        "Prepared {}. Storage unchanged.\n{}",
                        display::line(file.display()),
                        declaration_ids(&declaration.assigned_new_ids())
                    )
                }
                Import::Check { .. } => {
                    let checked = declaration
                        .check(
                            input,
                            &records,
                            Context {
                                at: Utc::now(),
                                recorder: None,
                            },
                        )
                        .map_err(|e| axon::Error::Invalid(e.to_string()))?;
                    declaration_changes(&declaration, &view, &checked)?
                }
            };
            Ok(Output {
                text: display::human_text(text),
                publication,
                diagnostic: String::new(),
            })
        }
    }
}
pub(super) fn capture(args: Create) -> Result<Output> {
    let kind = args.kind.kind();
    let lifecycle = if args.accept {
        Lifecycle::NotStarted
    } else {
        Lifecycle::Undecided
    };
    let description = args.body.read()?.unwrap_or_default();
    let title = args.title;
    let (_, mut store) = open()?;
    let text = store.update(|header, records, view| {
        let parent = args.parent.map(|p| resolve(view, &p)).transpose()?;
        let needs = args
            .needs
            .into_iter()
            .map(|p| resolve(view, &p))
            .collect::<Result<BTreeSet<_>>>()?;
        let id = fresh_entity_id(&header.prefix, view)?;
        let record = records.create(
            id.clone(),
            Current {
                kind,
                lifecycle,
                owner: None,
                title,
                description,
                label: args.label.0,
                condition: args.command,
                parent,
                needs,
            },
            context(),
        )?;
        let title = display::line(&record.after.title);
        let text = format!(
            "{}  {}  {}  {}  {title}\n",
            display::identity(&id),
            display::positive("Created"),
            display::muted(format!("{kind:?}")),
            display::situation(&format!("{lifecycle:?}"))
        );
        Ok((vec![Entry::Record(record)], text))
    })?;
    Ok(output(text, true))
}

pub(super) fn write(value: String, title: Option<String>, body: Body) -> Result<Output> {
    let body = body.read()?;
    if title.is_none() && body.is_none() {
        return Err(axon::Error::Invalid(
            "write requires --title, --description or --file".into(),
        ));
    }
    mutate(|records, view| {
        let id = resolve(view, &value)?;
        let record = records.write(&id, title, body, context())?;
        let mut changes = Vec::new();
        if let Some(record) = &record {
            let before = view.current(&id).expect("settled Entity");
            let after = &record.after;
            if before.title != after.title {
                changes.push("Title updated");
            }
            if before.description != after.description {
                changes.push(if after.description.is_empty() {
                    "Description removed"
                } else {
                    "Description updated"
                });
            }
        }
        if changes.is_empty() {
            changes.push("No changes");
        }
        Ok((record, confirmation(&id, &changes.join("  "))))
    })
}
pub(super) fn label(command: LabelCommand) -> Result<Output> {
    let LabelCommand::Set {
        id: value,
        value: label,
    } = command;
    mutate(|records, view| {
        let id = resolve(view, &value)?;
        let record = records.set_label(&id, label.0, context())?;
        let text = confirmation(
            &id,
            &format!(
                "{}: {}",
                if record.is_none() {
                    "No changes  Label"
                } else {
                    "Label updated"
                },
                label.0
            ),
        );
        Ok((record, text))
    })
}
pub(super) fn add_note(
    value: String,
    message: Option<String>,
    file: Option<PathBuf>,
) -> Result<Output> {
    let body = Body {
        description: message,
        description_file: file,
    }
    .read()?
    .unwrap_or_default();
    let (_, mut store) = open()?;
    let text = store.update(|_, records, view| {
        let id = resolve(view, &value)?;
        let note = records.add_note(&id, body, None, context())?;
        let entry = Entry::Note(note);
        let note_id = entry.id()?;
        Ok((
            vec![entry],
            confirmation(&id, &format!("Note {note_id} recorded")),
        ))
    })?;
    Ok(output(text, true))
}
pub(super) fn parent(command: Parent) -> Result<Output> {
    let (value, parent) = match command {
        Parent::Set { id, parent } => (id, Some(parent)),
        Parent::Unset { id } => (id, None),
    };
    mutate(|records, view| {
        let id = resolve(view, &value)?;
        let parent = parent.map(|p| resolve(view, &p)).transpose()?;
        let record = records.set_parent(&id, parent.clone(), context())?;
        let text = confirmation(
            &id,
            &format!(
                "{}Parent: {}",
                if record.is_none() { "No changes  " } else { "" },
                parent
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| "(none)".into())
            ),
        );
        Ok((record, text))
    })
}
pub(super) fn condition(command: Condition) -> Result<Output> {
    let (value, command) = match command {
        Condition::Set { id, command } => (id, Some(command)),
        Condition::Unset { id } => (id, None),
    };
    mutate(|records, view| {
        let id = resolve(view, &value)?;
        let unset = command.is_none();
        let record = records.set_condition(&id, command, context())?;
        let text = confirmation(
            &id,
            if record.is_none() {
                "No changes"
            } else if unset {
                "Condition unset"
            } else {
                "Condition updated"
            },
        );
        Ok((record, text))
    })
}
pub(super) fn dependency(command: Dependency) -> Result<Output> {
    let (value, needs, add) = match command {
        Dependency::Add { id, needs } => (id, needs, true),
        Dependency::Rm { id, needs } => (id, needs, false),
    };
    mutate(|records, view| {
        let id = resolve(view, &value)?;
        // A removal names one of the Entity's own dependencies, which need not exist as an
        // Entity (a violation Git left behind); an addition names an Entity.
        let needs = if add {
            resolve(view, &needs)?
        } else {
            match resolve_dependency(view, &id, &needs)? {
                Some(target) => target,
                None => resolve(view, &needs)?,
            }
        };

        let record = if add {
            records.add_dependency(&id, &needs, context())?
        } else {
            records.remove_dependency(&id, &needs, context())?
        };
        let result = match (add, record.is_some()) {
            (true, true) => "Dependency added:",
            (false, true) => "Dependency removed:",
            (true, false) => "No changes  Dependency already present:",
            (false, false) => "No changes  Dependency already absent:",
        };
        Ok((record, confirmation(&id, &format!("{result} {needs}"))))
    })
}
/// The dependency of the Entity that `value` names: completely, or by a unique suffix when
/// `value` is not the complete ID of any Entity. None when no dependency matches, an error
/// when several do.
fn resolve_dependency(view: &View, id: &EntityId, value: &str) -> Result<Option<EntityId>> {
    let Some(current) = view.current(id) else {
        return Ok(None);
    };
    if let Some(exact) = current.needs.iter().find(|need| need.as_ref() == value) {
        return Ok(Some(exact.clone()));
    }
    if let Ok(exact) = EntityId::try_from(value.to_owned())
        && (view.is_known(&exact) || view.noted_only().contains(&exact))
    {
        return Ok(None);
    }
    let matches: Vec<_> = current
        .needs
        .iter()
        .filter(|need| need.as_ref().ends_with(value))
        .collect();
    match matches.as_slice() {
        [] => Ok(None),
        [found] => Ok(Some((*found).clone())),
        ids => Err(axon::Error::Invalid(format!(
            "ambiguous dependency {value}: {}",
            ids.iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}
pub(super) fn convert(value: String, kind: Kind) -> Result<Output> {
    mutate(|records, view| {
        let id = resolve(view, &value)?;
        let record = records.convert(&id, kind, context())?;
        let text = confirmation(
            &id,
            &match &record {
                Some(record) => converted(
                    view.current(&id).expect("settled Entity").kind,
                    record.after.kind,
                ),
                None => format!("No changes  Kind: {kind:?}"),
            },
        );
        Ok((record, text))
    })
}
/// `axon resolve ID --head RECORD_ID`: one resolve record that takes the head's value. The
/// situation after it (with `+Invalid` when a violation remains) is derived from the record
/// set with the new record added, without evaluating conditions.
pub(super) fn resolve_conflict(
    value: String,
    head: String,
    reason: Option<String>,
) -> Result<Output> {
    mutate(|records, view| {
        let id = resolve(view, &value)?;
        let head = RecordId::try_from(head.as_str())?;
        let record = records.resolve(&id, &head, reason, context())?;
        // The situation is read from a set that holds the new record, so that the store and
        // the view derived from it agree.
        let mut after = records.clone();
        after.insert(Entry::Record(record.clone()))?;
        let derived = after.view()?;
        let key = derived.key(&id).expect("resolved Entity is known");
        let situation = situation_label(&axon::read::View::new(&after, &derived).row(key, None));
        Ok((
            Some(record),
            confirmation(&id, &format!("Resolved: {head}  {situation}")),
        ))
    })
}
pub(super) fn transition(args: Change, operation: Operation) -> Result<Output> {
    mutate(|records, view| {
        let id = resolve(view, &args.id)?;
        let record = records.perform(&id, operation, args.reason, context())?;
        let effect = match operation {
            Operation::Accept => "Accepted  NotStarted",
            Operation::Withdraw => "Withdrawn  Undecided",
            Operation::Start => "Started  InProgress",
            Operation::Release => "Released  NotStarted",
            Operation::Complete => "Completed",
            Operation::Cancel => "Cancelled",
            Operation::Reconsider => "Reconsidered  Undecided",
            Operation::Reopen => "Reopened  NotStarted",
        };
        Ok((Some(record), confirmation(&id, effect)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axon::lifecycle::{Label, record::Kind};
    use chrono::Utc;
    use proptest::prelude::*;

    #[test]
    fn a_removal_resolves_among_the_dependencies_before_the_store() {
        let mut store = Store::new();
        let context = || Context {
            at: Utc::now(),
            recorder: None,
        };
        let register = |store: &mut Store, name: &str, needs: &[&str]| {
            let record = store
                .create(
                    name.to_string().try_into().unwrap(),
                    Current {
                        kind: Kind::Issue,
                        lifecycle: Lifecycle::NotStarted,
                        owner: None,
                        title: name.into(),
                        description: String::new(),
                        label: Label::Chore,
                        condition: None,
                        parent: None,
                        needs: needs
                            .iter()
                            .map(|n| n.to_string().try_into().unwrap())
                            .collect(),
                    },
                    context(),
                )
                .unwrap();
            store.insert(Entry::Record(record)).unwrap();
        };
        register(&mut store, "t-abc", &[]);
        register(&mut store, "xt-abc", &[]);
        register(&mut store, "t-twin-q", &[]);
        register(&mut store, "t-other-q", &[]);
        register(&mut store, "t-user", &["xt-abc", "t-twin-q", "t-other-q"]);
        let view = store.view().unwrap();
        let user: EntityId = "t-user".to_string().try_into().unwrap();
        let found = |value: &str| {
            resolve_dependency(&view, &user, value)
                .unwrap()
                .map(|id| id.to_string())
        };
        // A dependency named completely wins; a complete ID of another Entity resolves to
        // that Entity (none here), even when it is the suffix of a dependency.
        assert_eq!(found("xt-abc").as_deref(), Some("xt-abc"));
        assert_eq!(found("t-abc"), None);
        // A suffix unique among the dependencies resolves although the store has more.
        assert_eq!(found("twin-q").as_deref(), Some("t-twin-q"));
        assert_eq!(found("abc").as_deref(), Some("xt-abc"));
        // An ambiguous suffix among the dependencies is refused.
        let error = resolve_dependency(&view, &user, "-q")
            .unwrap_err()
            .to_string();
        assert!(error.contains("ambiguous dependency"), "{error}");
        assert_eq!(found("nothing"), None);
    }

    proptest! {
        #[test]
        fn dependency_suffixes_use_only_the_owned_set(tail in "[a-z0-9]{3,8}") {
            let owned = format!("owned-{tail}");
            let twin = format!("twin-{tail}");
            let foreign = format!("foreign-{owned}");
            let user = "owner-user";
            let mut store = Store::new();
            let context = || Context { at: Utc::now(), recorder: None };
            for (id, needs) in [
                (owned.as_str(), vec![]),
                (twin.as_str(), vec![]),
                (foreign.as_str(), vec![]),
                (user, vec![owned.as_str(), twin.as_str()]),
            ] {
                let record = store.create(id.to_owned().try_into().unwrap(), Current {
                    kind: Kind::Issue,
                    lifecycle: Lifecycle::NotStarted,
                    owner: None,
                    title: id.into(),
                    description: String::new(),
                    label: Label::Chore,
                    condition: None,
                    parent: None,
                    needs: needs.into_iter().map(|n| n.to_owned().try_into().unwrap()).collect(),
                }, context()).unwrap();
                store.insert(Entry::Record(record)).unwrap();
            }
            let view = store.view().unwrap();
            let user = EntityId::try_from(user.to_owned()).unwrap();
            prop_assert_eq!(resolve_dependency(&view, &user, &owned).unwrap().as_ref().map(ToString::to_string), Some(owned.clone()));
            prop_assert_eq!(resolve_dependency(&view, &user, &foreign).unwrap(), None);
            prop_assert_eq!(resolve_dependency(&view, &user, &format!("d-{tail}")).unwrap().as_ref().map(ToString::to_string), Some(owned));
            prop_assert!(resolve_dependency(&view, &user, &tail).unwrap_err().to_string().contains("ambiguous dependency"));
            prop_assert_eq!(resolve_dependency(&view, &user, "missing").unwrap(), None);
        }
    }
}
