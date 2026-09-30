use super::record::{Current, View};
use super::{EntityId, Error, Kind, Lifecycle};
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
pub enum CandidateList {
    Proposals,
    Tasks,
}

/// Condition results of one list invocation. Each Entity is evaluated at most once, ancestors
/// before descendants, and nothing below an unsurfaced or terminal ancestor is evaluated. A
/// conflicted or missing ancestor has no condition and ends the chain there.
pub struct Surfacing<'a, E, F> {
    view: &'a View,
    cache: BTreeMap<EntityId, bool>,
    evaluate: F,
    error: std::marker::PhantomData<E>,
}
impl<'a, E: From<Error>, F: FnMut(&EntityId, &str) -> std::result::Result<bool, E>>
    Surfacing<'a, E, F>
{
    pub fn new(view: &'a View, evaluate: F) -> Self {
        Self {
            view,
            cache: BTreeMap::new(),
            evaluate,
            error: std::marker::PhantomData,
        }
    }
    fn chain(&self, id: &EntityId) -> Vec<EntityId> {
        let mut chain = self.view.ancestors(id);
        chain.insert(0, id.clone());
        chain.reverse();
        chain
    }
    /// Whether the Entity and all of its ancestors surface. A conflicted or unknown Entity
    /// has no current value and does not surface.
    pub fn surfaced(&mut self, id: &EntityId) -> std::result::Result<bool, E> {
        for ancestor in self.chain(id) {
            let Some(current): Option<&Current> = self.view.current(&ancestor) else {
                return Ok(false);
            };
            if !current.lifecycle.editable() {
                return Ok(false);
            }
            let satisfied = match self.cache.get(&ancestor) {
                Some(value) => *value,
                None => {
                    let value = match &current.condition {
                        Some(command) => (self.evaluate)(&ancestor, command)?,
                        None => true,
                    };
                    self.cache.insert(ancestor.clone(), value);
                    value
                }
            };
            if !satisfied {
                return Ok(false);
            }
        }
        Ok(true)
    }
    /// Whether the Entity and its ancestors surface as far as this invocation has evaluated
    /// them. Entities outside the evaluated range count as surfaced, so a read never evaluates
    /// anything only to decorate a related row.
    pub fn surfaced_so_far(&self, id: &EntityId) -> bool {
        self.chain(id).into_iter().all(|ancestor| {
            self.view
                .current(&ancestor)
                .is_some_and(|c| c.lifecycle.editable())
                && self.cache.get(&ancestor).copied().unwrap_or(true)
        })
    }
}

/// The rows of a list in creation order. `tasks` rows are NotStarted Issues and Groups that
/// surface, every InProgress Issue, and every Group whose effective lifecycle is InProgress.
/// Such a Group is listed whether or not it surfaces, but its own surfacing is still decided
/// (below surfaced ancestors), so a failing condition of its own fails the list. Conflicted
/// Entities have no current value and are never candidates.
///
/// `include` filters candidates before evaluation: an excluded Entity is not evaluated as a
/// candidate, but when a retained candidate's surfacing is decided it is evaluated like any
/// other ancestor (see [`Surfacing::surfaced`]), whatever its kind.
/// `surfacing` evaluates each condition at most once. An evaluation error fails the whole list
/// and never returns partial rows.
pub fn list_candidates<
    'a,
    E: From<Error>,
    F: FnMut(&EntityId, &str) -> std::result::Result<bool, E>,
