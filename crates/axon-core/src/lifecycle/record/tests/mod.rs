//! Tests of the record set. `scenarios` replays the six `run` tests of
//! `spec/record_integration_test.qnt` on conflicts, gaps, terminal cycles, mutual Completed
//! dependencies, duplicate creation and gap conflicts, `witnesses` the main witnesses of
//! `spec/record_integration.qnt`, and the rest cover conversion across kinds, the rejection
//! rules of ordinary operations (including the `run` tests on waived lifecycle operations,
//! the broken ancestor chain, cycles among waiting Entities and new edges on a cycle), the
//! lifecycle, relation, text and Note rules on a single store, the codec and order
//! independence.
use super::*;
use chrono::{TimeZone, Utc};
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};

mod codec;
mod conversion;
mod integration_pbt;
mod lifecycle_pbt;
mod operations;
mod ordering;
mod rules;
mod scenarios;
mod witnesses;

fn id(text: &str) -> EntityId {
    text.to_string().try_into().unwrap()
}
fn actor(name: &str) -> Option<Recorder> {
    Some(Recorder {
        actor: name.into(),
        data: BTreeMap::from([("session_id".into(), serde_json::json!("session-1"))]),
    })
}
/// A distinct time per call site keeps records of the same operation on two replicas
/// distinct, as their real timestamps would.
fn ctx(seconds: i64, who: &str) -> Context {
    Context {
        at: Utc.timestamp_opt(seconds, 500_000_000).unwrap(),
        recorder: actor(who),
    }
}
fn current(kind: Kind, lifecycle: Lifecycle, parent: Option<&str>) -> Current {
    Current {
        kind,
        lifecycle,
        owner: None,
        title: "task".into(),
        description: "body\n日本語".into(),
        condition: None,
        parent: parent.map(id),
        needs: BTreeSet::new(),
    }
}
fn insert(store: &mut Store, record: Record) -> RecordId {
    store.insert(Entry::Record(record)).unwrap()
}

