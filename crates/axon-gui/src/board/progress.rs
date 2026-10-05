//! The lifecycle transitions the state menu offers for one Entity, each with whether the core
//! accepts it on the records read. The menu lists every operation that leads somewhere from
//! the Entity's kind and stored lifecycle, so a Group is never offered a start or a release.

use super::{Board, Change, Rejection};
use axon::lifecycle::{EntityId, Lifecycle, Operation};

/// The order the menu lists operations in: forward through the lifecycle, then back, then
/// ending the work.
pub const ORDER: [Operation; 8] = [
    Operation::Accept,
    Operation::Start,
    Operation::Complete,
    Operation::Release,
    Operation::Withdraw,
    Operation::Reopen,
    Operation::Reconsider,
    Operation::Cancel,
];

/// One transition the menu offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub operation: Operation,
    /// The stored lifecycle it leads to.
    pub to: Lifecycle,
    /// Whether the core accepts it on this read; the write checks again under the lock.
    pub check: Result<(), Rejection>,
}

impl Step {
    pub fn change(&self, entity: &EntityId) -> Change {
        Change::Transition {
            entity: entity.clone(),
            operation: self.operation,
            to: self.to,
        }
    }
}

/// The transitions of `id` on `board`, in [`ORDER`]; none while the Entity is conflicted or
/// unknown, since it has no lifecycle to start from.
pub(super) fn steps(board: &Board, id: &EntityId) -> Vec<Step> {
    let read = board.read();
    let Some(current) = read.current(id) else {
        return Vec::new();
    };
    ORDER
        .into_iter()
        .filter_map(|operation| {
            let to = operation.apply_as(current.kind, current.lifecycle).ok()?;
            let change = Change::Transition {
                entity: id.clone(),
                operation,
                to,
            };
            Some(Step {
                operation,
                to,
                check: change.check(board),
            })
        })
        .collect()
}
