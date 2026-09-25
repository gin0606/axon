use super::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy)]
pub enum CandidateList {
    Proposals,
    Tasks,
}

/// Condition results of one list invocation. Each Entity is evaluated at most once, ancestors
/// before descendants, and nothing below an unsurfaced or terminal ancestor is evaluated.
pub struct Surfacing<'a, E, F> {
    snapshot: &'a Snapshot,
    cache: BTreeMap<EntityId, bool>,
    evaluate: F,
    error: std::marker::PhantomData<E>,
}
impl<'a, E: From<Error>, F: FnMut(&Entity, &str) -> std::result::Result<bool, E>>
    Surfacing<'a, E, F>
{
    pub fn new(snapshot: &'a Snapshot, evaluate: F) -> Self {
        Self {
            snapshot,
            cache: BTreeMap::new(),
            evaluate,
            error: std::marker::PhantomData,
        }
    }
    /// Whether the Entity and all of its ancestors surface.
    pub fn surfaced(&mut self, entity: &Entity) -> std::result::Result<bool, E> {
        let mut chain = self.snapshot.ancestors(entity)?;
        chain.insert(0, entity);
        for ancestor in chain.into_iter().rev() {
            if !ancestor.current.lifecycle.editable() {
                return Ok(false);
            }
            let satisfied = match self.cache.get(&ancestor.id) {
                Some(value) => *value,
                None => {
                    let value = match &ancestor.current.condition {
                        Some(command) => (self.evaluate)(ancestor, command)?,
                        None => true,
                    };
                    self.cache.insert(ancestor.id.clone(), value);
                    value
                }
            };
            if !satisfied {
                return Ok(false);
            }
        }
        Ok(true)
    }
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
    include: impl FnMut(&Entity) -> bool,
    evaluate: impl FnMut(&Entity, &str) -> std::result::Result<bool, E>,
) -> std::result::Result<Vec<&Entity>, E> {
    list_candidates(
        snapshot,
        kind,
        include,
        &mut Surfacing::new(snapshot, evaluate),
    )
}

/// The rows of a list in creation order. `tasks` rows are NotStarted Issues and Groups that
/// surface, every InProgress Issue, and every Group whose effective lifecycle is InProgress.
/// Such a Group is listed whether or not it surfaces, but its own surfacing is still decided
/// (below surfaced ancestors), so a failing condition of its own fails the list.
pub fn list_candidates<
    'a,
    E: From<Error>,
    F: FnMut(&Entity, &str) -> std::result::Result<bool, E>,
>(
    snapshot: &'a Snapshot,
    kind: CandidateList,
    mut include: impl FnMut(&Entity) -> bool,
    surfacing: &mut Surfacing<'a, E, F>,
) -> std::result::Result<Vec<&'a Entity>, E> {
    // Only `tasks` rows depend on the derived lifecycle of Groups.
    let working = match kind {
        CandidateList::Proposals => BTreeSet::new(),
        CandidateList::Tasks => snapshot.working_groups(),
    };
    let mut visible = Vec::new();
    let mut entities: Vec<_> = snapshot.entities().collect();
    entities.sort_by(|a, b| (&a.created_at, &a.id).cmp(&(&b.created_at, &b.id)));
    for entity in entities {
        if !include(entity) {
            continue;
        }
        let listed = match kind {
            CandidateList::Proposals => {
                entity.current.lifecycle == Lifecycle::Undecided && surfacing.surfaced(entity)?
            }
            CandidateList::Tasks => match entity.current.lifecycle {
                Lifecycle::InProgress => true,
                Lifecycle::NotStarted => {
                    let surfaced = surfacing.surfaced(entity)?;
                    surfaced || working.contains(&entity.id)
                }
                _ => false,
            },
        };
        if listed {
            visible.push(entity);
        }
    }
    Ok(visible)
}
