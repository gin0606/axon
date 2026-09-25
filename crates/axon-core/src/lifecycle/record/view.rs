use super::model::{Current, Entry, Record, RecordId, RecordKind};
use super::store::Store;
use super::{EntityId, Kind, Lifecycle, Operation, Result};
use crate::lifecycle::invalid;
use std::collections::{BTreeMap, BTreeSet};

/// A settled Entity: exactly one head, whose value is the Entity's current value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settled {
    pub head: RecordId,
    pub current: Current,
}

/// The kinds of structural violation, each attributed to one Entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ViolationKind {
    /// The Entity is inside a containment cycle.
    ContainmentCycle,
    /// The parent does not exist or is not a Group.
    UnknownParent,
    /// An unfinished Entity under a terminal parent.
    OpenUnderTerminal,
    /// An InProgress Issue or an effectively InProgress Group with an unadopted ancestor.
    UnadoptedAncestor,
    /// A Completed Entity whose dependency is not Completed.
    CompletedWithOpenDependency,
    /// A dependency that does not exist.
    UnknownDependency,
    /// The Entity is inside a cycle of the ordinary completion path.
    CompletionCycle,
}
impl ViolationKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::ContainmentCycle => "containment cycle",
            Self::UnknownParent => "unknown parent",
            Self::OpenUnderTerminal => "unfinished under a terminal parent",
            Self::UnadoptedAncestor => "unadopted ancestor",
            Self::CompletedWithOpenDependency => "Completed with an unfinished dependency",
            Self::UnknownDependency => "unknown dependency",
            Self::CompletionCycle => "completion cycle",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Violation {
    pub entity: EntityId,
    pub kind: ViolationKind,
}

/// Everything a read derives from the record set. Only settled Entities have current values;
/// a conflicted Entity is neither an adopted ancestor nor a Completed dependency for anyone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    heads: BTreeMap<EntityId, BTreeSet<RecordId>>,
    settled: BTreeMap<EntityId, Settled>,
    conflicted: BTreeSet<EntityId>,
    gaps: BTreeMap<EntityId, BTreeSet<RecordId>>,
    noted_only: BTreeSet<EntityId>,
    children: BTreeMap<EntityId, Vec<EntityId>>,
    working: BTreeSet<EntityId>,
    violations: BTreeSet<Violation>,
}

