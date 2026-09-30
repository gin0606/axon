use super::*;
use crate::lifecycle::Label;
use crate::lifecycle::record::Record as StoreRecord;
use crate::lifecycle::record::{Context, Entry, RecordKind, Store};

fn id(value: &str) -> Result<EntityId> {
    value
        .to_owned()
        .try_into()
        .map_err(|e| invalid(format!("identity/reference: {e}")))
}
fn record_id(record: &Record) -> Result<EntityId> {
    id(record
        .id
        .as_deref()
        .ok_or_else(|| invalid("identity/reference: id: run axon import prepare FILE"))?)
}
/// The final value a declaration record asks for. The condition is the stored one, kept.
struct Desired {
    kind: Kind,
    lifecycle: Lifecycle,
    title: String,
    description: String,
    parent: Option<EntityId>,
    needs: BTreeSet<EntityId>,
}
impl Desired {
    fn matches(&self, current: &Current) -> bool {
        current.kind == self.kind
            && current.lifecycle == self.lifecycle
            && current.title == self.title
            && current.description == self.description
            && current.parent == self.parent
            && current.needs == self.needs
    }
}
/// How the retry rule classifies one Entity of the declaration against the store.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Applied {
    /// The store already holds the final value; `changed` when that value differs from the
    /// base (or the Entity is new), so an apply must have put it there.
    Done { changed: bool },
    /// The store holds the base value (or, for a new Entity, nothing).
    Pending,
}
impl Declaration {
    fn typed_records(&self) -> impl Iterator<Item = (Kind, &Record)> {
        self.groups
            .iter()
            .map(|r| (Kind::Group, r))
            .chain(self.issues.iter().map(|r| (Kind::Issue, r)))
    }
    fn resolved(&self, reference: &Reference) -> Result<EntityId> {
        let (tag, value) = self.target(reference)?;
        if tag != 0 {
            return Err(invalid(format!(
                "identity/reference: key {value}: run axon import prepare FILE"
            )));
        }
        id(&value)
    }
    fn desired(&self, kind: Kind, r: &Record) -> Result<Desired> {
        Ok(Desired {
            kind,
            lifecycle: match r.lifecycle.as_str() {
                "undecided" => Lifecycle::Undecided,
                "not-started" => Lifecycle::NotStarted,
                "in-progress" => Lifecycle::InProgress,
                "completed" => Lifecycle::Completed,
                "cancelled" => Lifecycle::Cancelled,
                _ => return Err(invalid("schema: lifecycle")),
            },
            title: r.title.clone(),
            description: r.description.clone(),
            parent: r.parent.as_ref().map(|p| self.resolved(p)).transpose()?,
            needs: r
                .needs
                .iter()
                .map(|p| self.resolved(p))
                .collect::<Result<_>>()?,
        })
    }
    fn existing_identities(&self, view: &View) -> Result<()> {
        for (kind, r) in self.typed_records().filter(|(_, r)| r.base.is_some()) {
            let id = record_id(r)?;
            let current = settled(view, &id)?;
            if current.kind != kind {
                return Err(invalid(format!("read-only: {id}: kind is fixed")));
            }
        }
        Ok(())
    }
    /// The retry rule for one Entity: done when the store holds the final value, pending when
    /// it holds the base (or nothing, for a new Entity), a conflict otherwise.
    fn applied(&self, view: &View, kind: Kind, r: &Record) -> Result<Applied> {
        let id = record_id(r)?;
        let desired = self.desired(kind, r)?;
        match (&r.base, view.current(&id)) {
            (base, Some(current)) if desired.matches(current) => Ok(Applied::Done {
                changed: base
                    .as_ref()
                    .is_none_or(|base| *base != fingerprint(&id, current)),
            }),
            (Some(base), Some(current)) if *base == fingerprint(&id, current) => {
                Ok(Applied::Pending)
            }
            (Some(_), Some(_)) => Err(invalid(format!("conflict: {id}: base mismatch"))),
            (Some(_), None) => Err(invalid(format!(
                "identity/reference: {id}: ID does not exist in storage"
            ))),
            (None, Some(_)) => Err(invalid(format!("conflict: {id}: new id already exists"))),
            (None, None) => Ok(Applied::Pending),
        }
    }
    fn external_ids(&self) -> Result<BTreeSet<EntityId>> {
        let selected = self
            .records()
            .map(record_id)
            .collect::<Result<BTreeSet<_>>>()?;
        let mut ids = BTreeSet::new();
        for r in self.records() {
            for reference in r.parent.iter().chain(&r.needs) {
                let target = self.resolved(reference)?;
                if !selected.contains(&target) {
                    ids.insert(target);
                }
            }
        }
        Ok(ids)
    }
    /// Regenerates `references` from the store; every external Entity must exist and be
    /// settled.
    pub fn refresh_references(&mut self, view: &View) -> Result<()> {
        self.references = self
            .external_ids()?
            .into_iter()
            .map(|id| {
                let c = settled(view, &id)?;
                Ok(External {
                    id: id.to_string(),
                    kind: kind(c.kind).into(),
                    lifecycle: lifecycle(c.lifecycle).into(),
                    title: c.title.clone(),
                })
            })
            .collect::<Result<_>>()?;
        Ok(())
    }
    pub fn refresh_applied(&mut self, view: &View) -> Result<()> {
        for r in self.groups.iter_mut().chain(&mut self.issues) {
            let id = record_id(r)?;
            let c = settled(view, &id)?;
            r.base = Some(fingerprint(&id, c));
            r.lifecycle = lifecycle(c.lifecycle).into();
        }
        self.refresh_references(view)
    }
    /// Whether the declaration is the retry of an interrupted apply: some Entity it changes
    /// (a new one, or one whose base differs from its final value) already holds the final
    /// value. An Entity the declaration leaves as it is says nothing.
    fn is_retry(&self, view: &View) -> bool {
        self.typed_records().any(|(kind, r)| {
            matches!(
                self.applied(view, kind, r),
                Ok(Applied::Done { changed: true })
            )
        })
    }
    /// Whether every Entity already holds its final value and some final value differs from
    /// the base: an earlier run applied the declaration, and nothing remains to publish.
    fn fully_applied(&self, view: &View) -> bool {
        let mut changed = false;
        for (kind, r) in self.typed_records() {
            match self.applied(view, kind, r) {
                Ok(Applied::Done { changed: c }) => changed |= c,
                _ => return false,
            }
        }
        changed
    }
    /// Rejects a settled store with violations unless the declaration is the retry of an
    /// interrupted apply that is already complete or whose remaining records remove every
    /// violation. Returns the candidate that decided it, so a caller need not build it again.
    fn require_valid_store(
        &self,
        store: &Store,
        view: &View,
        context: &Context,
    ) -> Result<Option<Candidate>> {
        if view.violations().is_empty() {
            return Ok(None);
        }
        if self.is_retry(view) {
            // The violation is another writer's: this declaration adds no record to it.
            if self.fully_applied(view) {
                return Ok(None);
            }
            let candidate = self.candidate(store, view, context.clone())?;
            if candidate.after.violations().is_empty() {
                return Ok(Some(candidate));
            }
        }
        let ids: Vec<_> = view
            .violations()
            .iter()
            .map(|v| format!("{} ({})", v.entity, v.kind.label()))
            .collect();
        Err(invalid(format!(
            "core rejection: the store has structural violations; repair them first: {}",
            ids.join(", ")
        )))
    }
    pub fn prepare(&mut self, store: &Store, prefix: &str) -> Result<()> {
        self.validate()?;
        let view = store.view().map_err(|e| invalid(e.to_string()))?;
        require_settled(&view)?;
        self.existing_identities(&view)?;
        // Preserve key references when a colliding provisional ID must be replaced.
        let aliases: Vec<_> = self
            .records()
            .filter_map(|r| Some((r.id.clone()?, r.key.clone()?)))
            .collect();
        for r in self.groups.iter_mut().chain(&mut self.issues) {
            for reference in r.parent.iter_mut().chain(&mut r.needs) {
                if let Reference::Id(value) = reference
                    && let Some((_, key)) = aliases.iter().find(|(id, _)| id == &value.id)
                {
                    *reference = Reference::key(key);
                }
            }
        }
        // IDs with any record, including Notes without other records, are taken.
        let mut used: BTreeSet<String> = view
            .known()
            .chain(view.noted_only())
            .map(ToString::to_string)
            .chain(self.records().filter_map(|r| r.id.clone()))
            .collect();
        // An assigned ID that already exists is kept when that Entity holds the declared
        // final value (an interrupted apply published it); otherwise another writer took the
        // ID and a fresh one is assigned. A fresh ID changes the final value of every record
        // that refers to it, so the decision is repeated until nothing more is reassigned.
        loop {
            let mut reassign = BTreeSet::new();
            for (kind, r) in self.typed_records() {
                if r.base.is_none()
                    && let Some(value) = &r.id
                    && let Ok(existing) = id(value)
                    && (view.is_known(&existing) || view.noted_only().contains(&existing))
                    && !matches!(self.applied(&view, kind, r), Ok(Applied::Done { .. }))
                {
                    reassign.insert(value.clone());
                }
            }
            let mut assigned = false;
            for r in self.groups.iter_mut().chain(&mut self.issues) {
                if r.base.is_none() && r.id.as_ref().is_none_or(|v| reassign.contains(v)) {
                    loop {
                        let candidate = crate::lifecycle::record::new_entity_id(prefix)
                            .map_err(|e| invalid(e.to_string()))?
                            .to_string();
                        if used.insert(candidate.clone()) {
                            r.id = Some(candidate);
                            assigned = true;
                            break;
                        }
                    }
                }
            }
            if !assigned {
                break;
            }
        }
        self.refresh_references(&view)?;
        self.validate()?;
        let context = Context {
            at: chrono::Utc::now(),
            recorder: None,
        };
        self.require_valid_store(store, &view, &context)?;
        Ok(())
    }
    pub fn check(&self, input: &str, store: &Store, context: Context) -> Result<Checked> {
        self.validate()?;
        let view = store.view().map_err(|e| invalid(e.to_string()))?;
        require_settled(&view)?;
        self.existing_identities(&view)?;
        for r in self.records() {
            record_id(r)?;
        }
        if self.serialize(&view)? != input {
            return Err(invalid(
                "schema: non-canonical declaration; run axon import prepare FILE",
            ));
        }
        for reference in &self.references {
            let id = id(&reference.id)?;
            let current = settled(&view, &id)?;
            if reference.kind != kind(current.kind) {
                return Err(invalid(format!("read-only: {id}: kind is fixed")));
            }
        }
        let conflicts: Vec<_> = self
            .typed_records()
            .filter_map(|(kind, r)| self.applied(&view, kind, r).err().map(|e| e.0))
            .collect();
        if !conflicts.is_empty() {
            return Err(invalid(conflicts.join("; ")));
        }
        // Every Entity holds its final value: nothing to publish, and nothing left to check.
        if self.fully_applied(&view) {
            return Ok(Checked {
                after: store.clone(),
                after_view: view,
                records: Vec::new(),
                already_applied: true,
            });
        }
        for (_, r) in self.typed_records().filter(|(_, r)| r.base.is_some()) {
            let id = record_id(r)?;
            if lifecycle(view.current(&id).unwrap().lifecycle) != r.lifecycle {
                return Err(invalid(format!("read-only: {id}: lifecycle is fixed")));
            }
        }
        let external = self.external_ids()?;
        for id in &external {
            settled(&view, id)?;
        }
        if external
            .iter()
            .map(ToString::to_string)
            .collect::<BTreeSet<_>>()
            != self.references.iter().map(|r| r.id.clone()).collect()
        {
            return Err(invalid(
                "identity/reference: references set differs; run axon import prepare FILE",
            ));
        }
        let candidate = match self.require_valid_store(store, &view, &context)? {
            Some(candidate) => candidate,
            None => self.candidate(store, &view, context)?,
        };
        Ok(Checked {
            after: candidate.store,
            after_view: candidate.after,
            records: candidate.records,
            already_applied: false,
        })
    }
    /// Validates the declaration as the sequence of ordinary operations on a private copy and
    /// reduces the outcome to one record per changed Entity: a created record with the final
    /// value for a new Entity, an import record for an existing one (the record `Store::import`
    /// would make; it is built here because the sequence interleaves Entities, which a single
    /// Entity's operation cannot). The records are in publication order (new Entities with
    /// parents and dependencies first, then existing ones), and the store they are added to is
    /// derived again to confirm they add no violation the operations did not.
    fn candidate(&self, store: &Store, view: &View, context: Context) -> Result<Candidate> {
        let mut scratch = store.clone();
        self.apply_operations(&mut scratch, context.clone())?;
        let after = scratch.view().map_err(|e| invalid(e.to_string()))?;
        let mut created = Vec::new();
        let mut imported = Vec::new();
        for (kind, r) in self.typed_records() {
            let id = record_id(r)?;
            let final_value = after
                .current(&id)
                .ok_or_else(|| invalid(format!("core rejection: {id}: unsettled")))?
                .clone();
            if view.is_known(&id) {
                let before = view.current(&id).expect("settled before");
                if *before == final_value {
                    continue;
                }
                let head = view.head(&id).expect("settled").clone();
                imported.push(StoreRecord {
                    entity: id,
                    kind: RecordKind::Import,
                    parents: [head].into(),
                    at: context.at,
                    recorder: context.recorder.clone(),
                    reason: None,
                    after: final_value,
                });
            } else {
                debug_assert_eq!(final_value.kind, kind);
                created.push(StoreRecord {
                    entity: id,
                    kind: RecordKind::Created,
                    parents: Default::default(),
                    at: context.at,
                    recorder: context.recorder.clone(),
                    reason: None,
                    after: final_value,
                });
            }
        }
        // New Entities are published after their new parents and dependencies.
        let new_ids: BTreeSet<_> = created.iter().map(|r| r.entity.clone()).collect();
        let mut ordered: Vec<StoreRecord> = Vec::new();
        let mut placed: BTreeSet<EntityId> = BTreeSet::new();
        let mut pending = created;
        while !pending.is_empty() {
            let Some(index) = pending.iter().position(|r| {
                r.after
                    .parent
                    .iter()
                    .chain(&r.after.needs)
                    .all(|other| !new_ids.contains(other) || placed.contains(other))
            }) else {
                return Err(invalid(format!(
                    "core rejection: {}: parent/needs: cycle among new Entities",
                    pending[0].entity
                )));
            };
            let record = pending.remove(index);
            placed.insert(record.entity.clone());
            ordered.push(record);
        }
        ordered.extend(imported);
        let mut published = store.clone();
        for record in &ordered {
            published
                .insert(Entry::Record(record.clone()))
                .map_err(|e| invalid(format!("core rejection: {e}")))?;
        }
        let final_view = published.view().map_err(|e| invalid(e.to_string()))?;
        let added: Vec<_> = final_view
            .violations()
            .difference(view.violations())
            .map(|v| format!("{} ({})", v.entity, v.kind.label()))
            .collect();
        if !added.is_empty() {
            return Err(invalid(format!(
                "core rejection: the declaration would add a structural violation: {}",
                added.join(", ")
            )));
        }
        Ok(Candidate {
            store: published,
            after: final_view,
            records: ordered,
        })
    }
    /// Use ordinary core operations on a private candidate; callers publish only after success.
    pub fn apply_operations(&self, candidate: &mut Store, context: Context) -> Result<()> {
        // The scratch store with its view, derived once per record added rather than per
        // lookup.
        struct Scratch<'s> {
            store: &'s mut Store,
            view: View,
        }
        impl Scratch<'_> {
            fn apply(
                &mut self,
                record: crate::lifecycle::Result<Option<StoreRecord>>,
            ) -> crate::lifecycle::Result<()> {
                if let Some(record) = record? {
                    self.store.insert(Entry::Record(record))?;
                    self.view = self.store.view()?;
                }
                Ok(())
            }
            fn current(&self, id: &EntityId) -> Result<&Current> {
                self.view
                    .current(id)
                    .ok_or_else(|| invalid(format!("core rejection: {id}: unsettled")))
            }
        }
        let view = candidate.view().map_err(|e| invalid(e.to_string()))?;
        // A new Entity that a retry finds already registered is treated as an existing one.
        let records = self
            .typed_records()
            .map(|(kind, r)| {
                let id = record_id(r)?;
                let existing = r.base.is_some() || view.is_known(&id);
                Ok((kind, id, existing, self.desired(kind, r)?))
            })
            .collect::<Result<Vec<_>>>()?;
        let mut scratch = Scratch {
            store: candidate,
            view,
        };
        let core = |id: &EntityId, field: &str, result: crate::lifecycle::Result<()>| {
            result.map_err(|e| invalid(format!("core rejection: {id}: {field}: {e}")))
        };
        for (_, id, existing, desired) in &records {
            if *existing {
                let old = scratch.current(id)?.clone();
                if old.parent != desired.parent {
                    let record = scratch.store.set_parent(id, None, context.clone());
                    core(id, "parent", scratch.apply(record))?;
                }
                for target in old.needs.difference(&desired.needs) {
                    let record = scratch.store.remove_dependency(id, target, context.clone());
                    core(id, "needs", scratch.apply(record))?;
                }
            }
        }
        let mut pending: Vec<_> = records
            .iter()
            .filter(|(_, _, existing, _)| !existing)
            .collect();
        while !pending.is_empty() {
            let Some(index) = pending.iter().position(|(_, _, _, d)| {
                d.parent.as_ref().is_none_or(|p| scratch.view.is_known(p))
            }) else {
                return Err(invalid(format!(
                    "core rejection: {}: parent: containment cycle",
                    pending[0].1
                )));
            };
            let (kind, id, _, desired) = pending.remove(index);
            let initial = Current {
                kind: *kind,
                lifecycle: desired.lifecycle,
                owner: None,
                title: desired.title.clone(),
                description: desired.description.clone(),
                // A declaration does not carry a label yet; a new Entity starts as chore.
                label: Label::Chore,
                condition: None,
                parent: desired.parent.clone(),
                needs: BTreeSet::new(),
            };
            let record = scratch
                .store
                .create(id.clone(), initial, context.clone())
                .map(Some);
            core(id, "parent/lifecycle", scratch.apply(record))?;
        }
        for (_, id, existing, desired) in &records {
            if *existing {
                let old = scratch.current(id)?;
                let title = (old.title != desired.title).then(|| desired.title.clone());
                let description =
                    (old.description != desired.description).then(|| desired.description.clone());
                if title.is_some() || description.is_some() {
                    let record = scratch.store.write(id, title, description, context.clone());
                    core(id, "title/description", scratch.apply(record))?;
                }
            }
        }
        for (_, id, _, desired) in &records {
            if scratch.current(id)?.parent != desired.parent {
                let record = scratch
                    .store
                    .set_parent(id, desired.parent.clone(), context.clone());
                core(id, "parent", scratch.apply(record))?;
            }
        }
        for (_, id, _, desired) in &records {
            for target in &desired.needs {
                if !scratch.current(id)?.needs.contains(target) {
                    let record = scratch.store.add_dependency(id, target, context.clone());
                    core(id, "needs", scratch.apply(record))?;
                }
            }
        }
        Ok(())
    }
}
struct Candidate {
    store: Store,
    after: View,
    records: Vec<StoreRecord>,
}
/// A store with a conflicted Entity takes no declaration until it is resolved.
fn require_settled(view: &View) -> Result<()> {
    match view.conflicted().first() {
        Some(id) => Err(invalid(format!(
            "identity/reference: {id}: conflicted; resolve it first"
        ))),
        None => Ok(()),
    }
}
/// The outcome of `check`: the store as it will be after the records are published, and the
/// records themselves in publication order. Nothing here changes the store.
#[derive(Debug)]
pub struct Checked {
    pub after: Store,
    /// The view of `after`, derived once.
    pub after_view: View,
    pub records: Vec<StoreRecord>,
    pub already_applied: bool,
}
