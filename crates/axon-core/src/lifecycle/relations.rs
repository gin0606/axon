use super::*;
use std::collections::{BTreeMap, BTreeSet};

fn is_completed(snapshot: &Snapshot, id: &EntityId) -> bool {
    snapshot
        .entities
        .get(id)
        .is_some_and(|e| e.current.lifecycle == Lifecycle::Completed)
}

impl Snapshot {
    pub fn children(&self, id: &EntityId) -> Result<Vec<&Entity>> {
        self.entity(id)?;
        Ok(self
            .entities()
            .filter(|e| e.current.parent.as_ref() == Some(id))
            .collect())
    }

    /// Indexes every child under its parent in one pass, for walks that would otherwise
    /// rescan all Entities per visited node.
    pub(crate) fn children_by_parent(&self) -> BTreeMap<&EntityId, Vec<&Entity>> {
        let mut index: BTreeMap<&EntityId, Vec<&Entity>> = BTreeMap::new();
        for child in self.entities() {
            if let Some(parent) = &child.current.parent {
                index.entry(parent).or_default().push(child);
            }
        }
        index
    }

    /// Ancestors from the parent upwards. Fails on a missing parent or a containment cycle.
    pub fn ancestors(&self, entity: &Entity) -> Result<Vec<&Entity>> {
        let mut ancestors = Vec::new();
        let mut parent = entity.current.parent.as_ref();
        while let Some(id) = parent {
            if ancestors.len() > self.entities.len() {
                return Err(invalid("containment cycle"));
            }
            let ancestor = self.entity(id)?;
            ancestors.push(ancestor);
            parent = ancestor.current.parent.as_ref();
        }
        Ok(ancestors)
    }

    /// Whether every ancestor Group is adopted (stored NotStarted); an Entity without a parent
    /// passes.
    pub fn ancestors_adopted(&self, entity: &Entity) -> Result<bool> {
        Ok(self
            .ancestors(entity)?
            .iter()
            .all(|a| a.current.lifecycle == Lifecycle::NotStarted))
    }

    /// Every Entity below the Group, parents before children.
    pub fn descendants(&self, id: &EntityId) -> Result<Vec<&Entity>> {
        self.entity(id)?;
        let children = self.children_by_parent();
        let mut found = Vec::new();
        let mut seen = BTreeSet::from([id]);
        let mut pending = vec![id];
        while let Some(parent) = pending.pop() {
            for child in children.get(parent).into_iter().flatten() {
                if seen.insert(&child.id) {
                    found.push(*child);
                    pending.push(&child.id);
                }
            }
        }
        Ok(found)
    }

    /// Groups whose effective lifecycle is InProgress: stored NotStarted with a direct child
    /// that is InProgress or Completed, or a child Group that is itself working.
    pub fn working_groups(&self) -> BTreeSet<EntityId> {
        self.working_groups_in(&self.children_by_parent())
    }
    pub(crate) fn working_groups_in(
        &self,
        children: &BTreeMap<&EntityId, Vec<&Entity>>,
    ) -> BTreeSet<EntityId> {
        let mut working = BTreeSet::new();
        let mut settled = BTreeSet::new();
        let mut visiting = BTreeSet::new();
        for group in self.entities().filter(|e| e.kind == Kind::Group) {
            // Post-order over the subtree so each child's answer is settled before its parent's.
            // A containment cycle is reported by `validate_relations`; here it only ends the walk.
            let mut stack = vec![(group, false)];
            while let Some((node, expanded)) = stack.pop() {
                if settled.contains(&node.id) {
                    continue;
                }
                if node.kind != Kind::Group || node.current.lifecycle != Lifecycle::NotStarted {
                    settled.insert(node.id.clone());
                    continue;
                }
                if !expanded {
                    if !visiting.insert(node.id.clone()) {
                        continue;
                    }
                    stack.push((node, true));
                    for child in children.get(&node.id).into_iter().flatten() {
                        if !settled.contains(&child.id) && !visiting.contains(&child.id) {
                            stack.push((child, false));
                        }
                    }
                    continue;
                }
                if children.get(&node.id).into_iter().flatten().any(|child| {
                    matches!(
                        child.current.lifecycle,
                        Lifecycle::InProgress | Lifecycle::Completed
                    ) || working.contains(&child.id)
                }) {
                    working.insert(node.id.clone());
                }
                settled.insert(node.id.clone());
            }
        }
        working
    }

    /// The lifecycle used for display and prerequisites: the stored value for an Issue, and
    /// InProgress for a NotStarted Group with work below it.
    pub fn effective_lifecycle(&self, entity: &Entity) -> Lifecycle {
        if entity.kind == Kind::Group && self.working_groups().contains(&entity.id) {
            Lifecycle::InProgress
        } else {
            entity.current.lifecycle
        }
    }