impl View {
    pub(super) fn derive(store: &Store) -> Result<Self> {
        Self::derive_with(store, None)
    }
    /// Derives the view of the store plus one record that is not in it yet, so an operation
    /// can check its result without copying the store.
    pub(super) fn derive_with(store: &Store, extra: Option<(&RecordId, &Entry)>) -> Result<Self> {
        let lookup = |id: &RecordId| -> Option<&Entry> {
            match extra {
                Some((extra_id, entry)) if extra_id == id => Some(entry),
                _ => store.get(id),
            }
        };
        let mut by_entity: BTreeMap<&EntityId, Vec<&RecordId>> = BTreeMap::new();
        let mut referenced = BTreeSet::new();
        let mut noted = BTreeSet::new();
        let mut gaps: BTreeMap<EntityId, BTreeSet<RecordId>> = BTreeMap::new();
        let mut in_degree: BTreeMap<&RecordId, usize> = BTreeMap::new();
        let mut children_of: BTreeMap<&RecordId, Vec<&RecordId>> = BTreeMap::new();
        for (id, entry) in store.entries().chain(extra) {
            let record = match entry {
                Entry::Note(note) => {
                    noted.insert(&note.entity);
                    continue;
                }
                Entry::Record(record) => record,
            };
            by_entity.entry(&record.entity).or_default().push(id);
            let mut present = 0;
            for parent_id in &record.parents {
                referenced.insert(parent_id);
                let Some(parent) = lookup(parent_id) else {
                    gaps.entry(record.entity.clone())
                        .or_default()
                        .insert(parent_id.clone());
                    continue;
                };
                let Some(parent) = parent.as_record() else {
                    return Err(invalid(format!(
                        "record {id} of {} has a Note as its parent",
                        record.entity
                    )));
                };
                if parent.entity != record.entity {
                    return Err(invalid(format!(
                        "record {id} of {} has a parent of {}",
                        record.entity, parent.entity
                    )));
                }
                present += 1;
                children_of.entry(parent_id).or_default().push(id);
                continues(parent_id, parent, record).map_err(|error| {
                    invalid(format!("record {id} of {}: {error}", record.entity))
                })?;
            }
            in_degree.insert(id, present);
        }
        // Content-addressed IDs cannot form a cycle; a set that does is corrupt.
        let mut ready: Vec<_> = in_degree
            .iter()
            .filter(|(_, count)| **count == 0)
            .map(|(id, _)| *id)
            .collect();
        let mut removed = 0;
        while let Some(id) = ready.pop() {
            removed += 1;
            for child in children_of.get(id).into_iter().flatten() {
                let count = in_degree.get_mut(child).expect("indexed child");
                *count -= 1;
                if *count == 0 {
                    ready.push(child);
                }
            }
        }
        if removed != in_degree.len() {
            return Err(invalid("causal cycle among records"));
        }
        let mut heads = BTreeMap::new();
        let mut settled = BTreeMap::new();
        let mut conflicted = BTreeSet::new();
        for (entity, ids) in by_entity {
            let entity_heads: BTreeSet<RecordId> = ids
                .into_iter()
                .filter(|id| !referenced.contains(id))
                .cloned()
                .collect();
            if entity_heads.len() == 1 {
                let head = entity_heads.first().expect("one head").clone();
                let current = lookup(&head)
                    .and_then(Entry::as_record)
                    .expect("indexed record")
                    .after
                    .clone();
                settled.insert(entity.clone(), Settled { head, current });
            } else {
                conflicted.insert(entity.clone());
            }
            heads.insert(entity.clone(), entity_heads);
        }
        let noted_only = noted
            .into_iter()
            .filter(|entity| !heads.contains_key(*entity))
            .cloned()
            .collect();
        let mut children: BTreeMap<EntityId, Vec<EntityId>> = BTreeMap::new();
        for (id, entity) in &settled {
            if let Some(parent) = &entity.current.parent {
                children.entry(parent.clone()).or_default().push(id.clone());
            }
        }
        let mut view = Self {
            heads,
            settled,
            conflicted,
            gaps,
            noted_only,
            children,
            working: BTreeSet::new(),
            violations: BTreeSet::new(),
        };
        view.working = view.derive_working();
        view.violations = view.derive_violations();
        Ok(view)
    }

    fn derive_working(&self) -> BTreeSet<EntityId> {
        let mut working = BTreeSet::new();
        loop {
            let next: BTreeSet<EntityId> = self
                .settled
                .iter()
                .filter(|(id, entity)| {
                    entity.current.kind == Kind::Group
                        && entity.current.lifecycle == Lifecycle::NotStarted
                        && self.children(id).iter().any(|child| {
                            matches!(
                                self.settled[child].current.lifecycle,
                                Lifecycle::InProgress | Lifecycle::Completed
                            ) || working.contains(child)
                        })
                })
                .map(|(id, _)| id.clone())
                .collect();
            if next == working {
                return working;
            }
            working = next;
        }
    }

    fn derive_violations(&self) -> BTreeSet<Violation> {
        let mut violations = BTreeSet::new();
        let mut add = |entity: &EntityId, kind: ViolationKind| {
            violations.insert(Violation {
                entity: entity.clone(),
                kind,
            });
        };
        for (id, entity) in &self.settled {
            let current = &entity.current;
            let ancestors = self.ancestors(id);
            if ancestors.contains(id) {
                add(id, ViolationKind::ContainmentCycle);
            }
            if let Some(parent) = &current.parent {
                let unknown = !self.is_known(parent)
                    || self.current(parent).is_some_and(|p| p.kind != Kind::Group);
                if unknown {
                    add(id, ViolationKind::UnknownParent);
                }
                if self.current(parent).is_some_and(Current::is_terminal) && !current.is_terminal()
                {
                    add(id, ViolationKind::OpenUnderTerminal);
                }
            }
            if (current.lifecycle == Lifecycle::InProgress || self.working.contains(id))
                && ancestors
                    .iter()
                    .any(|a| self.settled[a].current.lifecycle != Lifecycle::NotStarted)
            {
                add(id, ViolationKind::UnadoptedAncestor);
            }
            if current.lifecycle == Lifecycle::Completed
                && current.needs.iter().any(|d| {
                    self.current(d)
                        .is_some_and(|dep| dep.lifecycle != Lifecycle::Completed)
                })
            {
                add(id, ViolationKind::CompletedWithOpenDependency);
            }
            if current.needs.iter().any(|d| !self.is_known(d)) {
                add(id, ViolationKind::UnknownDependency);
            }
        }
        for id in self.completion_cycle_members() {
            add(&id, ViolationKind::CompletionCycle);
        }
        violations
    }

