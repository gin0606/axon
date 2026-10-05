//! In-memory record sets built through the core's operations, for tests.

use super::Board;
use axon::lifecycle::{
    Context, EntityId, Kind, Label, Lifecycle, Operation,
    record::{Current, Entry, Store},
};
use chrono::{DateTime, TimeDelta, Utc};
use std::collections::BTreeSet;

#[derive(Clone)]
pub struct Fixture {
    pub store: Store,
    clock: DateTime<Utc>,
    serial: usize,
}

impl Fixture {
    pub fn new() -> Self {
        Self {
            store: Store::new(),
            clock: DateTime::from_timestamp(1_800_000_000, 0).unwrap(),
            serial: 0,
        }
    }

    fn context(&mut self) -> Context {
        self.clock += TimeDelta::seconds(1);
        Context {
            at: self.clock,
            recorder: None,
        }
    }

    fn insert(&mut self, entry: Entry) {
        self.store.insert(entry).unwrap();
    }

    /// Creates an Entity through the core, NotStarted unless `lifecycle` says otherwise.
    pub fn create(
        &mut self,
        kind: Kind,
        lifecycle: Lifecycle,
        title: &str,
        label: Label,
        parent: Option<&EntityId>,
    ) -> EntityId {
        self.serial += 1;
        let id = EntityId::try_from(format!("axon-{:04}", self.serial)).unwrap();
        let context = self.context();
        let record = self
            .store
            .create(
                id.clone(),
                Current {
                    kind,
                    lifecycle,
                    owner: None,
                    title: title.into(),
                    description: String::new(),
                    label,
                    condition: None,
                    parent: parent.cloned(),
                    needs: BTreeSet::new(),
                },
                context,
            )
            .unwrap();
        self.insert(Entry::Record(record));
        id
    }

    pub fn issue(&mut self, title: &str, parent: Option<&EntityId>) -> EntityId {
        self.create(
            Kind::Issue,
            Lifecycle::NotStarted,
            title,
            Label::Feat,
            parent,
        )
    }

    pub fn group(&mut self, title: &str, parent: Option<&EntityId>) -> EntityId {
        self.create(
            Kind::Group,
            Lifecycle::NotStarted,
            title,
            Label::Feat,
            parent,
        )
    }

    pub fn perform(&mut self, id: &EntityId, operation: Operation) {
        let context = self.context();
        let record = self.store.perform(id, operation, None, context).unwrap();
        self.insert(Entry::Record(record));
    }

    pub fn describe(&mut self, id: &EntityId, description: &str) {
        let context = self.context();
        let record = self
            .store
            .write(id, None, Some(description.into()), None, context)
            .unwrap()
            .unwrap();
        self.insert(Entry::Record(record));
    }

    pub fn condition(&mut self, id: &EntityId, command: &str) {
        let context = self.context();
        let record = self
            .store
            .set_condition(id, Some(command.into()), None, context)
            .unwrap()
            .unwrap();
        self.insert(Entry::Record(record));
    }

    pub fn needs(&mut self, id: &EntityId, target: &EntityId) {
        let context = self.context();
        let record = self
            .store
            .add_dependency(id, target, None, context)
            .unwrap()
            .unwrap();
        self.insert(Entry::Record(record));
    }

    /// Moves `id` under `parent` in `store`, which may be a copy forked from this one, and
    /// returns the record for the caller to insert where it wants.
    pub fn parent_in(&mut self, store: &Store, id: &EntityId, parent: &EntityId) -> Entry {
        let context = self.context();
        Entry::Record(
            store
                .set_parent(id, Some(parent.clone()), None, context)
                .unwrap()
                .unwrap(),
        )
    }

    /// Performs `operation` on `id` in `store`, which may be a copy forked from this one.
    pub fn perform_in(&mut self, store: &Store, id: &EntityId, operation: Operation) -> Entry {
        let context = self.context();
        Entry::Record(store.perform(id, operation, None, context).unwrap())
    }

    pub fn insert_entry(&mut self, entry: Entry) {
        self.insert(entry);
    }

    pub fn note(&mut self, id: &EntityId, body: &str) {
        let context = self.context();
        let note = self.store.add_note(id, body.into(), None, context).unwrap();
        self.insert(Entry::Note(note));
    }

    pub fn board(&self) -> Board {
        Board::new("axon", self.store.clone(), self.store.view().unwrap())
    }
}
