//! The records of one project as values the window lists, filters and inspects.
//!
//! A [`Board`] owns one read of a store. [`filter`] decides which Entities match,
//! [`listing`] lays them out as a tree or a flat list, [`detail`] gathers what the detail
//! pane shows of one Entity, and [`organize`] names the structural changes the core makes. Everything here is a pure derivation from the records: no GPUI,
//! no disk access, and no condition command is ever run.

pub mod detail;
pub mod explorer;
pub mod filter;
pub mod listing;
pub mod organize;

pub use detail::{Change, EntityDetail, HistoryEntry, Link, NoteEntry, Structure, Wait, WaitKind};
pub use explorer::Explorer;
pub use filter::Filter;
pub use listing::{Layout, ListRow, Listing};
pub use organize::{Purpose, Rearrangement, Rejection};

use axon::lifecycle::{EntityId, Kind, Label, Lifecycle, record};
use axon::read;
use std::collections::HashMap;

/// The lifecycle a list shows and filters by: the effective value (InProgress for a NotStarted
/// Group with a direct child effectively InProgress or Completed, the stored value otherwise), or
/// `Conflicted` for an Entity with several heads, which has no value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum State {
    Undecided,
    NotStarted,
    InProgress,
    Completed,
    Cancelled,
    Conflicted,
}
impl State {
    pub const ALL: [State; 6] = [
        State::Undecided,
        State::NotStarted,
        State::InProgress,
        State::Completed,
        State::Cancelled,
        State::Conflicted,
    ];
    pub fn of(lifecycle: Option<Lifecycle>) -> Self {
        match lifecycle {
            Some(Lifecycle::Undecided) => Self::Undecided,
            Some(Lifecycle::NotStarted) => Self::NotStarted,
            Some(Lifecycle::InProgress) => Self::InProgress,
            Some(Lifecycle::Completed) => Self::Completed,
            Some(Lifecycle::Cancelled) => Self::Cancelled,
            None => Self::Conflicted,
        }
    }
}

/// One Entity as a list row shows it. A conflicted Entity shows its first head's value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub id: EntityId,
    pub kind: Kind,
    pub label: Label,
    pub title: String,
    pub state: State,
    /// The situation with every condition taken as satisfied.
    pub status: read::Status,
    /// The parent as recorded, whether or not the store holds it.
    pub parent: Option<EntityId>,
    /// The Entity is attributed a structural violation.
    pub invalid: bool,
}

/// One read of a project's store with the rows derived from it.
pub struct Board {
    records: record::Store,
    view: record::View,
    /// Every known Entity in creation order (ties by ID).
    items: Vec<Item>,
    index: HashMap<EntityId, usize>,
    /// Every Entity in tree order with its depth, which no filter changes.
    tree: Vec<(EntityId, usize)>,
}

impl Board {
    pub fn new(records: record::Store, view: record::View) -> Self {
        let mut board = Self {
            records,
            view,
            items: Vec::new(),
            index: HashMap::new(),
            tree: Vec::new(),
        };
        let items: Vec<Item> = {
            let read = board.read();
            read::list(&read, |_, _| true, None)
                .into_iter()
                .map(|row| {
                    let presented = read.presented(row.id).expect("a listed Entity is known");
                    Item {
                        id: row.id.clone(),
                        kind: row.kind,
                        label: row.label,
                        title: row.title.to_owned(),
                        state: State::of(read.effective(row.id)),
                        status: row.status,
                        parent: presented.parent.clone(),
                        invalid: row.invalid,
                    }
                })
                .collect()
        };
        board.index = items
            .iter()
            .enumerate()
            .map(|(ix, item)| (item.id.clone(), ix))
            .collect();
        board.items = items;
        board.tree = listing::tree(&board);
        board
    }

    /// The core's read API over these records.
    pub fn read(&self) -> read::View<'_> {
        read::View::new(&self.records, &self.view)
    }
    pub fn records(&self) -> &record::Store {
        &self.records
    }
    pub fn items(&self) -> &[Item] {
        &self.items
    }
    pub(crate) fn tree(&self) -> &[(EntityId, usize)] {
        &self.tree
    }
    pub fn item(&self, id: &EntityId) -> Option<&Item> {
        self.index.get(id).map(|ix| &self.items[*ix])
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    /// Whether the Entity matches `filter`; an Entity the store does not hold matches nothing.
    pub fn matches(&self, filter: &Filter, id: &EntityId) -> bool {
        let read = self.read();
        match (self.item(id), read.presented(id)) {
            (Some(item), Some(presented)) => filter.matches(item, presented),
            _ => false,
        }
    }
    /// The detail of a known Entity, without running any condition.
    pub fn detail(&self, id: &EntityId) -> axon::lifecycle::Result<EntityDetail> {
        detail::of(self, id)
    }
}

#[cfg(test)]
pub(crate) mod fixture;
#[cfg(test)]
mod tests;