    pub(crate) fn require_open_parent(&self, parent: Option<&EntityId>) -> Result<()> {
        if let Some(id) = parent {
            let parent = self.entity(id)?;
            if parent.kind != Kind::Group || !parent.current.lifecycle.editable() {
                return Err(invalid("parent must be an unfinished Group"));
            }
        }
        Ok(())
    }

    fn dependencies_complete(&self, entity: &Entity) -> bool {
        entity
            .current
            .dependencies
            .iter()
            .all(|id| is_completed(self, id))
    }

    /// Checks explicit operation eligibility without evaluating stored conditions.
    /// Checking `Complete` does not record or imply a final review; performing it does.
    pub fn check_operation(&self, id: &EntityId, operation: Operation) -> Result<()> {
        let children = self.children_by_parent();
        self.check_operation_in(id, operation, &children)
    }

    /// `check_operation` with a caller-built child index, so a read that checks every row
    /// does not rescan the snapshot per Group.
    pub(crate) fn check_operation_in(
        &self,
        id: &EntityId,
        operation: Operation,
        children: &BTreeMap<&EntityId, Vec<&Entity>>,
    ) -> Result<()> {
        let entity = self.entity(id)?;
        operation.apply_as(entity.kind, entity.current.lifecycle)?;
        self.require_open_parent(entity.current.parent.as_ref())?;
        let ancestors = self.ancestors(entity)?;
        let ancestors_adopted = || {
            ancestors
                .iter()
                .all(|a| a.current.lifecycle == Lifecycle::NotStarted)
        };
        let adopted_ancestors = "all ancestor Groups must be adopted (NotStarted)";
        let children_open = || {
            children
                .get(id)
                .into_iter()
                .flatten()
                .any(|e| e.current.lifecycle.editable())
        };
        match operation {
            Operation::Start => {
                if !ancestors_adopted() {
                    return Err(invalid(adopted_ancestors));
                }
                if !self.dependencies_complete(entity) {
                    return Err(invalid("dependencies must be Completed"));
                }
                if let Some(ancestor) = ancestors.iter().find(|a| !self.dependencies_complete(a)) {
                    return Err(invalid(format!(
                        "dependencies of ancestor {} must be Completed",
                        ancestor.id
                    )));
                }
            }
            Operation::Complete => {
                if !self.dependencies_complete(entity) {
                    return Err(invalid("dependencies must be Completed"));
                }
                if entity.kind == Kind::Group && children_open() {
                    return Err(invalid("all children must be terminal"));
                }
                if !ancestors_adopted() {
                    return Err(invalid(adopted_ancestors));
                }
            }
            Operation::Cancel => {
                if entity.kind == Kind::Group && children_open() {
                    return Err(invalid("all children must be terminal"));
                }
            }
            Operation::Withdraw => {
                if entity.kind == Kind::Group
                    && self.effective_lifecycle(entity) == Lifecycle::InProgress
                {
                    return Err(invalid(
                        "a Group that is InProgress through its children cannot be withdrawn",
                    ));
                }
            }
            Operation::Accept => {
                if entity.kind == Kind::Group
                    && !ancestors_adopted()
                    && self.descendants(id)?.iter().any(|d| {
                        matches!(
                            d.current.lifecycle,
                            Lifecycle::InProgress | Lifecycle::Completed
                        )
                    })
                {
                    return Err(invalid(
                        "a Group with InProgress or Completed work below it is accepted only under adopted ancestors",
                    ));
                }
            }
            Operation::Reopen => {
                if !ancestors_adopted() {
                    return Err(invalid(adopted_ancestors));
                }
                let dependents: Vec<_> = self
                    .entities()
                    .filter(|e| {
                        e.current.lifecycle == Lifecycle::Completed
                            && e.current.dependencies.contains(id)
                    })
                    .map(|e| e.id.to_string())
                    .collect();
                if !dependents.is_empty() {
                    return Err(invalid(format!(
                        "Completed dependents must be reopened first: {}",
                        dependents.join(", ")
                    )));
                }
            }
            Operation::Release | Operation::Reconsider => {}
        }
        Ok(())
    }

    pub fn set_parent(&mut self, id: &EntityId, parent: Option<EntityId>) -> Result<()> {
        let entity = self.entity(id)?;
        if entity.current.parent == parent {
            return Ok(());
        }
        self.require_open_parent(entity.current.parent.as_ref())?;
        self.require_open_parent(parent.as_ref())?;
        if let Some(destination) = &parent
            && matches!(
                self.effective_lifecycle(entity),
                Lifecycle::InProgress | Lifecycle::Completed
            )
        {
            let destination = self.entity(destination)?;
            if destination.current.lifecycle != Lifecycle::NotStarted
                || !self.ancestors_adopted(destination)?
            {
                return Err(invalid(
                    "InProgress or Completed work moves only under a Group that is adopted (NotStarted) along with all of its ancestors",
                ));
            }
        }
        let mut candidate = self.clone();
        candidate
            .entities
            .get_mut(id)
            .expect("checked Entity")
            .current
            .parent = parent;
        candidate.validate_relations()?;
        *self = candidate;
        Ok(())
    }

