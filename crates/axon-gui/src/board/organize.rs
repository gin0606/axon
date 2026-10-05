//! Choosing what a structural change names: the Group to move an Entity under, or a
//! dependency to add. Whether the core accepts a choice is decided when it is made.

use super::{Board, Change, Link};
use axon::lifecycle::{EntityId, Kind};

/// What a picker chooses for an Entity: the Group to put it under, or a dependency to add.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    Parent,
    Dependency,
}

impl Purpose {
    /// The change choosing `target` makes for `entity`.
    pub fn change(self, entity: EntityId, target: EntityId) -> Change {
        match self {
            Self::Parent => Change::Move {
                entity,
                parent: Some(target),
            },
            Self::Dependency => Change::AddDependency { entity, target },
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
