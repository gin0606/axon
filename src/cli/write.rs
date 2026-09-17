use super::{
    Output, Publication,
    args::{Body, Change, Condition, Create, Dependency, Import, Parent},
    display, output,
    render::{confirmation, declaration_changes, declaration_ids},
    store::{context, fresh_entity_id, open, resolve},
};
use axon::{
    Result,
    lifecycle::{Context, Current, Lifecycle, Operation},
};
use chrono::Utc;
use std::{collections::BTreeSet, path::PathBuf};

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
                    file.display(),
                    declaration_ids(&outcome.new_ids)
                )),
                publication: Publication::StorageAndDeclaration,
                diagnostic: String::new(),
            })
        }
        command => {
            let (prefix, snapshot) = store.read()?;
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
                        .prepare(&snapshot, &prefix)
                        .map_err(|e| axon::Error::Invalid(e.to_string()))?;
                    let text = declaration
                        .serialize(&snapshot)
                        .map_err(|e| axon::Error::Invalid(e.to_string()))?;
                    axon::declaration_file::rewrite(file, &bytes, text.as_bytes())?;
                    format!(
                        "Prepared {}. Storage unchanged.\n{}",
                        file.display(),
                        declaration_ids(&declaration.assigned_new_ids())
                    )
                }
                Import::Check { .. } => {
                    let checked = declaration
                        .check(
                            input,
                            &snapshot,
                            Context {
                                at: Utc::now(),
                                recorder: None,
                            },
                        )
                        .map_err(|e| axon::Error::Invalid(e.to_string()))?;
                    declaration_changes(
                        &declaration,
                        &snapshot,
                        &checked.snapshot,
                        checked.already_applied,
                    )?
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
    let (_, mut store) = open()?;
    let kind = args.kind.kind();
    let lifecycle = if args.accept {
        Lifecycle::NotStarted
    } else {
        Lifecycle::Undecided
    };
    let description = args.body.read()?.unwrap_or_default();
    let title = args.title;
    let text = store.update(|prefix, snapshot| {
        let parent = args.parent.map(|p| resolve(snapshot, &p)).transpose()?;
        let dependencies = args
            .needs
            .into_iter()
            .map(|p| resolve(snapshot, &p))
            .collect::<Result<BTreeSet<_>>>()?;
        let id = fresh_entity_id(prefix, snapshot)?;
        snapshot.create(
            id.clone(),
            kind,
            Current {
                title,
                description,
                lifecycle,
                condition: args.command,
                parent,
                dependencies,
            },
            context(),
        )?;
        let title = display::human_text(&snapshot.entity(&id)?.current.title).replace('\n', "\\n");
        Ok(format!(
            "{}  {}  {}  {}  {title}\n",
            display::identity(&id),
            display::positive("Created"),
            display::muted(format!("{kind:?}")),
            display::situation(&format!("{lifecycle:?}"))
        ))
    })?;
    Ok(output(text, true))
}

pub(super) fn write(value: String, title: Option<String>, body: Body) -> Result<Output> {
    let (_, mut store) = open()?;
    let body = body.read()?;
    if title.is_none() && body.is_none() {
        return Err(axon::Error::Invalid(
            "write requires --title, --description or --file".into(),
        ));
    }
    let text = store.update(|_, snapshot| {
        let id = resolve(snapshot, &value)?;
        let before = snapshot.entity(&id)?.current.clone();
        snapshot.write(&id, title, body)?;
        let after = &snapshot.entity(&id)?.current;
        let mut changes = Vec::new();
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
        if changes.is_empty() {
            changes.push("No changes");
        }
        Ok(confirmation(&id, &changes.join("  ")))
    })?;
    Ok(output(text, true))
}
pub(super) fn add_note(
    value: String,
    message: Option<String>,
    file: Option<PathBuf>,
) -> Result<Output> {
    let (_, mut store) = open()?;
    let body = Body {
        description: message,
        description_file: file,
    }
    .read()?
    .unwrap_or_default();
    let text = store.update(|_, snapshot| {
        let id = resolve(snapshot, &value)?;
        let note = snapshot.add_note(&id, body, context())?;
        Ok(confirmation(&id, &format!("Note {note} recorded")))
    })?;
    Ok(output(text, true))
}
pub(super) fn parent(command: Parent) -> Result<Output> {
    let (_, mut store) = open()?;
    let (value, parent) = match command {
        Parent::Set { id, parent } => (id, Some(parent)),
        Parent::Unset { id } => (id, None),
    };
    let text = store.update(|_, snapshot| {
        let id = resolve(snapshot, &value)?;
        let parent = parent.map(|p| resolve(snapshot, &p)).transpose()?;
        let unchanged = snapshot.entity(&id)?.current.parent == parent;
        snapshot.set_parent(&id, parent.clone())?;
        Ok(confirmation(
            &id,
            &format!(
                "{}Parent: {}",
                if unchanged { "No changes  " } else { "" },
                parent
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| "(none)".into())
            ),
        ))
    })?;
    Ok(output(text, true))
}
pub(super) fn condition(command: Condition) -> Result<Output> {
    let (_, mut store) = open()?;
    let (value, command) = match command {
        Condition::Set { id, command } => (id, Some(command)),
        Condition::Unset { id } => (id, None),
    };
    let text = store.update(|_, snapshot| {
        let id = resolve(snapshot, &value)?;
        let unchanged = snapshot.entity(&id)?.current.condition == command;
        let unset = command.is_none();
        snapshot.set_condition(&id, command)?;
        Ok(confirmation(
            &id,
            if unchanged {
                "No changes"
            } else if unset {
                "Condition unset"
            } else {
                "Condition updated"
            },
        ))
    })?;
    Ok(output(text, true))
}
pub(super) fn dependency(command: Dependency) -> Result<Output> {
    let (_, mut store) = open()?;
    let (value, needs, add) = match command {
        Dependency::Add { id, needs } => (id, needs, true),
        Dependency::Rm { id, needs } => (id, needs, false),
    };
    let text = store.update(|_, snapshot| {
        let id = resolve(snapshot, &value)?;
        let needs = resolve(snapshot, &needs)?;
        let present = snapshot.entity(&id)?.current.dependencies.contains(&needs);
        if add {
            snapshot.add_dependency(&id, &needs)?;
        } else {
            snapshot.remove_dependency(&id, &needs)?;
        }
        let result = match (add, present) {
            (true, false) => "Dependency added:",
            (false, true) => "Dependency removed:",
            (true, true) => "No changes  Dependency already present:",
            (false, false) => "No changes  Dependency already absent:",
        };
        Ok(confirmation(&id, &format!("{result} {needs}")))
    })?;
    Ok(output(text, true))
}
pub(super) fn transition(args: Change, operation: Operation) -> Result<Output> {
    let (_, mut store) = open()?;
    let text = store.update(|_, snapshot| {
        let id = resolve(snapshot, &args.id)?;
        snapshot.perform(&id, operation, args.reason, context())?;
        let effect = match operation {
            Operation::Accept => "Accepted  NotStarted",
            Operation::Withdraw => "Withdrawn  Undecided",
            Operation::Start => "Started  InProgress",
            Operation::Release => "Released  NotStarted",
            Operation::Complete => "Completed",
            Operation::Cancel => "Cancelled",
            Operation::Reconsider => "Reconsidered  Undecided",
        };
        Ok(confirmation(&id, effect))
    })?;
    Ok(output(text, true))
}
