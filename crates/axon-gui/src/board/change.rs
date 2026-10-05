//! The changes the window writes to one project: a lifecycle transition, or a structural change
//! (moving an Entity under another Group or out of any, adding or removing a dependency,
//! converting between Issue and Group). Every rule is the core's: this module names a change,
//! runs the core's operation for it against a set of records, and tells whether a later read
//! shows it.

use super::Board;
use axon::lifecycle::{
    Context, EntityId, Error, Kind, Lifecycle, Operation, Refusal,
    record::{Record, Store},
};
use chrono::DateTime;

/// One change of `entity`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
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
            Self::Transition { .. } => Section::Progress,
            Self::Move { .. } | Self::Convert { .. } => Section::Structure,
            Self::AddDependency { .. } | Self::RemoveDependency { .. } => Section::Dependencies,
        }
    }

    pub fn entity(&self) -> &EntityId {
        match self {
            Self::Transition { entity, .. }
            | Self::Move { entity, .. }
            | Self::AddDependency { entity, .. }
            | Self::RemoveDependency { entity, .. }
            | Self::Convert { entity, .. } => entity,
        }
    }

    /// The record the change adds to `records`, by the core's operation for it; none when the
    /// records show a structural change already.
    pub fn record(&self, records: &Store, context: Context) -> Result<Option<Record>, Error> {
        match self {
            Self::Transition {
                entity, operation, ..
            } => records.perform(entity, *operation, None, context).map(Some),
            Self::Move { entity, parent } => {
                records.set_parent(entity, parent.clone(), None, context)
            }
            Self::AddDependency { entity, target } => {
                records.add_dependency(entity, target, None, context)
            }
            Self::RemoveDependency { entity, target } => {
                records.remove_dependency(entity, target, None, context)
            }
            Self::Convert { entity, kind } => records.convert(entity, *kind, None, context),
        }
    }

    /// Whether the core accepts the change on the records of `board`. The write checks again
    /// against the records it finds under the store's lock; this only answers before writing.
    pub fn check(&self, board: &Board) -> Result<(), Rejection> {
        // The record is discarded, so its time is never seen.
        let context = Context {
            at: DateTime::UNIX_EPOCH,
            recorder: None,
        };
        self.record(board.records(), context)
            .map(|_| ())
            .map_err(Rejection::from)
    }

    /// Whether the settled value `board` shows of the Entity has the change made, for telling
    /// what a publication of unknown outcome left; none when the Entity is conflicted or not
    /// in the store, so its value cannot tell.
    pub fn is_shown_by(&self, board: &Board) -> Option<bool> {
        let read = board.read();
        let current = read.current(self.entity())?;
        Some(match self {
            Self::Transition { to, .. } => current.lifecycle == *to,
            Self::Move { parent, .. } => &current.parent == parent,
            Self::AddDependency { target, .. } => current.needs.contains(target),
            Self::RemoveDependency { target, .. } => !current.needs.contains(target),
            Self::Convert { kind, .. } => current.kind == *kind,
        })
    }
}