    pub fn add_dependency(&mut self, source: &EntityId, target: &EntityId) -> Result<()> {
        self.change_dependency(source, target, true)
    }

    pub fn remove_dependency(&mut self, source: &EntityId, target: &EntityId) -> Result<()> {
        self.change_dependency(source, target, false)
    }

    fn change_dependency(&mut self, source: &EntityId, target: &EntityId, add: bool) -> Result<()> {
        let entity = self.entity(source)?;
        self.entity(target)?;
        if entity.current.dependencies.contains(target) == add {
            return Ok(());
        }
        if entity.current.lifecycle == Lifecycle::Completed {
            return Err(invalid("Completed dependencies are fixed"));
        }
        let mut candidate = self.clone();
        let dependencies = &mut candidate
            .entities
            .get_mut(source)
            .expect("checked Entity")
            .current
            .dependencies;
        if add {
            dependencies.insert(target.clone());
        } else {
            dependencies.remove(target);
        }
        candidate.validate_relations()?;
        *self = candidate;
        Ok(())
    }

    pub(crate) fn validate_relations(&self) -> Result<()> {
        let mut predecessors: BTreeMap<EntityId, BTreeSet<EntityId>> = BTreeMap::new();
        for entity in self.entities() {
            if entity.kind == Kind::Group && entity.current.lifecycle == Lifecycle::InProgress {
                return Err(invalid(format!(
                    "Group {} never stores InProgress",
                    entity.id
                )));
            }
            let mut ancestors = BTreeSet::from([entity.id.clone()]);
            let mut parent = entity.current.parent.as_ref();
            let mut needs = entity.current.dependencies.clone();
            while let Some(id) = parent {
                if !ancestors.insert(id.clone()) {
                    return Err(invalid("containment cycle"));
                }
                let ancestor = self.entity(id)?;
                if ancestor.kind != Kind::Group {
                    return Err(invalid("parent must be a Group"));
                }
                if entity.current.lifecycle == Lifecycle::InProgress
                    && ancestor.current.lifecycle != Lifecycle::NotStarted
                {
                    return Err(invalid(format!(
                        "InProgress Issue {} requires adopted ancestors; {ancestor_id} is {:?}",
                        entity.id,
                        ancestor.current.lifecycle,
                        ancestor_id = ancestor.id
                    )));
                }
                if !ancestor.current.lifecycle.editable() && entity.current.lifecycle.editable() {
                    return Err(invalid("terminal Group has unfinished descendants"));
                }
                needs.extend(ancestor.current.dependencies.iter().cloned());
                parent = ancestor.current.parent.as_ref();
            }
            for target in &entity.current.dependencies {
                let dependency = self.entity(target)?;
                if entity.current.lifecycle == Lifecycle::Completed
                    && dependency.current.lifecycle != Lifecycle::Completed
                {
                    return Err(invalid("Completed Entity has unfinished dependencies"));
                }
            }
            // Keys are checked against `Entity::id` only after this; a mismatch is reported here.
            self.entity(&entity.id)?;
            predecessors.insert(entity.id.clone(), needs);
        }
        let children = self.children_by_parent();
        for id in self.working_groups_in(&children) {
            if !self.ancestors_adopted(self.entity(&id)?)? {
                return Err(invalid(format!(
                    "working Group {id} requires adopted ancestors"
                )));
            }
        }
        // One pass over parents: scanning for children per Entity would be quadratic.
        for entity in self.entities() {
            if let Some(parent) = &entity.current.parent {
                // Two entries sharing one `id` pass the check above and leave a key unindexed.
                predecessors
                    .get_mut(parent)
                    .ok_or_else(|| invalid(format!("missing Entity {parent}")))?
                    .insert(entity.id.clone());
            }
        }
        // Kahn's algorithm on the spec's contracted completion-precondition graph.
        // Terminal entities remain nodes, so cancellation cannot hide a cycle.
        let mut dependents: BTreeMap<EntityId, Vec<EntityId>> = BTreeMap::new();
        let mut ready = Vec::new();
        let mut pending = BTreeMap::new();
        for (id, needs) in predecessors {
            if needs.is_empty() {
                ready.push(id.clone());
            }
            pending.insert(id.clone(), needs.len());
            for need in needs {
                dependents.entry(need).or_default().push(id.clone());
            }
        }
        let mut removed = 0;
        while let Some(id) = ready.pop() {
            removed += 1;
            for dependent in dependents.get(&id).into_iter().flatten() {
                let count = pending.get_mut(dependent).expect("indexed Entity");
                *count -= 1;
                if *count == 0 {
                    ready.push(dependent.clone());
                }
            }
        }
        if removed != self.entities.len() {
            return Err(invalid("completion precondition cycle"));
        }
        Ok(())
    }
}