    /// Entities that keep a predecessor on the contracted completion path (direct children,
    /// own dependencies, ancestors' dependencies) after every free Entity is removed: the
    /// members of a cycle and everything waiting on one.
    fn completion_cycle_members(&self) -> BTreeSet<EntityId> {
        let mut pending: BTreeMap<&EntityId, usize> = BTreeMap::new();
        let mut dependents: BTreeMap<&EntityId, Vec<&EntityId>> = BTreeMap::new();
        let mut ready = Vec::new();
        for id in self.settled.keys() {
            let mut needs: BTreeSet<&EntityId> = self.children(id).iter().collect();
            needs.extend(self.settled_needs(id));
            for ancestor in self.ancestors(id) {
                needs.extend(self.settled_needs(&ancestor));
            }
            if needs.is_empty() {
                ready.push(id);
            }
            pending.insert(id, needs.len());
            for need in needs {
                dependents.entry(need).or_default().push(id);
            }
        }
        while let Some(id) = ready.pop() {
            pending.remove(id);
            for dependent in dependents.get(id).into_iter().flatten() {
                if let Some(count) = pending.get_mut(dependent) {
                    *count -= 1;
                    if *count == 0 {
                        ready.push(dependent);
                    }
                }
            }
        }
        pending.into_keys().cloned().collect()
    }

    // ---- what a read shows ----

