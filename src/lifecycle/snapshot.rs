use super::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub(crate) store: StoreId,
    pub(crate) entities: BTreeMap<EntityId, Entity>,
    pub(crate) states: BTreeMap<RecordId, StateRecord>,
    pub(crate) notes: BTreeMap<RecordId, Note>,
}
impl Snapshot {
    pub fn new(store: StoreId) -> Self {
        Self {
            store,
            entities: BTreeMap::new(),
            states: BTreeMap::new(),
            notes: BTreeMap::new(),
        }
    }
    pub fn store(&self) -> &StoreId {
        &self.store
    }
    pub fn entities(&self) -> impl Iterator<Item = &Entity> {
        self.entities.values()
    }
    pub fn entity(&self, id: &EntityId) -> Result<&Entity> {
        self.entities
            .get(id)
            .ok_or_else(|| invalid(format!("missing Entity {id}")))
    }
    pub fn state_record(&self, id: &RecordId) -> Result<&StateRecord> {
        self.states
            .get(id)
            .ok_or_else(|| invalid(format!("missing state record {id}")))
    }
    pub fn note(&self, id: &RecordId) -> Result<&Note> {
        self.notes
            .get(id)
            .ok_or_else(|| invalid(format!("missing Note {id}")))
    }
    pub fn create(
        &mut self,
        id: EntityId,
        kind: Kind,
        current: Current,
        context: Context,
    ) -> Result<()> {
        current.validate()?;
        if !matches!(
            current.lifecycle,
            Lifecycle::Undecided | Lifecycle::NotStarted
        ) {
            return Err(invalid("creation requires Undecided or NotStarted"));
        }
        if self.entities.contains_key(&id) {
            return Err(invalid(format!("duplicate Entity {id}")));
        }
        let root = self.fresh_record_id();
        let record = StateRecord {
            id: root.clone(),
            entity: id.clone(),
            parents: BTreeSet::new(),
            context: context.clone(),
            event: StateEvent::Created {
                kind,
                initial: current.lifecycle,
            },
        };
        self.states.insert(root.clone(), record);
        self.entities.insert(
            id.clone(),
            Entity {
                id,
                kind,
                created_at: context.at,
                root: root.clone(),
                head: root,
                current,
            },
        );
        Ok(())
    }
    pub fn perform(
        &mut self,
        id: &EntityId,
        operation: Operation,
        reason: Option<String>,
        context: Context,
    ) -> Result<RecordId> {
        let entity = self.entity(id)?;
        let before = entity.current.lifecycle;
        let after = operation.apply(before)?;
        let record_id = self.fresh_record_id();
        let record = StateRecord {
            id: record_id.clone(),
            entity: id.clone(),
            parents: BTreeSet::from([entity.head.clone()]),
            context,
            event: StateEvent::Transition {
                operation,
                before,
                after,
                reason,
            },
        };
        self.states.insert(record_id.clone(), record);
        let entity = self.entities.get_mut(id).expect("checked Entity");
        entity.current.lifecycle = after;
        entity.head = record_id.clone();
        Ok(record_id)
    }
    pub fn write(
        &mut self,
        id: &EntityId,
        title: Option<String>,
        description: Option<String>,
    ) -> Result<()> {
        let entity = self.entity(id)?;
        if !entity.current.lifecycle.editable() {
            return Err(invalid("terminal text is fixed"));
        }
        let mut current = entity.current.clone();
        if let Some(title) = title {
            current.title = title;
        }
        if let Some(description) = description {
            current.description = description;
        }
        current.validate()?;
        self.entities.get_mut(id).expect("checked Entity").current = current;
        Ok(())
    }
    pub fn add_note(&mut self, id: &EntityId, body: String, context: Context) -> Result<RecordId> {
        self.entity(id)?;
        if body.trim().is_empty() {
            return Err(invalid("empty Note"));
        }
        let record_id = self.fresh_record_id();
        let parents = self.note_tips(id);
        self.notes.insert(
            record_id.clone(),
            Note {
                id: record_id.clone(),
                entity: id.clone(),
                parents,
                context,
                body,
            },
        );
        Ok(record_id)
    }
    fn fresh_record_id(&self) -> RecordId {
        loop {
            let id = RecordId::generate();
            if !self.states.contains_key(&id) && !self.notes.contains_key(&id) {
                return id;
            }
        }
    }
    fn note_tips(&self, entity: &EntityId) -> BTreeSet<RecordId> {
        let records: Vec<_> = self
            .notes
            .values()
            .filter(|n| &n.entity == entity)
            .collect();
        let mut tips: BTreeSet<_> = records.iter().map(|n| n.id.clone()).collect();
        for note in records {
            for parent in &note.parents {
                tips.remove(parent);
            }
        }
        tips
    }
    /// Returns causal order. Concurrent records use IDs solely as a deterministic
    /// presentation tie-breaker; `precedes` is the ordering relation.
    pub fn history(&self, entity: &EntityId) -> Result<Vec<&StateRecord>> {
        self.entity(entity)?;
        Ok(self
            .state_order()?
            .into_iter()
            .filter(|r| &r.entity == entity)
            .collect())
    }
    pub fn notes(&self, entity: &EntityId) -> Result<Vec<&Note>> {
        self.entity(entity)?;
        Ok(self
            .note_order()?
            .into_iter()
            .filter(|r| &r.entity == entity)
            .collect())
    }
    pub fn precedes(&self, before: &RecordId, after: &RecordId) -> Result<bool> {
        let (owner, stream) = if let Some(record) = self.states.get(after) {
            (&record.entity, true)
        } else if let Some(note) = self.notes.get(after) {
            (&note.entity, false)
        } else {
            return Err(invalid(format!("missing record {after}")));
        };
        let before_owner = self
            .states
            .get(before)
            .map(|r| (&r.entity, true))
            .or_else(|| self.notes.get(before).map(|r| (&r.entity, false)))
            .ok_or_else(|| invalid(format!("missing record {before}")))?;
        if before_owner != (owner, stream) || before == after {
            return Ok(false);
        }
        let mut visited = BTreeSet::new();
        let mut pending = vec![after];
        while let Some(id) = pending.pop() {
            if !visited.insert(id) {
                continue;
            }
            let parents = if stream {
                &self.state_record(id)?.parents
            } else {
                &self.note(id)?.parents
            };
            if parents.contains(before) {
                return Ok(true);
            }
            pending.extend(parents);
        }
        Ok(false)
    }
    /// Retains both inputs' immutable records and selects an entire current value
    /// for every Entity. This is explicit integration, not automatic three-way merge.
    /// Failure leaves both input snapshots unchanged.
    pub fn integrate(
        &self,
        other: &Self,
        choices: &BTreeMap<EntityId, Side>,
        reason: Option<String>,
        context: Context,
    ) -> Result<Self> {
        self.validate()?;
        other.validate()?;
        if self.store != other.store {
            return Err(invalid("different stores"));
        }
        let ids: BTreeSet<_> = self
            .entities
            .keys()
            .chain(other.entities.keys())
            .cloned()
            .collect();
        if ids != choices.keys().cloned().collect() {
            return Err(invalid("select every Entity exactly once"));
        }
        let mut merged = self.clone();
        union(&mut merged.states, &other.states)?;
        union(&mut merged.notes, &other.notes)?;
        if merged.states.keys().any(|id| merged.notes.contains_key(id)) {
            return Err(invalid("record ID reused across streams"));
        }
        for id in ids {
            let left = self.entities.get(&id);
            let right = other.entities.get(&id);
            let selected = match choices[&id] {
                Side::Left => left,
                Side::Right => right,
            }
            .ok_or_else(|| invalid(format!("selected absent Entity {id}")))?;
            if let (Some(left), Some(right)) = (left, right)
                && (left.root != right.root
                    || left.kind != right.kind
                    || left.created_at != right.created_at)
            {
                return Err(invalid(format!("Entity identity collision {id}")));
            }
            let inputs: Vec<_> = [left, right]
                .into_iter()
                .flatten()
                .map(|entity| Candidate {
                    head: entity.head.clone(),
                    current: entity.current.clone(),
                })
                .collect();
            let selection = if left.is_some() && choices[&id] == Side::Right {
                1
            } else {
                0
            };
            let record_id = merged.fresh_record_id();
            merged.states.insert(
                record_id.clone(),
                StateRecord {
                    id: record_id.clone(),
                    entity: id.clone(),
                    parents: inputs.iter().map(|c| c.head.clone()).collect(),
                    context: context.clone(),
                    event: StateEvent::Integration {
                        inputs,
                        selected: selection,
                        reason: reason.clone(),
                    },
                },
            );
            let mut current = selected.clone();
            current.head = record_id;
            merged.entities.insert(id, current);
        }
        merged.validate()?;
        Ok(merged)
    }
    pub fn validate(&self) -> Result<()> {
        if self.states.keys().any(|id| self.notes.contains_key(id)) {
            return Err(invalid("record ID reused across streams"));
        }
        let state_order = self.state_order()?;
        self.note_order()?;
        let mut roots = BTreeMap::new();
        for (id, entity) in &self.entities {
            if id != &entity.id {
                return Err(invalid("Entity key mismatch"));
            }
            entity.current.validate()?;
            let head = self.state_record(&entity.head)?;
            if head.entity != *id || head.event.result()? != entity.current.lifecycle {
                return Err(invalid("current lifecycle proof mismatch"));
            }
            self.validate_current_proof(&entity.current, head)?;
        }
        for record in state_order {
            if self.states.get(&record.id) != Some(record) {
                return Err(invalid("state key mismatch"));
            }
            let entity = self.entity(&record.entity)?;
            match &record.event {
                StateEvent::Created { kind, initial } => {
                    if !record.parents.is_empty()
                        || !matches!(initial, Lifecycle::Undecided | Lifecycle::NotStarted)
                        || record.id != entity.root
                        || *kind != entity.kind
                        || record.context.at != entity.created_at
                        || roots.insert(&record.entity, &record.id).is_some()
                    {
                        return Err(invalid("invalid Entity origin"));
                    }
                }
                StateEvent::Transition {
                    operation,
                    before,
                    after,
                    ..
                } => {
                    if record.parents.len() != 1 || operation.apply(*before)? != *after {
                        return Err(invalid("invalid lifecycle transition"));
                    }
                    let parent = self.state_record(record.parents.first().expect("one parent"))?;
                    if parent.event.result()? != *before {
                        return Err(invalid("transition does not continue its parent"));
                    }
                }
                StateEvent::Integration {
                    inputs, selected, ..
                } => {
                    if inputs.is_empty()
                        || *selected >= inputs.len()
                        || record.parents != inputs.iter().map(|c| c.head.clone()).collect()
                    {
                        return Err(invalid("invalid integration inputs"));
                    }
                    for input in inputs {
                        input.current.validate()?;
                        self.validate_current_proof(
                            &input.current,
                            self.state_record(&input.head)?,
                        )?;
                    }
                }
            }
            if record.id != entity.head && !self.precedes(&record.id, &entity.head)? {
                return Err(invalid("current head omits retained branch"));
            }
        }
        if roots.len() != self.entities.len() {
            return Err(invalid("missing Entity origin"));
        }
        for (id, note) in &self.notes {
            if id != &note.id {
                return Err(invalid("Note key mismatch"));
            }
            self.entity(&note.entity)?;
            if note.body.trim().is_empty() {
                return Err(invalid("empty Note"));
            }
        }
        Ok(())
    }
    fn validate_current_proof(&self, current: &Current, head: &StateRecord) -> Result<()> {
        if head.event.result()? != current.lifecycle {
            return Err(invalid("current lifecycle proof mismatch"));
        }
        if !current.lifecycle.editable()
            && let StateEvent::Integration {
                inputs, selected, ..
            } = &head.event
        {
            let chosen = &inputs[*selected].current;
            if current.title != chosen.title || current.description != chosen.description {
                return Err(invalid("terminal text differs from integration selection"));
            }
        }
        Ok(())
    }
    pub(crate) fn state_order(&self) -> Result<Vec<&StateRecord>> {
        order(&self.states, |record| (&record.entity, &record.parents))
    }
    pub(crate) fn note_order(&self) -> Result<Vec<&Note>> {
        order(&self.notes, |note| (&note.entity, &note.parents))
    }
}
fn union<T: Clone + PartialEq>(
    target: &mut BTreeMap<RecordId, T>,
    source: &BTreeMap<RecordId, T>,
) -> Result<()> {
    for (id, value) in source {
        if let Some(existing) = target.get(id) {
            if existing != value {
                return Err(invalid(format!("different content for record {id}")));
            }
        } else {
            target.insert(id.clone(), value.clone());
        }
    }
    Ok(())
}
fn order<T>(
    records: &BTreeMap<RecordId, T>,
    link: impl Fn(&T) -> (&EntityId, &BTreeSet<RecordId>),
) -> Result<Vec<&T>> {
    let mut pending = BTreeMap::new();
    let mut children: BTreeMap<&RecordId, Vec<&RecordId>> = BTreeMap::new();
    let mut ready = BTreeSet::new();
    for (id, record) in records {
        let (owner, parents) = link(record);
        for parent in parents {
            let predecessor = records
                .get(parent)
                .ok_or_else(|| invalid(format!("missing or cross-stream parent {parent}")))?;
            if link(predecessor).0 != owner {
                return Err(invalid("cross-Entity causal parent"));
            }
            children.entry(parent).or_default().push(id);
        }
        pending.insert(id, parents.len());
        if parents.is_empty() {
            ready.insert(id);
        }
    }
    let mut result = Vec::new();
    while let Some(id) = ready.pop_first() {
        result.push(&records[id]);
        for child in children.get(id).into_iter().flatten() {
            let count = pending.get_mut(child).expect("indexed child");
            *count -= 1;
            if *count == 0 {
                ready.insert(*child);
            }
        }
    }
    if result.len() != records.len() {
        return Err(invalid("causal cycle"));
    }
    Ok(result)
}
