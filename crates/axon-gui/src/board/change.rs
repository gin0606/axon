//! The changes the window writes to one project: creating an Entity, editing its text and
//! label, adding a Note, a lifecycle transition, or a structural change (moving an Entity under
//! another Group or out of any, adding or removing a dependency, converting between Issue and
//! Group). Every rule is the core's: this module names a change, runs the core's operation for
//! it against a set of records, and tells whether a later read shows it. Each change writes at
//! most one entry, so a refused or failed one leaves nothing of it behind.

use super::Board;
use axon::lifecycle::{
    Context, EntityId, Error, Kind, Label, Lifecycle, Operation, Refusal,
    record::{Current, Entry, Imported, Nonce, Store, View},
};
use chrono::DateTime;
use std::collections::BTreeSet;

/// The value a new Entity is created with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewEntity {
    pub kind: Kind,
    /// Undecided or NotStarted; the core refuses anything else.
    pub lifecycle: Lifecycle,
    pub title: String,
    pub description: String,
    pub label: Label,
    pub parent: Option<EntityId>,
}

/// The text fields an edit sets. A field left `None` keeps the value the store holds when the
/// edit is written, so an edit never undoes a change of another field made meanwhile.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Edit {
    pub title: Option<String>,
    pub description: Option<String>,
    pub label: Option<Label>,
}
impl Edit {
    pub fn is_empty(&self) -> bool {
        self.title.is_none() && self.description.is_none() && self.label.is_none()
    }
}

/// One change of `entity`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    /// Creates `entity`, an ID the store does not know, with `value`.
    Create {
        entity: EntityId,
        value: NewEntity,
    },
    /// Sets the text fields and label of `entity` in one record.
    Edit {
        entity: EntityId,
        edit: Edit,
    },
    /// Appends a Note to `entity`. The Note carries `nonce`, chosen when it was asked for, so a
    /// later read tells this Note from any other with the same body.
    AddNote {
        entity: EntityId,
        body: String,
        nonce: Nonce,
    },
    /// One lifecycle transition, leading to `to` from the lifecycle shown when it was chosen.
    Transition {
        entity: EntityId,
        operation: Operation,
        to: Lifecycle,
    },
    /// Puts the Entity, with everything under it, under `parent`, or out of any Group.
    Move {
        entity: EntityId,
        parent: Option<EntityId>,
    },
    AddDependency {
        entity: EntityId,
        target: EntityId,
    },
    RemoveDependency {
        entity: EntityId,
        target: EntityId,
    },
    Convert {
        entity: EntityId,
        kind: Kind,
    },
}

/// The part of the detail pane a change is made from, where its outcome is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    /// The workbench, where an Entity is created.
    Create,
    /// The title, description and label, changed from the edit form.
    Text,
    Notes,
    /// The lifecycle, changed from the state menu.
    Progress,
    /// Where the Entity belongs and what kind it is.
    Structure,
    Dependencies,
}

/// Why the core refused a change: a refusal naming the Entities involved, or the core's
/// message for any other rule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Rejection {
    Refused(Refusal),
    Other(String),
}
impl From<Error> for Rejection {
    fn from(error: Error) -> Self {
        match error.refusal() {
            Some(refusal) => Self::Refused(refusal.clone()),
            None => Self::Other(error.message().to_owned()),
        }
    }
}

impl Change {
    pub fn section(&self) -> Section {
        match self {
            Self::Create { .. } => Section::Create,
            Self::Edit { .. } => Section::Text,
            Self::AddNote { .. } => Section::Notes,
            Self::Transition { .. } => Section::Progress,
            Self::Move { .. } | Self::Convert { .. } => Section::Structure,
            Self::AddDependency { .. } | Self::RemoveDependency { .. } => Section::Dependencies,
        }
    }

    pub fn entity(&self) -> &EntityId {
        match self {
            Self::Create { entity, .. }
            | Self::Edit { entity, .. }
            | Self::AddNote { entity, .. }
            | Self::Transition { entity, .. }
            | Self::Move { entity, .. }
            | Self::AddDependency { entity, .. }
            | Self::RemoveDependency { entity, .. }
            | Self::Convert { entity, .. } => entity,
        }
    }

