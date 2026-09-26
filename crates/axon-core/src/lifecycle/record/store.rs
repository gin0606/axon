use super::model::{Entry, Note, Record, RecordId};
use super::view::View;
use super::{EntityId, Result, decode, encode};
use crate::lifecycle::invalid;
use std::collections::{BTreeMap, BTreeSet};

/// The record set of one store, keyed by record ID. Records are never changed or removed;
/// the storage adapter lists the record files and the CLI publishes the records this module
/// produces.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Store {
    entries: BTreeMap<RecordId, Entry>,
}
impl Store {
    pub fn new() -> Self {
        Self::default()
    }
    /// Adds a record under the ID of its canonical bytes. Adding a record twice is a no-op,
    /// because the same ID always carries the same content.
    pub fn insert(&mut self, entry: Entry) -> Result<RecordId> {
        let id = RecordId::of(&encode(&entry)?);
        self.entries.entry(id.clone()).or_insert(entry);
        Ok(id)
    }
    /// Decodes one record file and adds it. The adapter compares the returned ID with the
    /// file name.
    pub fn insert_bytes(&mut self, bytes: &[u8]) -> Result<RecordId> {
        let (id, entry) = decode(bytes)?;
        self.entries.entry(id.clone()).or_insert(entry);
        Ok(id)
    }
    /// Adds every record of another set. Records with the same ID are the same record.
    pub fn absorb(&mut self, other: &Store) {
        for (id, entry) in &other.entries {
            self.entries
                .entry(id.clone())
                .or_insert_with(|| entry.clone());
        }
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn contains(&self, id: &RecordId) -> bool {
        self.entries.contains_key(id)
    }
    pub fn get(&self, id: &RecordId) -> Option<&Entry> {
        self.entries.get(id)
    }
    pub fn record(&self, id: &RecordId) -> Option<&Record> {
        self.entries.get(id).and_then(Entry::as_record)
    }
    pub fn note(&self, id: &RecordId) -> Option<&Note> {
        self.entries.get(id).and_then(Entry::as_note)
    }
    pub fn entries(&self) -> impl Iterator<Item = (&RecordId, &Entry)> {
        self.entries.iter()
    }
    pub fn records(&self) -> impl Iterator<Item = (&RecordId, &Record)> {
        self.entries
            .iter()
            .filter_map(|(id, entry)| entry.as_record().map(|record| (id, record)))
    }
    pub fn notes(&self) -> impl Iterator<Item = (&RecordId, &Note)> {
        self.entries
            .iter()
            .filter_map(|(id, entry)| entry.as_note().map(|note| (id, note)))
    }
    /// The records that do not continue their present parent record, with the reason each
    /// is corruption. Empty for every set a writer produced.
    pub fn problems(&self) -> Vec<(RecordId, crate::lifecycle::Error)> {
        super::view::record_problems(self)
    }
    /// Derives heads, conflicts, current values, gaps and violations from the whole set.
    /// Fails only on corruption that no writer produces: a parent that is a Note or belongs to
    /// another Entity, a causal cycle, or a record that does not continue its parent.
    pub fn view(&self) -> Result<View> {
        View::derive(self)
    }

    /// The Entity's records other than Notes in causal order. Concurrent records are ordered
    /// by ID for a stable presentation only; `precedes` is the ordering relation. A parent
    /// missing from the set (a gap) does not order anything.
    pub fn history(&self, entity: &EntityId) -> Result<Vec<(&RecordId, &Record)>> {
        let records: BTreeMap<&RecordId, &Record> = self
            .records()
            .filter(|(_, record)| &record.entity == entity)
            .collect();
        let mut pending = BTreeMap::new();
        let mut children: BTreeMap<&RecordId, Vec<&RecordId>> = BTreeMap::new();
        let mut ready = BTreeSet::new();
        for (id, record) in &records {
            let present: Vec<_> = record
                .parents
                .iter()
                .filter(|parent| records.contains_key(parent))
                .collect();
            for parent in &present {
                children.entry(parent).or_default().push(id);
            }
            pending.insert(*id, present.len());
            if present.is_empty() {
                ready.insert(*id);
            }
        }
        let mut order = Vec::new();
        while let Some(id) = ready.pop_first() {
            order.push((id, records[id]));
            for child in children.get(id).into_iter().flatten() {
                let count = pending.get_mut(child).expect("indexed child");
                *count -= 1;
                if *count == 0 {
                    ready.insert(child);
                }
            }
        }
        if order.len() != records.len() {
            return Err(invalid(format!("causal cycle in the records of {entity}")));
        }
        Ok(order)
    }
    /// Whether `before` is an ancestor of `after` through parent links present in the set.
    /// Time never orders records; only parents do.
    pub fn precedes(&self, before: &RecordId, after: &RecordId) -> bool {
        if before == after || !self.contains(before) {
            return false;
        }
        let mut visited = BTreeSet::new();
        let mut pending = vec![after];
        while let Some(id) = pending.pop() {
            if !visited.insert(id) {
                continue;
            }
            let Some(record) = self.record(id) else {
                continue;
            };
            if record.parents.contains(before) {
                return true;
            }
            pending.extend(record.parents.iter());
        }
        false
    }
    /// The Entity's Notes by time, then by ID.
    pub fn notes_of(&self, entity: &EntityId) -> Vec<(&RecordId, &Note)> {
        let mut notes: Vec<_> = self
            .notes()
            .filter(|(_, note)| &note.entity == entity)
            .collect();
        notes.sort_by(|a, b| (a.1.at, a.0).cmp(&(b.1.at, b.0)));
        notes
    }
    /// Every Note by time, then by ID.
    pub fn all_notes(&self) -> Vec<(&RecordId, &Note)> {
        let mut notes: Vec<_> = self.notes().collect();
        notes.sort_by(|a, b| (a.1.at, a.0).cmp(&(b.1.at, b.0)));
        notes
    }
}
