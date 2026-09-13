use super::*;
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
pub enum CandidateList {
    Triage,
    Tasks,
}

/// Evaluates only relevant candidates and their ancestors, once per invocation.
/// The returned list is complete; an evaluation error never returns partial rows.
pub fn candidates<E: From<Error>>(
    snapshot: &Snapshot,
    kind: CandidateList,
    evaluate: impl FnMut(&Entity, &str) -> std::result::Result<bool, E>,
) -> std::result::Result<Vec<&Entity>, E> {
    candidates_filtered(snapshot, kind, |_| true, evaluate)
}

/// Filters candidates before evaluation; retained candidates still check every ancestor.
pub fn candidates_filtered<E: From<Error>>(
    snapshot: &Snapshot,
    kind: CandidateList,
    mut include: impl FnMut(&Entity) -> bool,
    mut evaluate: impl FnMut(&Entity, &str) -> std::result::Result<bool, E>,
) -> std::result::Result<Vec<&Entity>, E> {
    let mut cache = BTreeMap::new();
    let mut visible = Vec::new();
    let mut entities: Vec<_> = snapshot.entities().collect();
    entities.sort_by(|a, b| (&a.created_at, &a.id).cmp(&(&b.created_at, &b.id)));
    for entity in entities {
        if !include(entity) {
            continue;
        }
        if matches!(kind, CandidateList::Tasks) && entity.current.lifecycle == Lifecycle::InProgress
        {
            visible.push(entity);
            continue;
        }
        let state = match kind {
            CandidateList::Triage => Lifecycle::Undecided,
            CandidateList::Tasks => Lifecycle::NotStarted,
        };
        if entity.current.lifecycle != state {
            continue;
        }
        let mut ancestors = vec![entity];
        let mut cursor = entity;
        while let Some(parent) = &cursor.current.parent {
            cursor = snapshot.entity(parent)?;
            ancestors.push(cursor);
        }
        let mut surfaced = true;
        for ancestor in ancestors.into_iter().rev() {
            if !ancestor.current.lifecycle.editable() {
                surfaced = false;
                break;
            }
            let satisfied = match cache.get(&ancestor.id) {
                Some(value) => *value,
                None => {
                    let value = match &ancestor.current.condition {
                        Some(command) => evaluate(ancestor, command)?,
                        None => true,
                    };
                    cache.insert(ancestor.id.clone(), value);
                    value
                }
            };
            if !satisfied {
                surfaced = false;
                break;
            }
        }
        if surfaced {
            visible.push(entity);
        }
    }
    Ok(visible)
}