    /// The entry the change adds to `records`, by the core's operation for it checked against
    /// `view`, the view derived from `records`; none when the records show the change already.
    pub fn entry(
        &self,
        records: &Store,
        view: &View,
        context: Context,
    ) -> Result<Option<Entry>, Error> {
        let operations = records.prepared(view);
        let record = match self {
            Self::Create { entity, value } => {
                let current = Current {
                    kind: value.kind,
                    lifecycle: value.lifecycle,
                    owner: None,
                    title: value.title.clone(),
                    description: value.description.clone(),
                    label: value.label,
                    condition: None,
                    parent: value.parent.clone(),
                    needs: BTreeSet::new(),
                };
                operations
                    .create(entity.clone(), current, context)
                    .map(Some)
            }
            Self::Edit { entity, edit } => edit_record(records, view, entity, edit, context),
            Self::AddNote {
                entity,
                body,
                nonce,
            } => {
                // A Note is checked against the records alone, conflicts or not.
                return records
                    .add_note(entity, body.clone(), None, context)
                    .map(|note| {
                        Some(Entry::Note(axon::lifecycle::Note {
                            nonce: nonce.clone(),
                            ..note
                        }))
                    });
            }
            Self::Transition {
                entity, operation, ..
            } => operations
                .perform(entity, *operation, None, context)
                .map(Some),
            Self::Move { entity, parent } => {
                operations.set_parent(entity, parent.clone(), None, context)
            }
            Self::AddDependency { entity, target } => {
                operations.add_dependency(entity, target, None, context)
            }
            Self::RemoveDependency { entity, target } => {
                operations.remove_dependency(entity, target, None, context)
            }
            Self::Convert { entity, kind } => operations.convert(entity, *kind, None, context),
        };
        record.map(|record| record.map(Entry::Record))
    }

    /// Whether the core accepts the change on the records of `board`, checked against the view
    /// the board derived when it was read. The write checks again against the records it finds
    /// under the store's lock; this only answers before writing.
    pub fn check(&self, board: &Board) -> Result<(), Rejection> {
        // The record is discarded, so its time is never seen.
        let context = Context {
            at: DateTime::UNIX_EPOCH,
            recorder: None,
        };
        self.entry(board.records(), board.read().derived(), context)
            .map(|_| ())
            .map_err(Rejection::from)
    }

    /// Whether the settled value `board` shows of the Entity has the change made, for telling
    /// what a publication of unknown outcome left; none when the Entity is conflicted or not
    /// in the store, so its value cannot tell.
    pub fn is_shown_by(&self, board: &Board) -> Option<bool> {
        match self {
            // An Entity not in the store was not created; the ID was new when it was asked for.
            Self::Create { entity, .. } => return Some(board.item(entity).is_some()),
            // Notes are added to a conflicted Entity too.
            Self::AddNote { entity, nonce, .. } => {
                board.item(entity)?;
                return Some(board.has_note(entity, nonce));
            }
            _ => {}
        }
        let read = board.read();
        let current = read.current(self.entity())?;
        Some(match self {
            Self::Create { .. } | Self::AddNote { .. } => unreachable!("answered above"),
            Self::Edit { edit, .. } => {
                edit.title.as_ref().is_none_or(|t| &current.title == t)
                    && edit
                        .description
                        .as_ref()
                        .is_none_or(|d| &current.description == d)
                    && edit.label.is_none_or(|l| current.label == l)
            }
            Self::Transition { to, .. } => current.lifecycle == *to,
            Self::Move { parent, .. } => &current.parent == parent,
            Self::AddDependency { target, .. } => current.needs.contains(target),
            Self::RemoveDependency { target, .. } => !current.needs.contains(target),
            Self::Convert { kind, .. } => current.kind == *kind,
        })
    }
}

/// The one record an edit adds: a text edit, a label change, or both at once through the core's
/// single-record update, which keeps the parent and dependencies the records hold.
fn edit_record(
    records: &Store,
    view: &View,
    entity: &EntityId,
    edit: &Edit,
    context: Context,
) -> Result<Option<axon::lifecycle::record::Record>, Error> {
    let text = edit.title.is_some() || edit.description.is_some();
    let operations = records.prepared(view);
    match (text, edit.label) {
        (_, None) => operations.write(
            entity,
            edit.title.clone(),
            edit.description.clone(),
            None,
            context,
        ),
        (false, Some(label)) => operations.set_label(entity, label, None, context),
        (true, Some(label)) => {
            // Without a settled value the core's text edit gives the reason.
            let Some(current) = view.current(entity) else {
                return operations.write(entity, edit.title.clone(), None, None, context);
            };
            let value = Imported {
                title: edit.title.clone().unwrap_or_else(|| current.title.clone()),
                description: edit
                    .description
                    .clone()
                    .unwrap_or_else(|| current.description.clone()),
                label,
                parent: current.parent.clone(),
                needs: current.needs.clone(),
            };
            records.import(entity, value, context)
        }
    }
}