/// The base of the model: Group g0, Issue i1 under g0, Group g2 and Issue i3, all NotStarted.
/// A fresh clock starts after the base so every later record is distinct.
struct Replica {
    store: Store,
    clock: Cell<i64>,
    name: String,
}
impl Replica {
    fn base() -> Store {
        let mut store = Store::new();
        let base = [
            ("g0", Kind::Group, None),
            ("i1", Kind::Issue, Some("g0")),
            ("g2", Kind::Group, None),
            ("i3", Kind::Issue, None),
        ];
        for (t, (name, kind, parent)) in base.into_iter().enumerate() {
            let record = store
                .create(
                    id(name),
                    current(kind, Lifecycle::NotStarted, parent),
                    ctx(t as i64, "setup"),
                )
                .unwrap();
            insert(&mut store, record);
        }
        store
    }
    fn new(name: &str) -> Self {
        Self {
            store: Self::base(),
            clock: Cell::new(100 + if name == "r0" { 0 } else { 1000 }),
            name: name.into(),
        }
    }
    fn from(name: &str, store: &Store) -> Self {
        let mut replica = Self::new(name);
        replica.store = store.clone();
        replica
    }
    fn tick(&self) -> Context {
        self.tick_as(&self.name)
    }
    fn tick_as(&self, who: &str) -> Context {
        self.clock.set(self.clock.get() + 1);
        ctx(self.clock.get(), who)
    }
    fn view(&self) -> View {
        self.store.view().unwrap()
    }
    fn current(&self, name: &str) -> Current {
        self.view().current(&id(name)).unwrap().clone()
    }
    fn head(&self, name: &str) -> RecordId {
        self.view().head(&id(name)).unwrap().clone()
    }
    fn op_as(&mut self, name: &str, operation: Operation, who: &str) -> RecordId {
        let context = self.tick_as(who);
        let record = self
            .store
            .perform(&id(name), operation, None, context)
            .unwrap();
        insert(&mut self.store, record)
    }
    fn op(&mut self, name: &str, operation: Operation) -> RecordId {
        let who = self.name.clone();
        self.op_as(name, operation, &who)
    }
    fn try_op(&mut self, name: &str, operation: Operation) -> Result<Record> {
        let context = self.tick();
        self.store.perform(&id(name), operation, None, context)
    }
    fn move_to(&mut self, name: &str, parent: Option<&str>) -> RecordId {
        let context = self.tick();
        let record = self
            .store
            .set_parent(&id(name), parent.map(id), context)
            .unwrap()
            .expect("a change");
        insert(&mut self.store, record)
    }
    fn try_move(&mut self, name: &str, parent: Option<&str>) -> Result<Option<Record>> {
        let context = self.tick();
        self.store.set_parent(&id(name), parent.map(id), context)
    }
    fn add_dep(&mut self, name: &str, target: &str) -> RecordId {
        let context = self.tick();
        let record = self
            .store
            .add_dependency(&id(name), &id(target), context)
            .unwrap()
            .expect("a change");
        insert(&mut self.store, record)
    }
    fn try_add_dep(&mut self, name: &str, target: &str) -> Result<Option<Record>> {
        let context = self.tick();
        self.store.add_dependency(&id(name), &id(target), context)
    }
    fn remove_dep(&mut self, name: &str, target: &str) -> RecordId {
        let context = self.tick();
        let record = self
            .store
            .remove_dependency(&id(name), &id(target), context)
            .unwrap()
            .expect("a change");
        insert(&mut self.store, record)
    }
    fn try_remove_dep(&mut self, name: &str, target: &str) -> Result<Option<Record>> {
        let context = self.tick();
        self.store
            .remove_dependency(&id(name), &id(target), context)
    }
    fn create(
        &mut self,
        name: &str,
        kind: Kind,
        lifecycle: Lifecycle,
        parent: Option<&str>,
    ) -> RecordId {
        let context = self.tick();
        let record = self
            .store
            .create(id(name), current(kind, lifecycle, parent), context)
            .unwrap();
        insert(&mut self.store, record)
    }
    fn try_create(&mut self, name: &str, value: Current) -> Result<Record> {
        let context = self.tick();
        self.store.create(id(name), value, context)
    }
    fn resolve(&mut self, name: &str, chosen: &RecordId) -> RecordId {
        let context = self.tick();
        let record = self
            .store
            .resolve(&id(name), chosen, Some("pick".into()), context)
            .unwrap();
        insert(&mut self.store, record)
    }
    fn note(&mut self, name: &str, body: &str) -> RecordId {
        let context = self.tick();
        let note = self
            .store
            .add_note(&id(name), body.into(), None, context)
            .unwrap();
        self.store.insert(Entry::Note(note)).unwrap()
    }
    /// Takes every record the other replica has: merge, squash merge or rebase merge.
    fn sync(&mut self, other: &Replica) {
        self.store.absorb(&other.store);
    }
    /// Takes one record only: cherry-pick.
    fn sync_one(&mut self, other: &Replica, record: &RecordId) {
        self.store
            .insert(other.store.get(record).unwrap().clone())
            .unwrap();
    }
    fn violations(&self, name: &str) -> BTreeSet<ViolationKind> {
        self.view()
            .violations()
            .iter()
            .filter(|v| v.entity == id(name))
            .map(|v| v.kind)
            .collect()
    }
    fn heads(&self, name: &str) -> BTreeSet<RecordId> {
        self.view().heads(&id(name)).unwrap().clone()
    }
}
fn lifecycle(view: &View, name: &str) -> Lifecycle {
    view.current(&id(name)).unwrap().lifecycle
}
fn error(result: Result<impl std::fmt::Debug>) -> String {
    match result {
        Ok(value) => panic!("expected rejection, got {value:?}"),
        Err(error) => error.to_string(),
    }
}
