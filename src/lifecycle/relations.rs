use super::*;
use std::collections::{BTreeMap, BTreeSet};

impl Snapshot {
    pub fn children(&self, id: &EntityId) -> Result<Vec<&Entity>> {
        self.entity(id)?;
        Ok(self
            .entities()
            .filter(|e| e.current.parent.as_ref() == Some(id))
            .collect())
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

    /// Checks explicit operation eligibility without evaluating stored conditions.
    /// Checking `Complete` does not record or imply a final review; performing it does.
    pub fn check_operation(&self, id: &EntityId, operation: Operation) -> Result<()> {
        let entity = self.entity(id)?;
        operation.apply(entity.current.lifecycle)?;
        self.require_open_parent(entity.current.parent.as_ref())?;
        if operation == Operation::Start
            && let Some(parent) = &entity.current.parent
            && self.entity(parent)?.current.lifecycle != Lifecycle::InProgress
        {
            return Err(invalid("parent must be InProgress"));
        }
        if matches!(operation, Operation::Start | Operation::Complete)
            && entity.current.dependencies.iter().any(|id| {
                self.entities
                    .get(id)
                    .is_none_or(|e| e.current.lifecycle != Lifecycle::Completed)
            })
        {
            return Err(invalid("dependencies must be Completed"));
        }
        let children = self.children(id)?;
        if matches!(operation, Operation::Complete | Operation::Cancel)
            && children.iter().any(|e| e.current.lifecycle.editable())
        {
            return Err(invalid("all children must be terminal"));
        }
        if operation == Operation::Release
            && children
                .iter()
                .any(|e| e.current.lifecycle == Lifecycle::InProgress)
        {
            return Err(invalid("cannot release with InProgress children"));
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
        let mut predecessors = BTreeMap::new();
        for entity in self.entities() {
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
                    && ancestor.current.lifecycle != Lifecycle::InProgress
                {
                    return Err(invalid("InProgress Entity requires InProgress ancestors"));
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
            needs.extend(self.children(&entity.id)?.iter().map(|e| e.id.clone()));
            predecessors.insert(entity.id.clone(), needs);
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
