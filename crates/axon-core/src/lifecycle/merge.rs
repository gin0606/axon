use super::*;
use std::collections::BTreeMap;

/// An immutable three-way comparison. Choices select whole Entity values;
/// construction and resolution never evaluate condition commands or publish data.
#[derive(Debug, Clone)]
pub struct MergePlan {
    left: Snapshot,
    right: Snapshot,
    automatic: BTreeMap<EntityId, Side>,
    conflicts: BTreeMap<EntityId, (Candidate, Candidate)>,
}
impl MergePlan {
    pub fn prepare(base: &Snapshot, left: &Snapshot, right: &Snapshot) -> Result<Self> {
        base.validate()?;
        left.validate()?;
        right.validate()?;
        for branch in [left, right] {
            if base.store != branch.store {
                return Err(invalid("different stores"));
            }
            for (id, entity) in &base.entities {
                let next = branch.entity(id)?;
                check_identity(entity, next)?;
            }
            for (id, record) in &base.states {
                if branch.states.get(id) != Some(record) {
                    return Err(invalid(format!("branch omits or changes base record {id}")));
                }
            }
            for (id, note) in &base.notes {
                if branch.notes.get(id) != Some(note) {
                    return Err(invalid(format!("branch omits or changes base Note {id}")));
                }
            }
        }
        let mut records = left.clone();
        super::snapshot::union(&mut records.states, &right.states)?;
        super::snapshot::union(&mut records.notes, &right.notes)?;
        if records
            .states
            .keys()
            .any(|id| records.notes.contains_key(id))
        {
            return Err(invalid("record ID reused across streams"));
        }
        let mut automatic = BTreeMap::new();
        let mut conflicts = BTreeMap::new();
        for id in left.entities.keys().chain(right.entities.keys()) {
            if automatic.contains_key(id) || conflicts.contains_key(id) {
                continue;
            }
            let side = match (left.entities.get(id), right.entities.get(id)) {
                (Some(l), Some(r)) => {
                    check_identity(l, r)?;
                    let old = base.entities.get(id).map(|e| &e.current);
                    if right.includes_candidate(r, l)? {
                        Some(Side::Right)
                    } else if left.includes_candidate(l, r)?
                        || l.current == r.current
                        || old == Some(&r.current)
                    {
                        Some(Side::Left)
                    } else if old == Some(&l.current) {
                        Some(Side::Right)
                    } else {
                        conflicts.insert(id.clone(), (candidate(l), candidate(r)));
                        None
                    }
                }
                (Some(_), None) => Some(Side::Left),
                (None, Some(_)) => Some(Side::Right),
                (None, None) => unreachable!(),
            };
            if let Some(side) = side {
                automatic.insert(id.clone(), side);
            }
        }
        Ok(Self {
            left: left.clone(),
            right: right.clone(),
            automatic,
            conflicts,
        })
    }

    pub fn automatic(&self) -> &BTreeMap<EntityId, Side> {
        &self.automatic
    }
    pub fn conflicts(&self) -> &BTreeMap<EntityId, (Candidate, Candidate)> {
        &self.conflicts
    }

    /// Explicit choices may override automatic choices to repair a structural
    /// conflict. Every unresolved Entity must be selected before whole-plan validation.
    pub fn resolve(
        &self,
        choices: &BTreeMap<EntityId, Side>,
        reason: Option<String>,
        context: Context,
    ) -> Result<Snapshot> {
        let mut selected = self.automatic.clone();
        selected.extend(choices.iter().map(|(id, side)| (id.clone(), *side)));
        self.left
            .integrate_selected(&self.right, &selected, reason, context, true)
    }
}
fn candidate(entity: &Entity) -> Candidate {
    Candidate {
        head: entity.head.clone(),
        current: entity.current.clone(),
    }
}
fn check_identity(left: &Entity, right: &Entity) -> Result<()> {
    if left.root != right.root || left.kind != right.kind || left.created_at != right.created_at {
        return Err(invalid(format!("Entity identity collision {}", left.id)));
    }
    Ok(())
}
impl Snapshot {
    // Text edits have no causal record: head ancestry alone cannot establish that
    // a divergent current value was already considered by a previous integration.
    pub(crate) fn includes_candidate(&self, selected: &Entity, other: &Entity) -> Result<bool> {
        if !self.states.contains_key(&other.head) {
            return Ok(false);
        }
        if selected.head == other.head {
            return Ok(selected.current == other.current);
        }
        if !self.precedes(&other.head, &selected.head)? {
            return Ok(false);
        }
        if selected.current == other.current {
            return Ok(true);
        }
        let wanted = candidate(other);
        for record in self.history(&selected.id)? {
            if let StateEvent::Integration { inputs, .. } = &record.event
                && inputs.contains(&wanted)
            {
                return Ok(true);
            }
        }
        Ok(false)
    }
}