    /// Entities with at least one record other than a Note.
    pub fn known(&self) -> impl Iterator<Item = &EntityId> {
        self.heads.keys()
    }
    pub fn is_known(&self, id: &EntityId) -> bool {
        self.heads.contains_key(id)
    }
    /// The heads of a known Entity: records that are nobody's parent.
    pub fn heads(&self, id: &EntityId) -> Option<&BTreeSet<RecordId>> {
        self.heads.get(id)
    }
    pub fn settled(&self) -> impl Iterator<Item = (&EntityId, &Settled)> {
        self.settled.iter()
    }
    pub fn settled_entity(&self, id: &EntityId) -> Option<&Settled> {
        self.settled.get(id)
    }
    pub fn is_settled(&self, id: &EntityId) -> bool {
        self.settled.contains_key(id)
    }
    /// The current value of a settled Entity.
    pub fn current(&self, id: &EntityId) -> Option<&Current> {
        self.settled.get(id).map(|entity| &entity.current)
    }
    pub fn head(&self, id: &EntityId) -> Option<&RecordId> {
        self.settled.get(id).map(|entity| &entity.head)
    }
    pub fn conflicted(&self) -> &BTreeSet<EntityId> {
        &self.conflicted
    }
    pub fn is_conflicted(&self, id: &EntityId) -> bool {
        self.conflicted.contains(id)
    }
    /// Entities with a record whose parent is missing, with the missing IDs.
    pub fn gaps(&self) -> &BTreeMap<EntityId, BTreeSet<RecordId>> {
        &self.gaps
    }
    /// Entities that have Notes but no other record.
    pub fn noted_only(&self) -> &BTreeSet<EntityId> {
        &self.noted_only
    }
    pub fn violations(&self) -> &BTreeSet<Violation> {
        &self.violations
    }
    pub fn is_valid(&self) -> bool {
        self.conflicted.is_empty() && self.violations.is_empty()
    }
    /// Whether the Entity is attributed a violation, which waives the repair-blocking rules
    /// for it.
    pub fn in_violation(&self, id: &EntityId) -> bool {
        self.violations.iter().any(|v| &v.entity == id)
    }
    /// Groups whose effective lifecycle is InProgress.
    pub fn working(&self) -> &BTreeSet<EntityId> {
        &self.working
    }
    /// The stored lifecycle of an Issue; InProgress for a NotStarted Group with work below it.
    pub fn effective_lifecycle(&self, id: &EntityId) -> Option<Lifecycle> {
        if self.working.contains(id) {
            return Some(Lifecycle::InProgress);
        }
        self.current(id).map(|current| current.lifecycle)
    }
    /// Settled direct children.
    pub fn children(&self, id: &EntityId) -> &[EntityId] {
        self.children.get(id).map_or(&[], Vec::as_slice)
    }
    /// Settled descendants, parents before children; a containment cycle ends the walk.
    pub fn descendants(&self, id: &EntityId) -> Vec<EntityId> {
        let mut found = Vec::new();
        let mut seen = BTreeSet::from([id.clone()]);
        let mut pending = vec![id.clone()];
        while let Some(parent) = pending.pop() {
            for child in self.children(&parent) {
                if seen.insert(child.clone()) {
                    found.push(child.clone());
                    pending.push(child.clone());
                }
            }
        }
        found
    }
    /// Settled ancestors from the parent upwards. A cycle shows as the Entity itself among
    /// its ancestors; an unsettled parent ends the walk.
    pub fn ancestors(&self, id: &EntityId) -> Vec<EntityId> {
        let mut found = Vec::new();
        let mut seen = BTreeSet::new();
        let mut next = self.current(id).and_then(|c| c.parent.clone());
        while let Some(ancestor) = next {
            if !self.is_settled(&ancestor) || !seen.insert(ancestor.clone()) {
                break;
            }
            next = self.current(&ancestor).and_then(|c| c.parent.clone());
            found.push(ancestor);
        }
        found
    }
    /// Whether every link of the parent chain is settled and adopted (NotStarted). An unknown
    /// or conflicted ancestor anywhere in the chain fails, since it cannot be known to be
    /// adopted.
    pub fn ancestors_adopted(&self, id: &EntityId) -> bool {
        let mut seen = BTreeSet::new();
        let mut next = self.current(id).and_then(|c| c.parent.clone());
        while let Some(ancestor) = next {
            let Some(current) = self.current(&ancestor) else {
                return false;
            };
            if current.lifecycle != Lifecycle::NotStarted {
                return false;
            }
            if !seen.insert(ancestor.clone()) {
                break;
            }
            next = current.parent.clone();
        }
        true
    }
    fn settled_needs(&self, id: &EntityId) -> Vec<&EntityId> {
        self.settled[id]
            .current
            .needs
            .iter()
            .filter(|d| self.is_settled(d))
            .collect()
    }
    /// Whether every dependency is settled and Completed.
    pub fn dependencies_completed(&self, id: &EntityId) -> bool {
        self.current(id).is_some_and(|current| {
            current.needs.iter().all(|d| {
                self.current(d)
                    .is_some_and(|dep| dep.lifecycle == Lifecycle::Completed)
            })
        })
    }
    /// Settled Completed Entities that depend on this one.
    pub fn completed_dependents(&self, id: &EntityId) -> Vec<&EntityId> {
        self.settled
            .iter()
            .filter(|(_, e)| {
                e.current.lifecycle == Lifecycle::Completed && e.current.needs.contains(id)
            })
            .map(|(dependent, _)| dependent)
            .collect()
    }
    /// Whether the parent, if any, is settled and unfinished.
    pub fn parent_open(&self, id: &EntityId) -> bool {
        self.current(id)
            .and_then(|c| c.parent.as_ref())
            .is_none_or(|parent| self.current(parent).is_some_and(|p| !p.is_terminal()))
    }
    fn children_ended(&self, id: &EntityId) -> bool {
        self.children(id)
            .iter()
            .all(|child| self.settled[child].current.is_terminal())
    }
    fn has_started_descendant(&self, id: &EntityId) -> bool {
        self.descendants(id).iter().any(|d| {
            matches!(
                self.settled[d].current.lifecycle,
                Lifecycle::InProgress | Lifecycle::Completed
            )
        })
    }

    pub(super) fn require_settled(&self, id: &EntityId) -> Result<&Current> {
        if self.conflicted.contains(id) {
            return Err(invalid(format!(
                "Entity {id} is conflicted; resolve it first"
            )));
        }
        self.current(id)
            .ok_or_else(|| invalid(format!("missing Entity {id}")))
    }

