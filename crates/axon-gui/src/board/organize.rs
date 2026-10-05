//! Rearranging the structure of one project: moving an Entity under another Group or out of
//! any, adding or removing a dependency, and converting between Issue and Group. Every rule is
//! the core's: this module names a change, runs the core's operation for it against a set of
//! records, and tells whether a later read shows it.

use super::{Board, Link};
use axon::lifecycle::{
    Context, EntityId, Error, Kind, Refusal,
    record::{Record, Store},
};
use chrono::DateTime;

/// One structural change of `entity`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Rearrangement {
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

/// Why the core refused a change: a structural refusal naming the Entities involved, or the
/// core's message for any other rule.
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

impl Rearrangement {
    /// Whether the change is to a dependency rather than to where the Entity is or what kind.
    pub fn is_dependency(&self) -> bool {
        matches!(
            self,
            Self::AddDependency { .. } | Self::RemoveDependency { .. }
        )
    }

    pub fn entity(&self) -> &EntityId {
        match self {
            Self::Move { entity, .. }
            | Self::AddDependency { entity, .. }
            | Self::RemoveDependency { entity, .. }
            | Self::Convert { entity, .. } => entity,
        }
    }

    /// The record the change adds to `records`, by the core's operation for it; none when the
    /// records show the change already.
    pub fn record(&self, records: &Store, context: Context) -> Result<Option<Record>, Error> {
        match self {
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
            Self::Move { parent, .. } => &current.parent == parent,
            Self::AddDependency { target, .. } => current.needs.contains(target),
            Self::RemoveDependency { target, .. } => !current.needs.contains(target),
            Self::Convert { kind, .. } => current.kind == *kind,
        })
    }
}

/// What a picker chooses for an Entity: the Group to put it under, or a dependency to add.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    Parent,
    Dependency,
}

impl Purpose {
    /// The change choosing `target` makes for `entity`.
    pub fn change(self, entity: EntityId, target: EntityId) -> Rearrangement {
        match self {
            Self::Parent => Rearrangement::Move {
                entity,
                parent: Some(target),
            },
            Self::Dependency => Rearrangement::AddDependency { entity, target },
        }
    }
}

/// The Entities of `board` a picker offers for `entity`, in creation order: Groups for a
/// parent, any Entity for a dependency, leaving out the Entity itself and what it already has.
/// `query` narrows them to those whose title or ID contains it. Returns the first `limit` of
/// them and how many there are. Whether the core accepts one is decided when it is chosen.
pub fn candidates(
    board: &Board,
    entity: &EntityId,
    purpose: Purpose,
    query: &str,
    limit: usize,
) -> (Vec<Link>, usize) {
    let read = board.read();
    let current = read.presented(entity);
    let has = |id: &EntityId| match purpose {
        Purpose::Parent => current.is_some_and(|c| c.parent.as_ref() == Some(id)),
        Purpose::Dependency => current.is_some_and(|c| c.needs.contains(id)),
    };
    let mut shown = Vec::new();
    let mut total = 0;
    for item in board.items() {
        let offered = &item.id != entity
            && !has(&item.id)
            && (purpose == Purpose::Dependency || item.kind == Kind::Group)
            && (query.is_empty() || item.title.contains(query) || item.id.as_ref().contains(query));
        if offered {
            total += 1;
            if shown.len() < limit {
                shown.push(board.link(&item.id));
            }
        }
    }
    (shown, total)
}
