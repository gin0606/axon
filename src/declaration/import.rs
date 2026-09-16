use super::*;
use crate::lifecycle::{Context, Current};

fn id(value: &str) -> Result<EntityId> {
    value
        .to_owned()
        .try_into()
        .map_err(|e| invalid(format!("identity/reference: {value}: {e}")))
}
fn record_id(record: &Record) -> Result<EntityId> {
    id(record
        .id
        .as_deref()
        .ok_or_else(|| invalid("identity/reference: id: run axon import prepare FILE"))?)
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
    fn desired(&self, r: &Record) -> Result<Current> {
        Ok(Current {
            title: r.title.clone(),
            description: r.description.clone(),
            lifecycle: match r.lifecycle.as_str() {
                "undecided" => Lifecycle::Undecided,
                "not-started" => Lifecycle::NotStarted,
                "in-progress" => Lifecycle::InProgress,
                "completed" => Lifecycle::Completed,
                "cancelled" => Lifecycle::Cancelled,
                _ => return Err(invalid("schema: lifecycle")),
            },
            condition: None,
            parent: r.parent.as_ref().map(|p| self.resolved(p)).transpose()?,
            dependencies: r
                .needs
                .iter()
                .map(|p| self.resolved(p))
                .collect::<Result<_>>()?,
        })
    }
    fn existing_identities(&self, snapshot: &Snapshot) -> Result<()> {
        for (kind, r) in self.typed_records().filter(|(_, r)| r.base.is_some()) {
            let id = record_id(r)?;
            let e = snapshot.entity(&id).map_err(|_| {
                invalid(format!(
                    "identity/reference: {id}: ID does not exist in storage"
                ))
            })?;
            if e.kind != kind {
                return Err(invalid(format!("read-only: {id}: kind is fixed")));
            }
        }
        Ok(())
    }
    pub fn already_applied(&self, snapshot: &Snapshot) -> Result<bool> {
        if self.records().any(|r| r.id.is_none()) {
            return Ok(false);
        }
        for (kind, r) in self.typed_records() {
            let Some(value) = &r.id else { return Ok(false) };
            let Ok(e) = snapshot.entity(&id(value)?) else {
                return Ok(false);
            };
            let mut desired = self.desired(r)?;
            desired.condition = e.current.condition.clone();
            if e.kind != kind || e.current != desired {
                return Ok(false);
            }
        }
        Ok(true)
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
    pub fn refresh_references(&mut self, snapshot: &Snapshot) -> Result<()> {
        self.references = self
            .external_ids()?
            .into_iter()
            .map(|id| {
                let e = snapshot.entity(&id).map_err(|_| {
                    invalid(format!(
                        "identity/reference: {id}: ID does not exist in storage"
                    ))
                })?;
                Ok(External {
                    id: id.to_string(),
                    kind: kind(e.kind).into(),
                    lifecycle: lifecycle(e.current.lifecycle).into(),
                    title: e.current.title.clone(),
                })
            })
            .collect::<Result<_>>()?;
        Ok(())
    }
    pub fn refresh_applied(&mut self, snapshot: &Snapshot) -> Result<()> {
        for r in self.groups.iter_mut().chain(&mut self.issues) {
            let e = snapshot
                .entity(&record_id(r)?)
                .map_err(|e| invalid(e.to_string()))?;
            r.base = Some(fingerprint(e));
            r.lifecycle = lifecycle(e.current.lifecycle).into();
        }
        self.refresh_references(snapshot)
    }
    pub fn prepare(&mut self, snapshot: &Snapshot, prefix: &str) -> Result<()> {
        self.validate()?;
        self.existing_identities(snapshot)?;
        let applied = self.already_applied(snapshot)?;
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
        let mut used: BTreeSet<String> = snapshot
            .entities()
            .map(|e| e.id.to_string())
            .chain(self.records().filter_map(|r| r.id.clone()))
            .collect();
        for r in self.groups.iter_mut().chain(&mut self.issues) {
            if r.base.is_none()
                && (r.id.is_none()
                    || (!applied
                        && r.id
                            .as_ref()
                            .is_some_and(|v| id(v).is_ok_and(|id| snapshot.entity(&id).is_ok()))))
            {
                loop {
                    let candidate = EntityId::generate(prefix).to_string();
                    if used.insert(candidate.clone()) {
                        r.id = Some(candidate);
                        break;
                    }
                }
            }
        }
        self.refresh_references(snapshot)?;
        self.validate()?;
        Ok(())
    }
    pub fn check(&self, input: &str, snapshot: &Snapshot, context: Context) -> Result<Checked> {
        self.validate()?;
        self.existing_identities(snapshot)?;
        for r in self.records() {
            record_id(r)?;
        }
        if self.serialize(snapshot)? != input {
            return Err(invalid(
                "schema: non-canonical declaration; run axon import prepare FILE",
            ));
        }
        for reference in &self.references {
            let id = id(&reference.id)?;
            let entity = snapshot.entity(&id).map_err(|_| {
                invalid(format!(
                    "identity/reference: {id}: ID does not exist in storage"
                ))
            })?;
            if reference.kind != kind(entity.kind) {
                return Err(invalid(format!("read-only: {id}: kind is fixed")));
            }
        }
        let conflicts: Vec<_> = self
            .records()
            .filter_map(|r| {
                let id = record_id(r).ok()?;
                match (&r.base, snapshot.entity(&id)) {
                    (Some(base), Ok(e)) if *base != fingerprint(e) => {
                        Some(format!("{id}: base mismatch"))
                    }
                    (None, Ok(_)) => Some(format!("{id}: new id already exists")),
                    _ => None,
                }
            })
            .collect();
        if !conflicts.is_empty() {
            if self.already_applied(snapshot)? {
                return Ok(Checked {
                    snapshot: snapshot.clone(),
                    already_applied: true,
                });
            }
            return Err(invalid(format!("conflict: {}", conflicts.join("; "))));
        }
        for r in self.records().filter(|r| r.base.is_some()) {
            let id = record_id(r)?;
            if lifecycle(snapshot.entity(&id).unwrap().current.lifecycle) != r.lifecycle {
                return Err(invalid(format!("read-only: {id}: lifecycle is fixed")));
            }
        }
        let external = self.external_ids()?;
        for id in &external {
            snapshot.entity(id).map_err(|_| {
                invalid(format!(
                    "identity/reference: {id}: ID does not exist in storage"
                ))
            })?;
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
        let mut candidate = snapshot.clone();
        self.apply_operations(&mut candidate, context)?;
        Ok(Checked {
            snapshot: candidate,
            already_applied: false,
        })
    }
    /// Use ordinary core operations on a private candidate; callers publish only after success.
    pub fn apply_operations(&self, candidate: &mut Snapshot, context: Context) -> Result<()> {
        let records = self
            .typed_records()
            .map(|(kind, r)| Ok((kind, record_id(r)?, r, self.desired(r)?)))
            .collect::<Result<Vec<_>>>()?;
        let core = |id: &EntityId, field: &str, result: crate::lifecycle::Result<()>| {
            result.map_err(|e| invalid(format!("core rejection: {id}: {field}: {e}")))
        };
        for (_, id, r, desired) in &records {
            if r.base.is_some() {
                let old = candidate.entity(id).unwrap().current.clone();
                if old.parent != desired.parent {
                    core(id, "parent", candidate.set_parent(id, None))?;
                }
                for target in old.dependencies.difference(&desired.dependencies) {
                    core(id, "needs", candidate.remove_dependency(id, target))?;
                }
            }
        }
        let mut pending: Vec<_> = records
            .iter()
            .filter(|(_, _, r, _)| r.base.is_none())
            .collect();
        while !pending.is_empty() {
            let Some(index) = pending.iter().position(|(_, _, _, d)| {
                d.parent
                    .as_ref()
                    .is_none_or(|p| candidate.entity(p).is_ok())
            }) else {
                return Err(invalid(format!(
                    "core rejection: {}: parent: containment cycle",
                    pending[0].1
                )));
            };
            let (kind, id, _, desired) = pending.remove(index);
            let mut initial = desired.clone();
            initial.dependencies.clear();
            core(
                id,
                "parent/lifecycle",
                candidate.create(id.clone(), *kind, initial, context.clone()),
            )?;
        }
        for (_, id, r, desired) in &records {
            if r.base.is_some() {
                let old = &candidate.entity(id).unwrap().current;
                let title = (old.title != desired.title).then(|| desired.title.clone());
                let description =
                    (old.description != desired.description).then(|| desired.description.clone());
                if title.is_some() || description.is_some() {
                    core(
                        id,
                        "title/description",
                        candidate.write(id, title, description),
                    )?;
                }
            }
        }
        for (_, id, _, desired) in &records {
            if candidate.entity(id).unwrap().current.parent != desired.parent {
                core(
                    id,
                    "parent",
                    candidate.set_parent(id, desired.parent.clone()),
                )?;
            }
        }
        for (_, id, _, desired) in &records {
            for target in &desired.dependencies {
                if !candidate
                    .entity(id)
                    .unwrap()
                    .current
                    .dependencies
                    .contains(target)
                {
                    core(id, "needs", candidate.add_dependency(id, target))?;
                }
            }
        }
        Ok(())
    }
}
#[derive(Debug)]
pub struct Checked {
    pub snapshot: Snapshot,
    pub already_applied: bool,
}