>(
    view: &'a View,
    kind: CandidateList,
    mut include: impl FnMut(&EntityId) -> bool,
    surfacing: &mut Surfacing<'a, E, F>,
) -> std::result::Result<Vec<&'a EntityId>, E> {
    let mut visible = Vec::new();
    for id in view.in_creation_order() {
        let Some(current) = view.current(id) else {
            continue;
        };
        if !include(id) {
            continue;
        }
        let listed = match kind {
            CandidateList::Proposals => {
                current.lifecycle == Lifecycle::Undecided && surfacing.surfaced(id)?
            }
            CandidateList::Tasks => match current.lifecycle {
                Lifecycle::InProgress => true,
                Lifecycle::NotStarted => {
                    let surfaced = surfacing.surfaced(id)?;
                    surfaced || (current.kind == Kind::Group && view.working().contains(id))
                }
                _ => false,
            },
        };
        if listed {
            visible.push(id);
        }
    }
    Ok(visible)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lifecycle::record::{Context, Entry, Operation, Recorder, Store};
    use chrono::{TimeZone, Utc};
    use std::collections::BTreeSet;

    fn id(text: &str) -> EntityId {
        text.to_string().try_into().unwrap()
    }
    fn context(seconds: i64) -> Context {
        Context {
            at: Utc.timestamp_opt(seconds, 0).unwrap(),
            recorder: Some(Recorder {
                actor: "agent".into(),
                data: BTreeMap::new(),
            }),
        }
    }
    fn create(
        store: &mut Store,
        name: &str,
        kind: Kind,
        lifecycle: Lifecycle,
        parent: Option<&str>,
        at: i64,
    ) {
        let record = store
            .create(
                id(name),
                Current {
                    kind,
                    lifecycle,
                    owner: None,
                    title: name.into(),
                    description: String::new(),
                    condition: Some(name.into()),
                    parent: parent.map(id),
                    needs: BTreeSet::new(),
                },
                context(at),
            )
            .unwrap();
        store.insert(Entry::Record(record)).unwrap();
    }

    /// Every truth assignment of the four conditions over a three-level plan, in every phase
    /// of the leaf's work, lists exactly what the boolean oracle lists: the surfaced
    /// candidates plus, for tasks, every effectively InProgress Entity. Each condition is
    /// evaluated at most once, only when every ancestor surfaced, and after its ancestors.
    #[test]
    fn candidate_evaluation_matches_boolean_oracle_for_all_three_level_conditions() {
        let mut store = Store::new();
        create(
            &mut store,
            "item",
            Kind::Group,
            Lifecycle::NotStarted,
            None,
            10,
        );
        create(
            &mut store,
            "nested",
            Kind::Group,
            Lifecycle::NotStarted,
            Some("item"),
            11,
        );
        create(
            &mut store,
            "leaf",
            Kind::Issue,
            Lifecycle::NotStarted,
            Some("nested"),
            12,
        );
        create(
            &mut store,
            "draft",
            Kind::Issue,
            Lifecycle::Undecided,
            Some("nested"),
            13,
        );
        for phase in 0..3 {
            if phase > 0 {
                // Starting and completing the leaf makes both Groups effectively InProgress.
                let operation = if phase == 1 {
                    Operation::Start
                } else {
                    Operation::Complete
                };
                let record = store
                    .perform(&id("leaf"), operation, None, context(20 + phase))
                    .unwrap();
                store.insert(Entry::Record(record)).unwrap();
            }
            let view = store.view().unwrap();
            for bits in 0..16 {
                let values = BTreeMap::from([
                    (id("item"), bits & 1 != 0),
                    (id("nested"), bits & 2 != 0),
                    (id("leaf"), bits & 4 != 0),
                    (id("draft"), bits & 8 != 0),
                ]);
                for kind in [CandidateList::Tasks, CandidateList::Proposals] {
                    let mut calls = Vec::new();
                    let mut surfacing =
                        Surfacing::new(&view, |entity: &EntityId, command: &str| {
                            assert_eq!(command, entity.to_string());
                            calls.push(entity.clone());
                            Ok::<_, Error>(values[entity])
                        });
                    let actual: BTreeSet<_> =
                        list_candidates(&view, kind, |_| true, &mut surfacing)
                            .unwrap()
                            .into_iter()
                            .cloned()
                            .collect();
                    let expected: BTreeSet<_> = view
                        .settled()
                        .filter(|(entity, settled)| {
                            let state = settled.current.lifecycle;
                            if matches!(kind, CandidateList::Tasks)
                                && view.effective_lifecycle(entity) == Some(Lifecycle::InProgress)
                            {
                                return true;
                            }
                            let wanted = match kind {
                                CandidateList::Tasks => Lifecycle::NotStarted,
                                CandidateList::Proposals => Lifecycle::Undecided,
                            };
                            if state != wanted {
                                return false;
                            }
                            let mut cursor = *entity;
                            loop {
                                if !values[cursor] {
                                    return false;
                                }
                                match &view.current(cursor).unwrap().parent {
                                    Some(parent) => cursor = parent,
                                    None => return true,
                                }
                            }
                        })
                        .map(|(entity, _)| entity.clone())
                        .collect();
                    assert_eq!(actual, expected, "phase {phase}, bits {bits}");
                    assert_eq!(calls.len(), calls.iter().collect::<BTreeSet<_>>().len());
                    for (index, called) in calls.iter().enumerate() {
                        let mut cursor = called;
                        while let Some(parent) = &view.current(cursor).unwrap().parent {
                            assert!(values[parent]);
                            assert!(calls[..index].contains(parent));
                            cursor = parent;
                        }
                    }
                }
            }
        }
    }
}