    /// Checks the prerequisites of one lifecycle operation on a settled Entity without
    /// evaluating conditions. Checking `Complete` neither records nor implies a final review;
    /// performing it does. Conflicts elsewhere in the store are checked by the writer, not
    /// here, so a read can still describe each row.
    pub fn check_operation(&self, id: &EntityId, operation: Operation) -> Result<()> {
        let current = self.require_settled(id)?;
        operation.apply_as(current.kind, current.lifecycle)?;
        if !self.parent_open(id) && !self.in_violation(id) {
            return Err(invalid("parent must be an unfinished Group"));
        }
        let adopted_ancestors = "all ancestor Groups must be adopted (NotStarted)";
        let deps_completed = "dependencies must be Completed";
        match operation {
            Operation::Start => {
                if !self.ancestors_adopted(id) {
                    return Err(invalid(adopted_ancestors));
                }
                if !self.dependencies_completed(id) {
                    return Err(invalid(deps_completed));
                }
                if let Some(ancestor) = self
                    .ancestors(id)
                    .into_iter()
                    .find(|a| !self.dependencies_completed(a))
                {
                    return Err(invalid(format!(
                        "dependencies of ancestor {ancestor} must be Completed"
                    )));
                }
            }
            Operation::Complete => {
                if !self.dependencies_completed(id) {
                    return Err(invalid(deps_completed));
                }
                if current.kind == Kind::Group && !self.children_ended(id) {
                    return Err(invalid("all children must be terminal"));
                }
                if !self.ancestors_adopted(id) {
                    return Err(invalid(adopted_ancestors));
                }
            }
            Operation::Cancel => {
                if current.kind == Kind::Group && !self.children_ended(id) {
                    return Err(invalid("all children must be terminal"));
                }
            }
            Operation::Withdraw => {
                if self.working.contains(id) {
                    return Err(invalid(
                        "a Group that is InProgress through its children cannot be withdrawn",
                    ));
                }
            }
            Operation::Accept => {
                if current.kind == Kind::Group
                    && self.has_started_descendant(id)
                    && !self.ancestors_adopted(id)
                {
                    return Err(invalid(
                        "a Group with InProgress or Completed work below it is accepted only under adopted ancestors",
                    ));
                }
            }
            Operation::Reopen => {
                if !self.ancestors_adopted(id) {
                    return Err(invalid(adopted_ancestors));
                }
                let dependents = self.completed_dependents(id);
                if !dependents.is_empty() && !self.in_violation(id) {
                    return Err(invalid(format!(
                        "Completed dependents must be reopened first: {}",
                        dependents
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
                    )));
                }
            }
            Operation::Release | Operation::Reconsider => {}
        }
        Ok(())
    }
}

/// Whether a record with a present parent changes only what its kind may change. A
/// transition changes lifecycle and owner by the rules of the kind at its time; an edit the
/// text; a parent, dependency or condition record its one field; a conversion the kind; an
/// import text, parent and needs; a resolve record repeats the chosen head.
fn continues(parent_id: &RecordId, parent: &Record, record: &Record) -> Result<()> {
    let before = &parent.after;
    let after = &record.after;
    let (mut kind, mut lifecycle, mut owner, mut text, mut parent_field, mut needs, mut condition) =
        (false, false, false, false, false, false, false);
    match &record.kind {
        RecordKind::Created => unreachable!("a created record has no parent"),
        RecordKind::Transition(operation) => {
            let expected = operation.apply_as(before.kind, before.lifecycle)?;
            if expected != after.lifecycle {
                return Err(invalid(format!(
                    "{operation:?} does not continue its parent"
                )));
            }
            lifecycle = true;
            owner = true;
        }
        RecordKind::Edit => text = true,
        RecordKind::Parent => parent_field = true,
        RecordKind::Dependency => needs = true,
        RecordKind::Condition => condition = true,
        RecordKind::Convert => {
            if before.kind == after.kind {
                return Err(invalid("a conversion keeps the kind"));
            }
            if !matches!(
                before.lifecycle,
                Lifecycle::Undecided | Lifecycle::NotStarted
            ) {
                return Err(invalid("a conversion from a started or terminal Entity"));
            }
            kind = true;
        }
        RecordKind::Import => {
            text = true;
            parent_field = true;
            needs = true;
        }
        RecordKind::Resolve { chosen } => {
            if parent_id == chosen && before != after {
                return Err(invalid("the value differs from the chosen head"));
            }
            return Ok(());
        }
    }
    let unchanged = [
        (kind || before.kind == after.kind, "kind"),
        (
            lifecycle || before.lifecycle == after.lifecycle,
            "lifecycle",
        ),
        (owner || before.owner == after.owner, "owner"),
        (
            text || (before.title == after.title && before.description == after.description),
            "text",
        ),
        (parent_field || before.parent == after.parent, "parent"),
        (needs || before.needs == after.needs, "needs"),
        (
            condition || before.condition == after.condition,
            "condition",
        ),
    ];
    if let Some((_, field)) = unchanged.iter().find(|(ok, _)| !ok) {
        return Err(invalid(format!(
            "a {} record changes the {field}",
            record.kind.name()
        )));
    }
    Ok(())
}
