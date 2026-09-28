//! Structured, storage-independent inspection of a record set.
use crate::lifecycle::record::{Current, Note, Record, RecordId, RecordKind, Store, ViolationKind};
use crate::lifecycle::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Undecided,
    Ready,
    Blocked,
    /// A NotStarted Issue whose own or ancestor's condition is unsatisfied. It cannot become a
    /// candidate, whether or not the prerequisites of `Start` are met.
    Unsurfaced,
    InProgressBlocked,
    InProgress,
    Completed,
    Cancelled,
    /// A NotStarted Group without direct children, whether or not it could complete.
    Empty,
    /// A NotStarted Group with children that completes once its final review passes.
    Confirmable,
    /// Several heads: no current value. Takes precedence over every other situation.
    Conflicted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchLocation {
    Title,
    Description,
}

/// One line of a list: the identity, the situation and, for a conflicted Entity, the value of
/// its first head in record ID order (or the common value when the heads agree).
#[derive(Debug, Clone)]
pub struct Row<'a> {
    pub id: &'a EntityId,
    pub kind: Kind,
    pub title: &'a str,
    pub status: Status,
    /// The Entity is attributed a structural violation.
    pub invalid: bool,
    pub matches: Vec<MatchLocation>,
}

pub fn matches_in(current: &Current, query: &str) -> Vec<MatchLocation> {
    let mut locations = Vec::new();
    if current.title.contains(query) {
        locations.push(MatchLocation::Title);
    }
    if current.description.contains(query) {
        locations.push(MatchLocation::Description);
    }
    locations
}

/// One record set with its derived view, for every row of a read.
pub struct View<'a> {
    store: &'a Store,
    view: &'a record::View,
}

impl<'a> View<'a> {
    pub fn new(store: &'a Store, view: &'a record::View) -> Self {
        Self { store, view }
    }
    pub fn store(&self) -> &'a Store {
        self.store
    }
    pub fn derived(&self) -> &'a record::View {
        self.view
    }
    /// Known Entities in creation order (ties by ID).
    pub fn entities(&self) -> Vec<&'a EntityId> {
        self.view.in_creation_order()
    }
    pub fn is_known(&self, id: &EntityId) -> bool {
        self.view.is_known(id)
    }
    /// The heads of a known Entity in record ID order.
    ///
    /// When the Entity has a gap, a head is `likely_newer` when its own parent is missing or
    /// when following parents from it never reaches the Entity's oldest record (its creation
    /// record, or else the earliest record whose parents are all missing). In an ordinary
    /// false conflict the older side reaches that record, directly or through a resolve record
    /// that joins it; with several gaps the mark is only a guess.
    pub fn heads(&self, id: &EntityId) -> Vec<Head<'a>> {
        let oldest = if self.view.gaps().contains_key(id) {
            oldest_record(self.store, id)
        } else {
            None
        };
        self.view
            .heads(id)
            .into_iter()
            .flatten()
            .map(|head| {
                let record = self.store.record(head).expect("a head is a record");
                let likely_newer = oldest.is_some_and(|oldest| {
                    record
                        .parents
                        .iter()
                        .any(|parent| !self.store.contains(parent))
                        || !reaches(self.store, head, oldest)
                });
                Head {
                    id: head,
                    record,
                    likely_newer,
                }
            })
            .collect()
    }
    /// The value a read shows for a known Entity: the current value when settled, the first
    /// head's value in record ID order otherwise.
    pub fn presented(&self, id: &EntityId) -> Option<&'a Current> {
        if let Some(current) = self.view.current(id) {
            return Some(current);
        }
        let head = self.view.heads(id)?.first()?;
        Some(&self.store.record(head).expect("a head is a record").after)
    }
    pub fn current(&self, id: &EntityId) -> Option<&'a Current> {
        self.view.current(id)
    }
    /// The lifecycle used for display and filters: InProgress for a working Group, the stored
    /// value otherwise; none while conflicted.
    pub fn effective(&self, id: &EntityId) -> Option<Lifecycle> {
        self.view.effective_lifecycle(id)
    }
    pub fn children(&self, id: &EntityId) -> &'a [EntityId] {
        self.view.children(id)
    }
    /// Every settled Entity below the Group in tree order: siblings by creation time, parents
    /// first, each once.
    pub fn descendants(&self, id: &EntityId) -> Vec<EntityId> {
        self.view.descendants(id)
    }
    /// An Issue that satisfies the prerequisites of `Start`.
    pub fn startable(&self, id: &EntityId) -> bool {
        self.current(id).is_some_and(|current| {
            current.kind == Kind::Issue
                && current.lifecycle == Lifecycle::NotStarted
                && self.view.check_operation(id, Operation::Start).is_ok()
        })
    }
    fn completable(&self, id: &EntityId) -> bool {
        self.view.check_operation(id, Operation::Complete).is_ok()
    }
    /// Dependencies that are not Completed: the known ones in creation order (a conflicted
    /// dependency is unmet, having no current value), then the ones the store does not hold,
    /// by ID.
    fn unmet_dependencies(&self, id: &EntityId) -> Vec<&'a EntityId> {
        let Some(current) = self.current(id) else {
            return Vec::new();
        };
        let mut unmet = self.sorted(
            current
                .needs
                .iter()
                .filter(|d| self.is_known(d))
                .filter(|d| {
                    self.current(d)
                        .is_none_or(|dep| dep.lifecycle != Lifecycle::Completed)
                })
                .collect(),
        );
        unmet.extend(current.needs.iter().filter(|d| !self.is_known(d)));
        unmet
    }
    /// The relation row of an ID that may not be held by the store.
    fn related_with<E>(
        &self,
        target: &EntityId,
        known: impl Fn(&EntityId) -> std::result::Result<bool, E>,
    ) -> std::result::Result<Related<'a>, E> {
        Ok(Related {
            id: target.clone(),
            row: if self.is_known(target) {
                Some(self.row_with(self.stored_id(target), known)?)
            } else {
                None
            },
        })
    }
    /// Settled ancestors from the parent upwards. On a containment cycle the walk returns to
    /// the Entity itself, which is not its own ancestor for any reason a read shows.
    fn ancestors(&self, id: &EntityId) -> Vec<EntityId> {
        self.view
            .ancestors(id)
            .into_iter()
            .filter(|ancestor| ancestor != id)
            .collect()
    }
    /// Settled ancestors whose stored value is not NotStarted: the ones an unadopted-ancestor
    /// violation counts, except the Entity itself on a containment cycle.
    fn unadopted_settled_ancestors(&self, id: &EntityId) -> Vec<EntityId> {
        self.ancestors(id)
            .into_iter()
            .filter(|a| {
                self.current(a)
                    .is_some_and(|c| c.lifecycle != Lifecycle::NotStarted)
            })
            .collect()
    }
    /// The parent that ends the settled ancestor chain because it has no current value: a
    /// conflicted one, or one the store does not hold.
    fn unsettled_ancestor(&self, id: &EntityId) -> Option<EntityId> {
        let chain = self.view.ancestors(id);
        let last = chain.last().unwrap_or(id);
        let parent = self.current(last)?.parent.clone()?;
        (!self.view.is_settled(&parent)).then_some(parent)
    }
    /// Ancestors that keep the Entity from starting: the unadopted settled ones, then the
    /// conflicted or missing ancestor that ends the chain, if any.
    fn unadopted_ancestors(&self, id: &EntityId) -> Vec<EntityId> {
        let mut found = self.unadopted_settled_ancestors(id);
        found.extend(self.unsettled_ancestor(id));
        found
    }
    /// Unmet dependencies of every ancestor, each listed once.
    fn ancestor_dependencies(&self, ancestors: &[EntityId]) -> Vec<&'a EntityId> {
        let mut found: Vec<&'a EntityId> = Vec::new();
        for ancestor in ancestors {
            for dependency in self.unmet_dependencies(ancestor) {
                if !found.contains(&dependency) {
                    found.push(dependency);
                }
            }
        }
        found
    }
    pub fn sorted(&self, ids: Vec<&'a EntityId>) -> Vec<&'a EntityId> {
        let mut ids = ids;
        ids.sort_by(|a, b| (self.view.created_at(a), *a).cmp(&(self.view.created_at(b), *b)));
        ids
    }
    /// The situation with every condition taken as satisfied: a Group is `Ready` when a
    /// startable Issue is below it, and no Issue is `Unsurfaced`.
    pub fn status(&self, id: &EntityId) -> Status {
        let Ok(status) = self.status_with(id, |_| Ok::<_, std::convert::Infallible>(true));
        status
    }
    /// The situation from evaluated conditions, as `tasks` rows and `show` derive it: `surfaced`
    /// decides whether an Entity and its ancestors surface. A NotStarted Issue that does not
    /// surface is `Unsurfaced`, and a Group is `Ready` only when a startable Issue below it
    /// surfaces.
    pub fn status_with<E>(
        &self,
        id: &EntityId,
        mut surfaced: impl FnMut(&EntityId) -> std::result::Result<bool, E>,
    ) -> std::result::Result<Status, E> {
        let Some(current) = self.current(id) else {
            return Ok(Status::Conflicted);
        };
        Ok(match (current.kind, current.lifecycle) {
            (_, Lifecycle::Undecided) => Status::Undecided,
            (_, Lifecycle::Completed) => Status::Completed,
            (_, Lifecycle::Cancelled) => Status::Cancelled,
            (Kind::Issue, Lifecycle::NotStarted) => {
                if !surfaced(id)? {
                    Status::Unsurfaced
                } else if self.startable(id) {
                    Status::Ready
                } else {
                    Status::Blocked
                }
            }
            (_, Lifecycle::InProgress) if !self.unmet_dependencies(id).is_empty() => {
                Status::InProgressBlocked
            }
            (_, Lifecycle::InProgress) => Status::InProgress,
            (Kind::Group, Lifecycle::NotStarted) => self.group_status(id, surfaced)?,
        })
    }
    /// Empty > Confirmable > Ready > InProgress > Blocked, as the model orders them.
    fn group_status<E>(
        &self,
        group: &EntityId,
        surfaced: impl FnMut(&EntityId) -> std::result::Result<bool, E>,
    ) -> std::result::Result<Status, E> {
        if self.children(group).is_empty() {
            return Ok(Status::Empty);
        }
        if self.completable(group) {
            return Ok(Status::Confirmable);
        }
        Ok(
            if !self.candidate_descendants(group, surfaced)?.1.is_empty() {
                Status::Ready
            } else if self.view.working().contains(group) {
                Status::InProgress
            } else {
                Status::Blocked
            },
        )
    }
    /// The startable descendants of a Group split into those that do not surface and those
    /// that do (the candidates). Every startable descendant is asked, so a failing condition
    /// fails the read regardless of which sibling happens to surface first.
    fn candidate_descendants<E>(
        &self,
        group: &EntityId,
        mut surfaced: impl FnMut(&EntityId) -> std::result::Result<bool, E>,
    ) -> std::result::Result<(Vec<EntityId>, Vec<EntityId>), E> {
        let mut unsurfaced = Vec::new();
        let mut candidates = Vec::new();
        for descendant in self.descendants(group) {
            if self.startable(&descendant) {
                if surfaced(&descendant)? {
                    candidates.push(descendant);
                } else {
                    unsurfaced.push(descendant);
                }
            }
        }
        Ok((unsurfaced, candidates))
    }
    /// The nearest-to-the-root ancestor whose own condition is unsatisfied, if any. Nothing
    /// below it is evaluated, so at most one ancestor is named. A terminal ancestor ends the
    /// walk without naming it: it has no condition to satisfy, and a read names it as an
    /// unadopted ancestor instead.
    fn unsurfaced_ancestor<E>(
        &self,
        ancestors: &[EntityId],
        mut surfaced: impl FnMut(&EntityId) -> std::result::Result<bool, E>,
    ) -> std::result::Result<Option<EntityId>, E> {
        for ancestor in ancestors.iter().rev() {
            if self.is_terminal(ancestor) {
                return Ok(None);
            }
            if !surfaced(ancestor)? {
                return Ok(Some(ancestor.clone()));
            }
        }
        Ok(None)
    }
    fn is_terminal(&self, id: &EntityId) -> bool {
        self.current(id).is_some_and(|c| !c.lifecycle.editable())
    }
    /// The row of a known Entity from a computed situation.
    fn row_from(&self, id: &'a EntityId, status: Status, query: Option<&str>) -> Row<'a> {
        let presented = self.presented(id).expect("a known Entity");
        Row {
            id,
            kind: presented.kind,
            title: &presented.title,
            status,
            invalid: self.view.in_violation(id),
            matches: query.map(|q| matches_in(presented, q)).unwrap_or_default(),
        }
    }
    /// The row of a known Entity with every condition taken as satisfied.
    pub fn row(&self, id: &'a EntityId, query: Option<&str>) -> Row<'a> {
        self.row_from(id, self.status(id), query)
    }
    fn row_with<E>(
        &self,
        id: &'a EntityId,
        surfaced: impl FnMut(&EntityId) -> std::result::Result<bool, E>,
    ) -> std::result::Result<Row<'a>, E> {
        Ok(self.row_from(id, self.status_with(id, surfaced)?, None))
    }
    fn owned_rows<E>(
        &self,
        ids: Vec<EntityId>,
        known: impl Fn(&EntityId) -> std::result::Result<bool, E>,
    ) -> std::result::Result<Vec<Row<'a>>, E> {
        ids.into_iter()
            .map(|id| {
                let id = self.stored_id(&id);
                self.row_with(id, &known)
            })
            .collect()
    }
    /// The view's own reference to a known ID, so rows can borrow it.
    fn stored_id(&self, id: &EntityId) -> &'a EntityId {
        self.view.key(id).expect("a known Entity")
    }
    /// Why a NotStarted Group is stalled: it cannot complete and no Issue below it is a
    /// candidate or InProgress. `surfaced` decides candidates and unsurfaced ancestors from
    /// evaluated conditions; `known` decorates the reason rows from the results so far.
    fn stall_with<'s, E>(
        &'s self,
        group: &'a EntityId,
        mut surfaced: impl FnMut(&EntityId) -> std::result::Result<bool, E>,
        known: impl Fn(&EntityId) -> std::result::Result<bool, E> + 's,
    ) -> std::result::Result<Option<Stall<'a>>, E> {
        let Some(current) = self.current(group) else {
            return Ok(None);
        };
        if current.kind != Kind::Group
            || current.lifecycle != Lifecycle::NotStarted
            || self.completable(group)
        {
            return Ok(None);
        }
        let descendants = self.descendants(group);
        let (unsurfaced_candidates, candidates) =
            self.candidate_descendants(group, &mut surfaced)?;
        if !candidates.is_empty()
            || descendants.iter().any(|d| {
                self.current(d)
                    .is_some_and(|c| c.kind == Kind::Issue && c.lifecycle == Lifecycle::InProgress)
            })
        {
            return Ok(None);
        }
        let ancestors = self.ancestors(group);
        let ancestor_dependencies = self.ancestor_dependencies(&ancestors);
        let unsurfaced_ancestor = self.unsurfaced_ancestor(&ancestors, &mut surfaced)?;
        // Below an unsurfaced or terminal ancestor the Group's own condition is not evaluated,
        // so only with every ancestor surfaced does the Group not surfacing mean its own
        // condition.
        let own_condition_unsatisfied = unsurfaced_ancestor.is_none()
            && !ancestors.iter().any(|a| self.is_terminal(a))
            && !surfaced(group)?;
        let rows = |ids: Vec<&'a EntityId>| {
            ids.into_iter()
                .map(|id| self.row_with(id, &known))
                .collect::<std::result::Result<Vec<_>, E>>()
        };
        let related = |ids: Vec<&'a EntityId>| {
            ids.into_iter()
                .map(|id| self.related_with(id, &known))
                .collect::<std::result::Result<Vec<_>, E>>()
        };
        let mut descendant_dependencies = Vec::new();
        for descendant in descendants
            .iter()
            .filter(|d| self.current(d).is_some_and(|c| c.lifecycle.editable()))
        {
            for dependency in self.unmet_dependencies(descendant) {
                descendant_dependencies.push((
                    self.row_with(self.stored_id(descendant), &known)?,
                    self.related_with(dependency, &known)?,
                ));
            }
        }
        let children = self.children(group);
        Ok(Some(Stall {
            dependencies: related(self.unmet_dependencies(group))?,
            ancestor_dependencies: related(ancestor_dependencies)?,
            descendant_dependencies,
            undecided_children: rows(
                children
                    .iter()
                    .filter(|c| {
                        self.current(c)
                            .is_some_and(|x| x.lifecycle == Lifecycle::Undecided)
                    })
                    .collect(),
            )?,
            open_subgroups: rows(
                children
                    .iter()
                    .filter(|c| {
                        self.current(c)
                            .is_some_and(|x| x.kind == Kind::Group && x.lifecycle.editable())
                    })
                    .collect(),
            )?,
            unsurfaced_candidates: self.owned_rows(unsurfaced_candidates, &known)?,
            own_condition_unsatisfied: rows(
                own_condition_unsatisfied
                    .then_some(group)
                    .into_iter()
                    .collect(),
            )?,
            undecided_ancestors: self.owned_rows(
                ancestors
                    .into_iter()
                    .filter(|a| {
                        self.current(a)
                            .is_some_and(|x| x.lifecycle == Lifecycle::Undecided)
                    })
                    .collect(),
                &known,
            )?,
            unsurfaced_ancestors: self
                .owned_rows(unsurfaced_ancestor.into_iter().collect(), &known)?,
            unadopted_ancestors: self
                .unadopted_ancestors(group)
                .iter()
                .filter(|a| {
                    self.current(a)
                        .is_none_or(|x| x.lifecycle != Lifecycle::Undecided)
                })
                .map(|ancestor| self.related_with(ancestor, &known))
                .collect::<std::result::Result<Vec<_>, E>>()?,
        }))
    }
}

pub fn list<'a>(
    view: &View<'a>,
    mut include: impl FnMut(&View<'a>, &EntityId) -> bool,
    query: Option<&str>,
) -> Vec<Row<'a>> {
    view.entities()
        .into_iter()
        .filter(|id| include(view, id))
        .map(|id| view.row(id, query))
        .collect()
}

pub fn candidates<'a, E: From<Error>>(
    view: &View<'a>,
    kind: CandidateList,
    include: impl FnMut(&EntityId) -> bool,
    evaluate: impl FnMut(&EntityId, &str) -> std::result::Result<bool, E>,
    query: Option<&str>,
) -> std::result::Result<Vec<Row<'a>>, E> {
    let mut surfacing = Surfacing::new(view.derived(), evaluate);
    let ids = list_candidates(view.derived(), kind, include, &mut surfacing)?;
    let mut rows = Vec::with_capacity(ids.len());
    for id in ids {
        let status = match kind {
            CandidateList::Proposals => view.status(id),
            CandidateList::Tasks => view.status_with(id, |issue| surfacing.surfaced(issue))?,
        };
        rows.push(view.row_from(id, status, query));
    }
    Ok(rows)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrerequisiteOperation {
    Start,
    Complete,
}

/// Unmet prerequisites of the next operation, apart from the children a Group waits for.
#[derive(Debug)]
pub struct Prerequisites<'a> {
    pub operation: PrerequisiteOperation,
    /// Ancestors whose stored lifecycle is not NotStarted, then the conflicted or missing
    /// ancestor that ends the chain, which has no row when the store does not hold it.
    pub ancestors: Vec<Related<'a>>,
    /// Unmet dependencies, including ones the store does not hold.
    pub dependencies: Vec<Related<'a>>,
    pub ancestor_dependencies: Vec<Related<'a>>,
    /// For `Start`, the ancestor whose unsatisfied condition keeps the Issue from surfacing.
    pub unsurfaced_ancestors: Vec<Row<'a>>,
}

/// The reasons a Group is stalled; at least one is present.
#[derive(Debug)]
pub struct Stall<'a> {
    /// Unmet dependencies, including ones the store does not hold.
    pub dependencies: Vec<Related<'a>>,
    pub ancestor_dependencies: Vec<Related<'a>>,
    /// An unfinished descendant and the dependency it waits for.
    pub descendant_dependencies: Vec<(Row<'a>, Related<'a>)>,
    pub undecided_children: Vec<Row<'a>>,
    pub open_subgroups: Vec<Row<'a>>,
    /// Startable Issues below the Group that do not surface.
    pub unsurfaced_candidates: Vec<Row<'a>>,
    /// The Group itself, when its own condition was evaluated and is unsatisfied.
    pub own_condition_unsatisfied: Vec<Row<'a>>,
    pub undecided_ancestors: Vec<Row<'a>>,
    /// The ancestor whose unsatisfied condition keeps the Group from surfacing.
    pub unsurfaced_ancestors: Vec<Row<'a>>,
    /// Ancestors that are not adopted other than the Undecided ones: settled ancestors whose
    /// stored value is not NotStarted (terminal ones, a violation Git left), then the
    /// conflicted or missing ancestor that ends the chain, which has no row when the store
    /// does not hold it.
    pub unadopted_ancestors: Vec<Related<'a>>,
}

#[derive(Debug)]
pub struct Descendant<'a> {
    pub row: Row<'a>,
    /// For each ancestor below the inspected root, whether it is its last sibling.
    pub ancestor_last: Vec<bool>,
    pub last: bool,
}

#[derive(Debug)]
pub struct Descendants<'a> {
    pub entries: Vec<Descendant<'a>>,
    pub completed: usize,
    pub cancelled: usize,
    pub awaiting_confirmation: bool,
}

/// The oldest record of an Entity among those without a parent in the set: its creation
/// record, or else the earliest record whose parents are all missing (ties by ID).
fn oldest_record<'a>(store: &'a Store, id: &EntityId) -> Option<&'a RecordId> {
    store
        .records()
        .filter(|(_, record)| {
            &record.entity == id && record.parents.iter().all(|parent| !store.contains(parent))
        })
        .min_by_key(|(record_id, record)| {
            (record.kind != RecordKind::Created, record.at, *record_id)
        })
        .map(|(record_id, _)| record_id)
}

/// Whether following parents from `from` (the record included) reaches `target`.
fn reaches(store: &Store, from: &RecordId, target: &RecordId) -> bool {
    let mut seen = BTreeSet::from([from]);
    let mut pending = vec![from];
    while let Some(id) = pending.pop() {
        if id == target {
            return true;
        }
        let Some(record) = store.record(id) else {
            continue;
        };
        for parent in &record.parents {
            if seen.insert(parent) {
                pending.push(parent);
            }
        }
    }
    false
}

/// One head of a conflicted Entity.
#[derive(Debug, Clone)]
pub struct Head<'a> {
    pub id: &'a RecordId,
    pub record: &'a Record,
    /// This head's parent is missing, or following parents from it does not reach the
    /// Entity's oldest record because a record on the way has a missing parent: the head is
    /// likely the newer side of a gap.
    pub likely_newer: bool,
}

/// An Entity named by a violation, a prerequisite or a stall reason, with its row when the
/// store holds it.
#[derive(Debug)]
pub struct Related<'a> {
    pub id: EntityId,
    pub row: Option<Row<'a>>,
}

/// One violation attributed to the inspected Entity.
#[derive(Debug)]
pub struct Invalid<'a> {
    pub kind: ViolationKind,
    pub related: Vec<Related<'a>>,
}

#[derive(Debug)]
pub struct Detail<'a> {
    pub row: Row<'a>,
    /// The stored lifecycle; none while conflicted.
    pub stored: Option<Lifecycle>,
    pub effective: Option<Lifecycle>,
    pub note_count: usize,
    pub description: &'a str,
    pub condition: Option<&'a str>,
    /// The parent, with its row when it is known.
    pub parent: Option<Related<'a>>,
    /// The heads of a conflicted Entity in record ID order; empty when settled.
    pub heads: Vec<Head<'a>>,
    pub violations: Vec<Invalid<'a>>,
    pub prerequisites: Option<Prerequisites<'a>>,
    pub stall: Option<Stall<'a>>,
    pub dependencies: Vec<Related<'a>>,
    pub dependents: Vec<Row<'a>>,
    pub descendants: Option<Descendants<'a>>,
}

/// `detail_with` with every condition taken as satisfied.
pub fn detail<'a>(view: &View<'a>, id: &EntityId) -> Result<Detail<'a>> {
    // Every condition counts as satisfied, and so does every ancestor: a terminal ancestor
    // (a violation Git left) does not hide its descendants in this mode.
    detail_from(view, id, |_| Ok::<_, Error>(true), |_| Ok(true))
}

/// The detail of one Entity with its situation derived from evaluated conditions. The range
/// is what a `tasks` row of the Entity needs: its ancestors from the root, the Entity itself
/// when it is NotStarted, and for a Group its startable descendants with the Groups between.
/// Each condition runs at most once, and the rows of related Entities use the results so far
/// rather than evaluating further. A conflicted Entity evaluates nothing.
pub fn detail_with<'a, E: From<Error>>(
    view: &View<'a>,
    id: &EntityId,
    evaluate: impl FnMut(&EntityId, &str) -> std::result::Result<bool, E>,
) -> std::result::Result<Detail<'a>, E> {
    // Evaluation and the cache-only lookups never overlap, so one cell serves both closures.
    let surfacing = std::cell::RefCell::new(Surfacing::new(view.derived(), evaluate));
    let surfaced = |e: &EntityId| surfacing.borrow_mut().surfaced(e);
    let known = |e: &EntityId| Ok(surfacing.borrow().surfaced_so_far(e));
    detail_from(view, id, surfaced, known)
}

/// The detail from the given surfacing decisions: `surfaced` for the Entities in the range of
/// the row, `known` for the related rows.
fn detail_from<'a, E: From<Error>>(
    view: &View<'a>,
    id: &EntityId,
    mut surfaced: impl FnMut(&EntityId) -> std::result::Result<bool, E>,
    known: impl Fn(&EntityId) -> std::result::Result<bool, E>,
) -> std::result::Result<Detail<'a>, E> {
    if !view.is_known(id) {
        return Err(Error(format!("missing Entity {id}")).into());
    }
    // A shared reference, so every row can use the same decisions.
    let known = &known;
    let id = view.stored_id(id);
    let presented = view.presented(id).expect("a known Entity");
    let current = view.current(id);
    // The target's own chain is evaluated first, as a tasks row is, whether or not deriving
    // the situation happens to need it (a Group without startable descendants would not).
    if current.is_some_and(|c| c.lifecycle == Lifecycle::NotStarted) {
        surfaced(id)?;
    }
    let status = view.status_with(id, &mut surfaced)?;
    let stall = view.stall_with(id, &mut surfaced, known)?;
    let related = |target: &EntityId| view.related_with(target, known);
    let parent = presented.parent.as_ref().map(related).transpose()?;
    // Known dependencies in creation order, then the ones the store does not hold by ID.
    let mut dependency_ids = view.sorted(
        presented
            .needs
            .iter()
            .filter(|d| view.is_known(d))
            .collect(),
    );
    dependency_ids.extend(presented.needs.iter().filter(|d| !view.is_known(d)));
    let dependencies = dependency_ids
        .into_iter()
        .map(related)
        .collect::<std::result::Result<Vec<_>, E>>()?;
    let prerequisites = if let Some(current) = current
        && stall.is_none()
        && matches!(
            current.lifecycle,
            Lifecycle::NotStarted | Lifecycle::InProgress
        ) {
        let starting = current.kind == Kind::Issue && current.lifecycle == Lifecycle::NotStarted;
        let ancestors = view.ancestors(id);
        let unadopted = view.unadopted_ancestors(id);
        let (ancestor_dependencies, unsurfaced_ancestor) = if starting {
            (
                view.ancestor_dependencies(&ancestors),
                view.unsurfaced_ancestor(&ancestors, known)?,
            )
        } else {
            (Vec::new(), None)
        };
        let unmet = view.unmet_dependencies(id);
        if unadopted.is_empty()
            && unmet.is_empty()
            && ancestor_dependencies.is_empty()
            && unsurfaced_ancestor.is_none()
        {
            None
        } else {
            Some(Prerequisites {
                operation: if starting {
                    PrerequisiteOperation::Start
                } else {
                    PrerequisiteOperation::Complete
                },
                ancestors: unadopted
                    .iter()
                    .map(related)
                    .collect::<std::result::Result<Vec<_>, E>>()?,
                dependencies: unmet
                    .into_iter()
                    .map(related)
                    .collect::<std::result::Result<Vec<_>, E>>()?,
                ancestor_dependencies: ancestor_dependencies
                    .into_iter()
                    .map(related)
                    .collect::<std::result::Result<Vec<_>, E>>()?,
                unsurfaced_ancestors: view
                    .owned_rows(unsurfaced_ancestor.into_iter().collect(), known)?,
            })
        }
    } else {
        None
    };
    let dependents = view
        .entities()
        .into_iter()
        .filter(|e| view.presented(e).is_some_and(|c| c.needs.contains(id)))
        .map(|e| view.row_with(e, &known))
        .collect::<std::result::Result<Vec<_>, E>>()?;
    let descendants = if presented.kind == Kind::Group && current.is_some() {
        let children = view.children(id);
        let count = children.len();
        let mut pending = children
            .iter()
            .enumerate()
            .rev()
            .map(|(i, child)| (child, Vec::new(), i + 1 == count))
            .collect::<Vec<_>>();
        let mut entries = Vec::new();
        let mut completed = 0;
        let mut cancelled = 0;
        let mut seen = BTreeSet::from([id.clone()]);
        while let Some((child, ancestor_last, last)) = pending.pop() {
            if !seen.insert(child.clone()) {
                continue;
            }
            let lifecycle = view.current(child).expect("settled child").lifecycle;
            completed += usize::from(lifecycle == Lifecycle::Completed);
            cancelled += usize::from(lifecycle == Lifecycle::Cancelled);
            if view.current(child).expect("settled child").kind == Kind::Group {
                let mut ancestors = ancestor_last.clone();
                ancestors.push(last);
                let children = view.children(child);
                let count = children.len();
                pending.extend(
                    children
                        .iter()
                        .enumerate()
                        .rev()
                        .map(|(i, child)| (child, ancestors.clone(), i + 1 == count)),
                );
            }
            entries.push(Descendant {
                row: view.row_with(child, &known)?,
                ancestor_last,
                last,
            });
        }
        Some(Descendants {
            entries,
            completed,
            cancelled,
            awaiting_confirmation: status == Status::Confirmable,
        })
    } else {
        None
    };
    let violations = view
        .derived()
        .violations()
        .iter()
        .filter(|v| &v.entity == id)
        .map(|v| {
            let ids = match v.kind {
                ViolationKind::ContainmentCycle => view.ancestors(id),
                ViolationKind::UnknownParent | ViolationKind::OpenUnderTerminal => {
                    presented.parent.iter().cloned().collect()
                }
                ViolationKind::UnadoptedAncestor => view.unadopted_settled_ancestors(id),
                ViolationKind::CompletedWithOpenDependency => presented
                    .needs
                    .iter()
                    .filter(|d| {
                        view.current(d)
                            .is_some_and(|dep| dep.lifecycle != Lifecycle::Completed)
                    })
                    .cloned()
                    .collect(),
                ViolationKind::UnknownDependency => presented
                    .needs
                    .iter()
                    .filter(|d| !view.is_known(d))
                    .cloned()
                    .collect(),
                // The Entity's own completion prerequisites (children, dependencies, the
                // dependencies of its ancestors) that are on a completion cycle too: the edges
                // it contributes to the cycle.
                ViolationKind::CompletionCycle => {
                    let on_cycle = |other: &EntityId| {
                        view.derived().violations().contains(&record::Violation {
                            entity: other.clone(),
                            kind: ViolationKind::CompletionCycle,
                        })
                    };
                    let mut prerequisites: Vec<EntityId> = view.children(id).to_vec();
                    prerequisites.extend(presented.needs.iter().cloned());
                    for ancestor in view.ancestors(id) {
                        if let Some(current) = view.current(&ancestor) {
                            prerequisites.extend(current.needs.iter().cloned());
                        }
                    }
                    let mut seen = BTreeSet::new();
                    prerequisites
                        .into_iter()
                        .filter(|other| {
                            other != id && on_cycle(other) && seen.insert(other.clone())
                        })
                        .collect()
                }
            };
            Ok(Invalid {
                kind: v.kind,
                related: ids
                    .iter()
                    .map(related)
                    .collect::<std::result::Result<Vec<_>, E>>()?,
            })
        })
        .collect::<std::result::Result<Vec<_>, E>>()?;
    Ok(Detail {
        row: view.row_from(id, status, None),
        stored: current.map(|c| c.lifecycle),
        effective: view.effective(id),
        note_count: view.store().notes_of(id).len(),
        description: &presented.description,
        condition: presented.condition.as_deref(),
        parent,
        heads: if view.derived().is_conflicted(id) {
            view.heads(id)
        } else {
            Vec::new()
        },
        violations,
        prerequisites,
        stall,
        dependencies,
        dependents,
        descendants,
    })
}

/// One record of an Entity's history with what a log line needs.
#[derive(Debug)]
pub struct RecordEntry<'a> {
    pub id: &'a RecordId,
    pub record: &'a Record,
    /// The value before this record: its parent's, when the parent is present and unique.
    pub before: Option<&'a Current>,
    pub concurrent_with_previous: bool,
    pub parent_missing: bool,
}

/// The Entity's records other than Notes in the order of `Store::history`: causal, with each
/// concurrent branch kept together.
pub fn history<'a>(store: &'a Store, id: &EntityId) -> Result<Vec<RecordEntry<'a>>> {
    let mut previous: Option<&RecordId> = None;
    store
        .history(id)?
        .into_iter()
        .map(|(record_id, record)| {
            let concurrent_with_previous = previous
                .map(|before| !store.precedes(before, record_id))
                .unwrap_or(false);
            previous = Some(record_id);
            let parent_missing = record.parents.iter().any(|parent| !store.contains(parent));
            let before = match record.kind {
                RecordKind::Resolve { .. } => None,
                _ => record
                    .parents
                    .first()
                    .and_then(|parent| store.record(parent))
                    .map(|parent| &parent.after),
            };
            Ok(RecordEntry {
                id: record_id,
                record,
                before,
                concurrent_with_previous,
                parent_missing,
            })
        })
        .collect()
}

/// The Entity's Notes by time, then by record ID.
pub fn notes<'a>(store: &'a Store, id: &EntityId) -> Vec<(&'a RecordId, &'a Note)> {
    store.notes_of(id)
}

#[derive(Debug)]
pub struct NoteMatch<'a> {
    pub id: &'a RecordId,
    pub note: &'a Note,
    /// Byte range of the first literal match, on UTF-8 boundaries.
    pub range: Range<usize>,
}

/// Matches in every Note: Entities in creation order (Entities with Notes only after them, by
/// ID), Notes by time then ID within an Entity.
pub fn search_notes<'a>(view: &View<'a>, query: &str) -> Result<Vec<NoteMatch<'a>>> {
    if query.is_empty() {
        return Err(Error("Note search query must not be empty".into()));
    }
    let mut grouped = BTreeMap::<&EntityId, Vec<_>>::new();
    for (id, note) in view.store().all_notes() {
        if let Some(position) = note.body.find(query) {
            grouped.entry(&note.entity).or_default().push(NoteMatch {
                id,
                note,
                range: position..position + query.len(),
            });
        }
    }
    let mut order = view.entities();
    order.extend(view.derived().noted_only().iter());
    Ok(order
        .into_iter()
        .flat_map(|entity| grouped.remove(entity).unwrap_or_default())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lifecycle::record::{Entry, Recorder};
    use chrono::{TimeZone, Utc};
    use proptest::prelude::*;

    fn id(value: &str) -> EntityId {
        value.to_owned().try_into().unwrap()
    }
    fn context(at: i64) -> Context {
        Context {
            at: Utc.timestamp_opt(at, 0).unwrap(),
            recorder: Some(Recorder {
                actor: "tester".into(),
                data: BTreeMap::new(),
            }),
        }
    }
    struct Fixture {
        store: Store,
        clock: std::cell::Cell<i64>,
    }
    impl Fixture {
        fn new() -> Self {
            Self {
                store: Store::new(),
                clock: std::cell::Cell::new(100),
            }
        }
        fn tick(&self) -> Context {
            self.clock.set(self.clock.get() + 1);
            context(self.clock.get())
        }
        fn insert(&mut self, record: Record) {
            self.store.insert(Entry::Record(record)).unwrap();
        }
        fn create(
            &mut self,
            name: &str,
            kind: Kind,
            parent: Option<&str>,
            dependencies: &[&str],
            at: i64,
        ) {
            let record = self
                .store
                .create(
                    id(name),
                    Current {
                        kind,
                        lifecycle: Lifecycle::NotStarted,
                        owner: None,
                        title: name.into(),
                        description: "body needle".into(),
                        condition: None,
                        parent: parent.map(id),
                        needs: dependencies.iter().map(|d| id(d)).collect(),
                    },
                    context(at),
                )
                .unwrap();
            self.insert(record);
        }
        fn perform(&mut self, name: &str, operation: Operation) {
            let record = self
                .store
                .perform(&id(name), operation, None, self.tick())
                .unwrap();
            self.insert(record);
        }
        fn add_dependency(&mut self, name: &str, target: &str) {
            let record = self
                .store
                .add_dependency(&id(name), &id(target), self.tick())
                .unwrap()
                .unwrap();
            self.insert(record);
        }
        fn set_condition(&mut self, name: &str, command: Option<&str>) {
            let record = self
                .store
                .set_condition(&id(name), command.map(String::from), self.tick())
                .unwrap()
                .unwrap();
            self.insert(record);
        }
        fn note(&mut self, name: &str, body: &str, at: i64) {
            let note = self
                .store
                .add_note(&id(name), body.into(), None, context(at))
                .unwrap();
            self.store.insert(Entry::Note(note)).unwrap();
        }
        fn derived(&self) -> record::View {
            self.store.view().unwrap()
        }
    }
    fn status_of(f: &Fixture, name: &str) -> Status {
        let derived = f.derived();
        View::new(&f.store, &derived).status(&id(name))
    }
    fn ids(rows: &[Row<'_>]) -> Vec<String> {
        rows.iter().map(|r| r.id.to_string()).collect()
    }
    fn related_ids(related: &[Related<'_>]) -> Vec<String> {
        related.iter().map(|r| r.id.to_string()).collect()
    }
    /// `detail` on a fresh view; the closure receives the value.
    fn inspect<T>(f: &Fixture, name: &str, check: impl FnOnce(Detail<'_>) -> T) -> T {
        let derived = f.derived();
        let view = View::new(&f.store, &derived);
        check(detail(&view, &id(name)).unwrap())
    }

    #[test]
    fn statuses_and_immediate_prerequisites_follow_lifecycle_guards() {
        let mut f = Fixture::new();
        f.create("parent", Kind::Group, None, &[], 1);
        f.create("dependency", Kind::Issue, None, &[], 2);
        f.create("child", Kind::Issue, Some("parent"), &["dependency"], 3);
        f.perform("parent", Operation::Withdraw);
        inspect(&f, "child", |value| {
            assert_eq!(value.row.status, Status::Blocked);
            let prerequisites = value.prerequisites.unwrap();
            assert_eq!(prerequisites.operation, PrerequisiteOperation::Start);
            assert_eq!(related_ids(&prerequisites.ancestors), ["parent"]);
            assert_eq!(related_ids(&prerequisites.dependencies), ["dependency"]);
            assert!(prerequisites.ancestor_dependencies.is_empty());
        });
        f.perform("parent", Operation::Accept);
        inspect(&f, "child", |value| {
            assert!(value.prerequisites.unwrap().ancestors.is_empty());
        });
        f.perform("dependency", Operation::Start);
        f.perform("dependency", Operation::Complete);
        assert_eq!(status_of(&f, "child"), Status::Ready);
        inspect(&f, "child", |value| assert!(value.prerequisites.is_none()));
        f.perform("child", Operation::Start);
        inspect(&f, "child", |value| {
            assert_eq!(value.row.status, Status::InProgress)
        });
        f.create("new-dependency", Kind::Issue, None, &[], 4);
        f.add_dependency("child", "new-dependency");
        inspect(&f, "child", |value| {
            assert_eq!(value.row.status, Status::InProgressBlocked);
            let prerequisites = value.prerequisites.unwrap();
            assert_eq!(prerequisites.operation, PrerequisiteOperation::Complete);
            assert_eq!(related_ids(&prerequisites.dependencies), ["new-dependency"]);
        });
        f.perform("new-dependency", Operation::Cancel);
        assert_eq!(status_of(&f, "child"), Status::InProgressBlocked);
        assert_eq!(status_of(&f, "new-dependency"), Status::Cancelled);
        assert_eq!(status_of(&f, "dependency"), Status::Completed);
        f.create("proposal", Kind::Issue, None, &[], 5);
        f.perform("proposal", Operation::Withdraw);
        assert_eq!(status_of(&f, "proposal"), Status::Undecided);
    }

    #[test]
    fn ancestor_dependencies_are_start_prerequisites_and_the_parent_line_is_deduplicated() {
        let mut f = Fixture::new();
        f.create("outer", Kind::Group, None, &[], 1);
        f.create("gate", Kind::Issue, None, &[], 2);
        f.create("inner", Kind::Group, Some("outer"), &["gate"], 3);
        f.create("leaf", Kind::Issue, Some("inner"), &[], 4);
        inspect(&f, "leaf", |value| {
            assert_eq!(value.row.status, Status::Blocked);
            let prerequisites = value.prerequisites.unwrap();
            assert!(prerequisites.ancestors.is_empty() && prerequisites.dependencies.is_empty());
            assert_eq!(related_ids(&prerequisites.ancestor_dependencies), ["gate"]);
        });
        assert_eq!(status_of(&f, "inner"), Status::Blocked);
        assert_eq!(status_of(&f, "outer"), Status::Blocked);
        f.perform("gate", Operation::Start);
        f.perform("gate", Operation::Complete);
        assert_eq!(status_of(&f, "leaf"), Status::Ready);
        assert_eq!(status_of(&f, "inner"), Status::Ready);
        assert_eq!(status_of(&f, "outer"), Status::Ready);
    }

    #[test]
    fn group_statuses_follow_the_priority_order() {
        let mut f = Fixture::new();
        f.create("plan", Kind::Group, None, &[], 1);
        assert_eq!(status_of(&f, "plan"), Status::Empty);
        assert!(
            f.derived()
                .check_operation(&id("plan"), Operation::Complete)
                .is_ok()
        );
        inspect(&f, "plan", |value| assert!(value.stall.is_none()));
        f.create("first", Kind::Issue, Some("plan"), &[], 2);
        assert_eq!(status_of(&f, "plan"), Status::Ready);
        inspect(&f, "plan", |value| assert!(value.stall.is_none()));
        f.perform("first", Operation::Start);
        assert_eq!(status_of(&f, "plan"), Status::InProgress);
        inspect(&f, "plan", |value| {
            assert_eq!(value.effective, Some(Lifecycle::InProgress))
        });
        f.create("second", Kind::Issue, Some("plan"), &[], 3);
        // Ready wins over InProgress while a startable Issue remains.
        assert_eq!(status_of(&f, "plan"), Status::Ready);
        f.perform("second", Operation::Withdraw);
        assert_eq!(status_of(&f, "plan"), Status::InProgress);
        f.perform("first", Operation::Complete);
        f.perform("second", Operation::Cancel);
        assert_eq!(status_of(&f, "plan"), Status::Confirmable);
        inspect(&f, "plan", |value| {
            assert!(value.descendants.unwrap().awaiting_confirmation)
        });
        f.create("third", Kind::Issue, Some("plan"), &[], 4);
        f.perform("third", Operation::Withdraw);
        assert_eq!(status_of(&f, "plan"), Status::InProgress);
        f.perform("third", Operation::Cancel);
        f.perform("plan", Operation::Complete);
        assert_eq!(status_of(&f, "plan"), Status::Completed);
        f.perform("plan", Operation::Reopen);
        assert_eq!(status_of(&f, "plan"), Status::Confirmable);
        inspect(&f, "plan", |value| {
            assert_eq!(value.effective, Some(Lifecycle::InProgress))
        });
        let mut empty = Fixture::new();
        empty.create("blocked", Kind::Group, None, &[], 1);
        empty.create("draft", Kind::Issue, Some("blocked"), &[], 2);
        empty.perform("draft", Operation::Withdraw);
        assert_eq!(status_of(&empty, "blocked"), Status::Blocked);
    }

    #[test]
    fn stalled_groups_list_their_reasons_on_blocked_working_and_empty_rows() {
        let mut f = Fixture::new();
        f.create("gate", Kind::Issue, None, &[], 1);
        f.create("outer", Kind::Group, None, &[], 2);
        f.perform("outer", Operation::Withdraw);
        f.create("plan", Kind::Group, Some("outer"), &["gate"], 3);
        f.create("draft", Kind::Issue, Some("plan"), &[], 4);
        f.perform("draft", Operation::Withdraw);
        f.create("sub", Kind::Group, Some("plan"), &[], 5);
        f.create("leaf", Kind::Issue, Some("sub"), &["gate"], 6);
        inspect(&f, "plan", |value| {
            assert_eq!(value.row.status, Status::Blocked);
            assert!(value.prerequisites.is_none());
            let stall = value.stall.unwrap();
            assert_eq!(related_ids(&stall.dependencies), ["gate"]);
            assert!(stall.ancestor_dependencies.is_empty());
            assert_eq!(
                stall
                    .descendant_dependencies
                    .iter()
                    .map(|(d, dep)| (d.id.to_string(), dep.id.to_string()))
                    .collect::<Vec<_>>(),
                [("leaf".to_owned(), "gate".to_owned())]
            );
            assert_eq!(ids(&stall.undecided_children), ["draft"]);
            assert_eq!(ids(&stall.open_subgroups), ["sub"]);
            assert_eq!(ids(&stall.undecided_ancestors), ["outer"]);
            // An Undecided ancestor is named once, not also as an unadopted one.
            assert!(stall.unadopted_ancestors.is_empty());
        });
        inspect(&f, "sub", |value| {
            let sub = value.stall.unwrap();
            assert_eq!(related_ids(&sub.ancestor_dependencies), ["gate"]);
            assert_eq!(ids(&sub.undecided_ancestors), ["outer"]);
        });
        // An empty Group that cannot complete is stalled; one that can is not.
        f.create("hollow", Kind::Group, Some("outer"), &[], 7);
        inspect(&f, "hollow", |value| {
            assert_eq!(value.row.status, Status::Empty);
            assert_eq!(ids(&value.stall.unwrap().undecided_ancestors), ["outer"]);
        });
        f.perform("outer", Operation::Accept);
        inspect(&f, "hollow", |value| assert!(value.stall.is_none()));
        // A startable or InProgress grandchild keeps the Group out of the stalled set.
        f.perform("gate", Operation::Start);
        f.perform("gate", Operation::Complete);
        assert_eq!(status_of(&f, "plan"), Status::Ready);
        inspect(&f, "plan", |value| assert!(value.stall.is_none()));
        f.perform("leaf", Operation::Start);
        assert_eq!(status_of(&f, "plan"), Status::InProgress);
        inspect(&f, "plan", |value| assert!(value.stall.is_none()));
        // A working row with completed but no InProgress descendants is stalled too.
        f.perform("leaf", Operation::Complete);
        inspect(&f, "plan", |value| {
            assert_eq!(value.row.status, Status::InProgress);
            let stall = value.stall.unwrap();
            assert_eq!(ids(&stall.undecided_children), ["draft"]);
            assert_eq!(ids(&stall.open_subgroups), ["sub"]);
            assert!(stall.dependencies.is_empty() && stall.undecided_ancestors.is_empty());
        });
        // A working row whose Issue waits for a dependency added during work is not stalled;
        // the dependency is a completion prerequisite of the Group only when it is its own.
        f.perform("draft", Operation::Accept);
        f.perform("draft", Operation::Start);
        f.create("late", Kind::Issue, None, &[], 8);
        f.add_dependency("plan", "late");
        inspect(&f, "plan", |value| {
            assert_eq!(value.row.status, Status::InProgress);
            assert!(value.stall.is_none());
            let prerequisites = value.prerequisites.unwrap();
            assert_eq!(prerequisites.operation, PrerequisiteOperation::Complete);
            assert_eq!(related_ids(&prerequisites.dependencies), ["late"]);
        });
    }

    #[test]
    fn confirmable_follows_the_complete_prerequisites_not_only_terminal_children() {
        let mut f = Fixture::new();
        f.create("gate", Kind::Issue, None, &[], 1);
        f.create("plan", Kind::Group, None, &["gate"], 2);
        f.create("done", Kind::Issue, Some("plan"), &[], 3);
        f.perform("gate", Operation::Start);
        f.perform("gate", Operation::Complete);
        f.perform("done", Operation::Start);
        f.perform("done", Operation::Complete);
        f.perform("gate", Operation::Reopen);
        inspect(&f, "plan", |value| {
            assert_eq!(value.row.status, Status::InProgress);
            assert!(!value.descendants.unwrap().awaiting_confirmation);
            let stall = value.stall.unwrap();
            assert_eq!(related_ids(&stall.dependencies), ["gate"]);
            assert!(
                stall.ancestor_dependencies.is_empty()
                    && stall.descendant_dependencies.is_empty()
                    && stall.undecided_children.is_empty()
                    && stall.open_subgroups.is_empty()
                    && stall.undecided_ancestors.is_empty()
            );
        });
        f.create("outer", Kind::Group, None, &[], 4);
        f.perform("outer", Operation::Withdraw);
        f.create("inner", Kind::Group, Some("outer"), &[], 5);
        f.create("dropped", Kind::Issue, Some("inner"), &[], 6);
        f.perform("dropped", Operation::Cancel);
        inspect(&f, "inner", |value| {
            assert_eq!(value.row.status, Status::Blocked);
            assert!(!value.descendants.unwrap().awaiting_confirmation);
            assert_eq!(ids(&value.stall.unwrap().undecided_ancestors), ["outer"]);
        });
    }

    #[test]
    fn tasks_rows_use_evaluated_conditions_for_group_readiness() {
        let mut f = Fixture::new();
        f.create("plan", Kind::Group, None, &[], 1);
        f.create("hidden", Kind::Issue, Some("plan"), &[], 2);
        f.set_condition("hidden", Some("exit 1"));
        f.create("working", Kind::Group, None, &[], 3);
        f.create("veiled", Kind::Issue, Some("working"), &[], 4);
        f.set_condition("veiled", Some("exit 1"));
        f.create("done", Kind::Issue, Some("working"), &[], 5);
        f.perform("done", Operation::Start);
        f.perform("done", Operation::Complete);
        // Without conditions both Groups have a startable descendant.
        assert_eq!(status_of(&f, "plan"), Status::Ready);
        assert_eq!(status_of(&f, "working"), Status::Ready);
        let derived = f.derived();
        let view = View::new(&f.store, &derived);
        let mut calls = Vec::new();
        let rows = candidates::<Error>(
            &view,
            CandidateList::Tasks,
            |e| view.current(e).unwrap().kind == Kind::Group,
            |entity, _| {
                calls.push(entity.to_string());
                Ok(false)
            },
            None,
        )
        .unwrap();
        assert_eq!(ids(&rows), ["plan", "working"]);
        // A hidden candidate leaves the Group Blocked, or InProgress when work has begun.
        assert_eq!(rows[0].status, Status::Blocked);
        assert_eq!(rows[1].status, Status::InProgress);
        // The descendants are evaluated for the Group rows although the filter excludes them.
        assert_eq!(calls, ["hidden", "veiled"]);
        // Every startable descendant is evaluated, so a failing condition fails the list
        // whichever sibling would have surfaced first.
        f.create("broken", Kind::Issue, Some("plan"), &[], 6);
        f.set_condition("broken", Some("exit 2"));
        let derived = f.derived();
        let view = View::new(&f.store, &derived);
        for filter in [true, false] {
            let view = &view;
            let error = candidates::<Error>(
                view,
                CandidateList::Tasks,
                move |e| !filter || view.current(e).unwrap().kind == Kind::Group,
                |entity, command| {
                    if command == "exit 2" {
                        Err(Error(format!("{entity} failed")))
                    } else {
                        Ok(true)
                    }
                },
                None,
            )
            .unwrap_err();
            assert!(error.to_string().contains("broken failed"), "{error}");
        }
        f.set_condition("broken", None);
        f.set_condition("hidden", None);
        f.set_condition("veiled", None);
        let derived = f.derived();
        let view = View::new(&f.store, &derived);
        let rows =
            candidates::<Error>(&view, CandidateList::Tasks, |_| true, |_, _| Ok(true), None)
                .unwrap();
        assert_eq!(
            ids(&rows),
            ["plan", "hidden", "working", "veiled", "broken"]
        );
        assert!(rows.iter().all(|r| r.status == Status::Ready));
    }

    #[test]
    fn show_derives_the_situation_from_the_conditions_its_row_needs() {
        let mut f = Fixture::new();
        f.create("outer", Kind::Group, None, &[], 1);
        f.set_condition("outer", Some("outer"));
        f.create("plan", Kind::Group, Some("outer"), &[], 2);
        f.create("gate", Kind::Issue, None, &[], 3);
        f.create("hidden", Kind::Issue, Some("plan"), &[], 4);
        f.set_condition("hidden", Some("hidden"));
        f.create("waiting", Kind::Issue, Some("plan"), &["gate"], 5);
        f.set_condition("waiting", Some("waiting"));
        f.create("draft", Kind::Issue, Some("plan"), &[], 6);
        f.perform("draft", Operation::Withdraw);
        f.set_condition("draft", Some("draft"));
        f.create("other", Kind::Group, None, &[], 7);
        f.set_condition("other", Some("other"));
        f.create("elsewhere", Kind::Issue, Some("other"), &[], 8);
        f.set_condition("gate", Some("gate"));
        f.create("consumer", Kind::Issue, None, &["waiting"], 9);
        f.set_condition("consumer", Some("consumer"));
        f.create("vacant", Kind::Group, None, &[], 10);
        f.set_condition("vacant", Some("vacant"));
        f.create("idea", Kind::Issue, Some("vacant"), &[], 11);
        f.perform("idea", Operation::Withdraw);
        f.create("gated", Kind::Group, Some("outer"), &[], 12);
        f.set_condition("gated", Some("gated"));
        f.create("gated-work", Kind::Issue, Some("gated"), &[], 13);
        f.create("bare", Kind::Group, None, &[], 14);
        f.set_condition("bare", Some("bare"));
        f.create("closing", Kind::Group, None, &[], 15);
        f.set_condition("closing", Some("closing"));
        f.create("done", Kind::Issue, Some("closing"), &[], 16);
        f.perform("done", Operation::Start);
        f.perform("done", Operation::Complete);
        f.create("paused", Kind::Group, None, &[], 17);
        f.set_condition("paused", Some("paused"));
        f.create("finished", Kind::Issue, Some("paused"), &[], 18);
        f.perform("finished", Operation::Start);
        f.perform("finished", Operation::Complete);
        f.create("held", Kind::Issue, Some("paused"), &["gate"], 19);
        fn inspect_with<T>(
            f: &Fixture,
            name: &str,
            results: &[(&str, bool)],
            check: impl FnOnce(Detail<'_>, Vec<String>) -> T,
        ) -> T {
            let derived = f.derived();
            let view = View::new(&f.store, &derived);
            let mut calls = Vec::new();
            let value = detail_with::<Error>(&view, &id(name), |entity, command| {
                calls.push(entity.to_string());
                assert_eq!(command, entity.to_string());
                Ok(results
                    .iter()
                    .find(|(id, _)| *id == command)
                    .map(|(_, satisfied)| *satisfied)
                    .unwrap_or(true))
            })
            .unwrap();
            check(value, calls)
        }
        // A Group evaluates its ancestors, itself and its startable descendants only: the
        // Issue waiting for a dependency, the Undecided child and another subtree stay out.
        inspect_with(&f, "plan", &[("hidden", false)], |plan, calls| {
            assert_eq!(calls, ["outer", "hidden"]);
            assert_eq!(plan.row.status, Status::Blocked);
            let stall = plan.stall.unwrap();
            assert_eq!(ids(&stall.unsurfaced_candidates), ["hidden"]);
            assert_eq!(stall.unsurfaced_candidates[0].status, Status::Unsurfaced);
            assert!(stall.unsurfaced_ancestors.is_empty());
            assert!(stall.own_condition_unsatisfied.is_empty());
            assert_eq!(ids(&stall.undecided_children), ["draft"]);
            assert_eq!(
                stall
                    .descendant_dependencies
                    .iter()
                    .map(|(d, dep)| (d.status, dep.id.to_string()))
                    .collect::<Vec<_>>(),
                [(Status::Blocked, "gate".to_owned())]
            );
            let tree = plan.descendants.unwrap();
            assert_eq!(
                tree.entries
                    .iter()
                    .map(|e| e.row.status)
                    .collect::<Vec<_>>(),
                [Status::Unsurfaced, Status::Blocked, Status::Undecided]
            );
        });
        // The hidden Issue itself is Unsurfaced without naming an ancestor.
        inspect_with(&f, "hidden", &[("hidden", false)], |hidden, calls| {
            assert_eq!(calls, ["outer", "hidden"]);
            assert_eq!(hidden.row.status, Status::Unsurfaced);
            assert!(hidden.prerequisites.is_none());
        });
        // Below an unsurfaced ancestor nothing else is evaluated, and the ancestor is named
        // for the Group and for Issues whether or not their prerequisites are met.
        inspect_with(&f, "plan", &[("outer", false)], |plan, calls| {
            assert_eq!(calls, ["outer"]);
            assert_eq!(plan.row.status, Status::Blocked);
            let stall = plan.stall.unwrap();
            assert_eq!(ids(&stall.unsurfaced_candidates), ["hidden"]);
            assert_eq!(ids(&stall.unsurfaced_ancestors), ["outer"]);
            assert!(stall.own_condition_unsatisfied.is_empty());
        });
        inspect_with(&f, "hidden", &[("outer", false)], |hidden, calls| {
            assert_eq!(calls, ["outer"]);
            assert_eq!(hidden.row.status, Status::Unsurfaced);
            let prerequisites = hidden.prerequisites.unwrap();
            assert_eq!(prerequisites.operation, PrerequisiteOperation::Start);
            assert_eq!(ids(&prerequisites.unsurfaced_ancestors), ["outer"]);
            assert!(prerequisites.dependencies.is_empty());
        });
        inspect_with(&f, "waiting", &[("outer", false)], |waiting, calls| {
            assert_eq!(calls, ["outer"]);
            assert_eq!(waiting.row.status, Status::Unsurfaced);
            let prerequisites = waiting.prerequisites.unwrap();
            assert_eq!(ids(&prerequisites.unsurfaced_ancestors), ["outer"]);
            assert_eq!(related_ids(&prerequisites.dependencies), ["gate"]);
        });
        // A surfaced Issue keeps Ready or Blocked. Its dependencies and dependents are
        // outside the range: their conditions do not run and their rows are not Unsurfaced.
        inspect_with(
            &f,
            "waiting",
            &[("gate", false), ("consumer", false)],
            |waiting, calls| {
                assert_eq!(calls, ["outer", "waiting"]);
                assert_eq!(waiting.row.status, Status::Blocked);
                let prerequisites = waiting.prerequisites.unwrap();
                assert!(prerequisites.unsurfaced_ancestors.is_empty());
                assert_eq!(
                    prerequisites.dependencies[0].row.as_ref().unwrap().status,
                    Status::Ready
                );
                assert_eq!(
                    waiting.dependencies[0].row.as_ref().unwrap().status,
                    Status::Ready
                );
                assert_eq!(ids(&waiting.dependents), ["consumer"]);
                assert_eq!(waiting.dependents[0].status, Status::Blocked);
            },
        );
        inspect_with(&f, "gate", &[("waiting", false)], |gate, calls| {
            assert_eq!(calls, ["gate"]);
            assert_eq!(gate.dependents[0].status, Status::Blocked);
        });
        // A Group without startable descendants still evaluates its own chain, as its tasks
        // row would, so a failing condition of its own fails the read.
        inspect_with(&f, "vacant", &[("vacant", false)], |hollow, calls| {
            assert_eq!(calls, ["vacant"]);
            assert_eq!(hollow.row.status, Status::Blocked);
            let stall = hollow.stall.unwrap();
            assert_eq!(ids(&stall.undecided_children), ["idea"]);
            // Its own unsatisfied condition is a reason of its own, as the Group itself.
            assert_eq!(ids(&stall.own_condition_unsatisfied), ["vacant"]);
            assert_eq!(stall.own_condition_unsatisfied[0].status, Status::Blocked);
        });
        inspect_with(&f, "vacant", &[], |hollow, _| {
            assert!(hollow.stall.unwrap().own_condition_unsatisfied.is_empty());
        });
        {
            let derived = f.derived();
            let view = View::new(&f.store, &derived);
            let error = detail_with::<Error>(&view, &id("vacant"), |entity, _| {
                Err(Error(format!("{entity} failed")))
            })
            .unwrap_err();
            assert!(error.to_string().contains("vacant failed"), "{error}");
        }
        // A Group whose own condition fails hides its candidates without an ancestor line.
        inspect_with(&f, "other", &[("other", false)], |other, calls| {
            assert_eq!(calls, ["other"]);
            assert_eq!(other.row.status, Status::Blocked);
            let stall = other.stall.unwrap();
            assert_eq!(ids(&stall.unsurfaced_candidates), ["elsewhere"]);
            assert!(stall.unsurfaced_ancestors.is_empty());
            assert_eq!(ids(&stall.own_condition_unsatisfied), ["other"]);
        });
        // Below an unsurfaced ancestor the Group's own condition is not evaluated, so an
        // unsatisfied one is not a reason; with the ancestor surfaced it is.
        inspect_with(
            &f,
            "gated",
            &[("outer", false), ("gated", false)],
            |gated, calls| {
                assert_eq!(calls, ["outer"]);
                let stall = gated.stall.unwrap();
                assert_eq!(ids(&stall.unsurfaced_candidates), ["gated-work"]);
                assert_eq!(ids(&stall.unsurfaced_ancestors), ["outer"]);
                assert!(stall.own_condition_unsatisfied.is_empty());
            },
        );
        inspect_with(&f, "gated", &[("gated", false)], |gated, calls| {
            assert_eq!(calls, ["outer", "gated"]);
            let stall = gated.stall.unwrap();
            assert!(stall.unsurfaced_ancestors.is_empty());
            assert_eq!(ids(&stall.own_condition_unsatisfied), ["gated"]);
        });
        // Groups that are not stalled get no reason from their own unsatisfied condition:
        // an Empty Group that can complete, a Confirmable Group, and one with a candidate.
        inspect_with(&f, "bare", &[("bare", false)], |bare, calls| {
            assert_eq!(calls, ["bare"]);
            assert_eq!(bare.row.status, Status::Empty);
            assert!(bare.stall.is_none());
        });
        inspect_with(&f, "closing", &[("closing", false)], |closing, calls| {
            assert_eq!(calls, ["closing"]);
            assert_eq!(closing.row.status, Status::Confirmable);
            assert!(closing.stall.is_none());
        });
        inspect_with(&f, "plan", &[("outer", true)], |plan, _| {
            assert_eq!(plan.row.status, Status::Ready);
            assert!(plan.stall.is_none());
        });
        // A stalled Group that is effectively InProgress names its own condition as itself.
        inspect_with(&f, "paused", &[("paused", false)], |paused, calls| {
            assert_eq!(calls, ["paused"]);
            assert_eq!(paused.row.status, Status::InProgress);
            let stall = paused.stall.unwrap();
            assert_eq!(ids(&stall.own_condition_unsatisfied), ["paused"]);
            assert_eq!(
                stall.own_condition_unsatisfied[0].status,
                Status::InProgress
            );
            assert_eq!(
                stall
                    .descendant_dependencies
                    .iter()
                    .map(|(d, dep)| (d.id.to_string(), dep.id.to_string()))
                    .collect::<Vec<_>>(),
                [("held".to_owned(), "gate".to_owned())]
            );
        });
        // Undecided and InProgress targets evaluate nothing.
        inspect_with(&f, "draft", &[("draft", false)], |draft, calls| {
            assert!(calls.is_empty());
            assert_eq!(draft.row.status, Status::Undecided);
        });
        f.perform("elsewhere", Operation::Start);
        inspect_with(&f, "elsewhere", &[("other", false)], |elsewhere, calls| {
            assert!(calls.is_empty());
            assert_eq!(elsewhere.row.status, Status::InProgress);
        });
        // With a descendant InProgress the Group is not stalled, whatever its own condition.
        inspect_with(&f, "other", &[("other", false)], |other, calls| {
            assert_eq!(calls, ["other"]);
            assert_eq!(other.row.status, Status::InProgress);
            assert!(other.stall.is_none());
        });
        // A failing evaluation fails the whole read, and skipping conditions never evaluates.
        let derived = f.derived();
        let view = View::new(&f.store, &derived);
        let error = detail_with::<Error>(&view, &id("plan"), |entity, _| {
            Err(Error(format!("{entity} failed")))
        })
        .unwrap_err();
        assert!(error.to_string().contains("outer failed"), "{error}");
        let plan = detail(&view, &id("plan")).unwrap();
        assert_eq!(plan.row.status, Status::Ready);
        assert!(plan.stall.is_none());
        let hollow = detail(&view, &id("vacant")).unwrap();
        assert!(hollow.stall.unwrap().own_condition_unsatisfied.is_empty());
    }

    #[test]
    fn descendants_are_preorder_with_sibling_structure_and_terminal_counts() {
        let mut f = Fixture::new();
        f.create("root", Kind::Group, None, &[], 1);
        f.create("subgroup", Kind::Group, Some("root"), &[], 3);
        f.create("first", Kind::Issue, Some("root"), &[], 2);
        f.create("nested", Kind::Issue, Some("subgroup"), &[], 4);
        f.perform("first", Operation::Cancel);
        f.perform("nested", Operation::Start);
        f.perform("nested", Operation::Complete);
        inspect(&f, "root", |value| {
            let tree = value.descendants.unwrap();
            assert_eq!(
                tree.entries
                    .iter()
                    .map(|e| e.row.id.to_string())
                    .collect::<Vec<_>>(),
                ["first", "subgroup", "nested"]
            );
            assert_eq!(tree.entries[1].row.status, Status::Confirmable);
            assert!(!tree.entries[0].last);
            assert!(tree.entries[1].last);
            assert_eq!(tree.entries[2].ancestor_last, [true]);
            assert_eq!((tree.completed, tree.cancelled), (1, 1));
            assert!(!tree.awaiting_confirmation);
        });
        f.perform("subgroup", Operation::Complete);
        inspect(&f, "root", |value| {
            assert!(value.descendants.unwrap().awaiting_confirmation)
        });
    }

    #[test]
    fn search_and_candidates_return_values_without_formatting() {
        let mut f = Fixture::new();
        f.create("needle", Kind::Issue, None, &[], 2);
        f.create("earlier", Kind::Issue, None, &[], 1);
        f.set_condition("needle", Some("check"));
        {
            let derived = f.derived();
            let view = View::new(&f.store, &derived);
            let rows = list(&view, |_, _| true, Some("needle"));
            assert_eq!(*rows[0].id, id("earlier"));
            assert_eq!(rows[0].matches, [MatchLocation::Description]);
            assert_eq!(
                rows[1].matches,
                [MatchLocation::Title, MatchLocation::Description]
            );
            let mut calls = Vec::new();
            let rows = candidates::<Error>(
                &view,
                CandidateList::Tasks,
                |_| true,
                |entity, command| {
                    calls.push((entity.clone(), command.to_owned()));
                    Ok(false)
                },
                None,
            )
            .unwrap();
            assert_eq!(calls, [(id("needle"), "check".to_owned())]);
            assert_eq!(rows.len(), 1);
            assert_eq!(*rows[0].id, id("earlier"));
        }
        f.note("needle", "日本語 needle needle", 0);
        f.note("earlier", "needle first", 99);
        let derived = f.derived();
        let view = View::new(&f.store, &derived);
        let matches = search_notes(&view, "needle").unwrap();
        assert_eq!(matches[0].note.entity, id("earlier"));
        assert_eq!(matches[1].range, 10..16);
        assert!(search_notes(&view, "NEEDLE").unwrap().is_empty());
        assert!(search_notes(&view, "").is_err());
    }

    #[test]
    fn list_filters_see_the_effective_lifecycle_of_groups() {
        let mut f = Fixture::new();
        f.create("plan", Kind::Group, None, &[], 1);
        f.create("work", Kind::Issue, Some("plan"), &[], 2);
        f.perform("work", Operation::Start);
        let derived = f.derived();
        let view = View::new(&f.store, &derived);
        let working = list(
            &view,
            |view, e| view.effective(e) == Some(Lifecycle::InProgress),
            None,
        );
        assert_eq!(ids(&working), ["plan", "work"]);
        assert!(
            list(
                &view,
                |view, e| view.effective(e) == Some(Lifecycle::NotStarted),
                None
            )
            .is_empty()
        );
    }

    #[test]
    fn history_preserves_causality_and_exposes_concurrent_branches_and_missing_parents() {
        let mut f = Fixture::new();
        f.create("item", Kind::Issue, None, &[], 10);
        f.note("item", "base", 40);
        let mut left = Fixture {
            store: f.store.clone(),
            clock: std::cell::Cell::new(200),
        };
        let mut right = Fixture {
            store: f.store.clone(),
            clock: std::cell::Cell::new(300),
        };
        left.note("item", "left", 1);
        right.note("item", "right", 50);
        left.perform("item", Operation::Start);
        right.perform("item", Operation::Cancel);
        left.store.absorb(&right.store);
        let notes = notes(&left.store, &id("item"));
        assert_eq!(
            notes
                .iter()
                .map(|(_, n)| n.body.as_str())
                .collect::<Vec<_>>(),
            ["left", "base", "right"]
        );
        let history = history(&left.store, &id("item")).unwrap();
        assert_eq!(history.len(), 3);
        assert_eq!(history[0].record.kind, RecordKind::Created);
        assert!(history[0].before.is_none());
        assert!(!history[1].concurrent_with_previous);
        assert!(history[2].concurrent_with_previous);
        assert_eq!(history[1].before.unwrap().lifecycle, Lifecycle::NotStarted);
        let derived = left.derived();
        assert!(derived.is_conflicted(&id("item")));
        let view = View::new(&left.store, &derived);
        assert_eq!(view.heads(&id("item")).len(), 2);
        assert_eq!(view.status(&id("item")), Status::Conflicted);
        let detail = detail(&view, &id("item")).unwrap();
        assert_eq!(detail.heads.len(), 2);
        assert!(detail.stored.is_none() && detail.prerequisites.is_none());
        // A settled Entity has one head and shows none.
        let mut settled = Fixture::new();
        settled.create("plain", Kind::Issue, None, &[], 1);
        let derived = settled.derived();
        let view = View::new(&settled.store, &derived);
        assert!(super::detail(&view, &id("plain")).unwrap().heads.is_empty());
        // A record whose parent is missing shows as such and has no value before it.
        let mut gapped = Store::new();
        let (last_id, last) = left
            .store
            .records()
            .find(|(_, r)| matches!(r.kind, RecordKind::Transition(Operation::Cancel)))
            .unwrap();
        gapped.insert(Entry::Record(last.clone())).unwrap();
        let history = super::history(&gapped, &id("item")).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].id, last_id);
        assert!(history[0].parent_missing && history[0].before.is_none());
    }

    #[test]
    fn a_head_cut_off_from_the_oldest_record_is_likely_newer_and_a_head_reaching_it_is_not() {
        let item = id("item");
        let heads_of = |store: &Store| -> Vec<bool> {
            let derived = store.view().unwrap();
            let view = View::new(store, &derived);
            view.heads(&item).iter().map(|h| h.likely_newer).collect()
        };
        let newer_of = |store: &Store| -> Vec<RecordId> {
            let derived = store.view().unwrap();
            let view = View::new(store, &derived);
            view.heads(&item)
                .into_iter()
                .filter(|h| h.likely_newer)
                .map(|h| h.id.clone())
                .collect()
        };
        let mut f = Fixture::new();
        f.create("item", Kind::Issue, None, &[], 10);
        let base = f.store.clone();
        // Start → Release → Start, and only the last two arrive: the gap is below the head.
        let mut a = Fixture {
            store: base.clone(),
            clock: std::cell::Cell::new(200),
        };
        a.perform("item", Operation::Start);
        let start = a.store.view().unwrap().head(&item).unwrap().clone();
        a.perform("item", Operation::Release);
        a.perform("item", Operation::Start);
        let mut main = base.clone();
        for (record_id, record) in a.store.records() {
            if record_id != &start {
                main.insert(Entry::Record(record.clone())).unwrap();
            }
        }
        assert_eq!(heads_of(&main).iter().filter(|m| **m).count(), 1);
        let newer = newer_of(&main).remove(0);
        assert_ne!(main.record(&newer).unwrap().kind, RecordKind::Created);
        let resolved = main.resolve(&item, &newer, None, context(300)).unwrap();
        main.insert(Entry::Record(resolved)).unwrap();
        assert!(main.view().unwrap().gaps().contains_key(&item));
        // A real conflict above the gap: both heads reach the creation through the resolve.
        let mut left = Fixture {
            store: main.clone(),
            clock: std::cell::Cell::new(400),
        };
        let mut right = Fixture {
            store: main.clone(),
            clock: std::cell::Cell::new(500),
        };
        left.perform("item", Operation::Release);
        right.perform("item", Operation::Cancel);
        left.store.absorb(&right.store);
        assert_eq!(heads_of(&left.store), [false, false]);
        // A later false conflict on the gapped Entity: Release → Start, only the Start
        // arrives. Its cut-off side is marked, the resolve side is not.
        let mut later = Fixture {
            store: main.clone(),
            clock: std::cell::Cell::new(600),
        };
        later.perform("item", Operation::Release);
        later.perform("item", Operation::Start);
        let last = later.store.view().unwrap().head(&item).unwrap().clone();
        let mut picked = main.clone();
        picked
            .insert(Entry::Record(later.store.record(&last).unwrap().clone()))
            .unwrap();
        assert_eq!(newer_of(&picked), [last]);
        assert_eq!(heads_of(&picked).len(), 2);
        // Without a gap nothing is marked.
        let mut plain_left = Fixture {
            store: base.clone(),
            clock: std::cell::Cell::new(700),
        };
        let mut plain_right = Fixture {
            store: base.clone(),
            clock: std::cell::Cell::new(800),
        };
        plain_left.perform("item", Operation::Start);
        plain_right.perform("item", Operation::Cancel);
        plain_left.store.absorb(&plain_right.store);
        assert_eq!(heads_of(&plain_left.store), [false, false]);
        // A resolve record whose one parent is missing is marked even though its other parent
        // reaches the creation.
        let mut left = Fixture {
            store: base.clone(),
            clock: std::cell::Cell::new(900),
        };
        let mut right = Fixture {
            store: base.clone(),
            clock: std::cell::Cell::new(1000),
        };
        left.perform("item", Operation::Start);
        let started = left.store.view().unwrap().head(&item).unwrap().clone();
        left.perform("item", Operation::Release);
        let released = left.store.view().unwrap().head(&item).unwrap().clone();
        right.perform("item", Operation::Cancel);
        let cancelled = right.store.view().unwrap().head(&item).unwrap().clone();
        let mut joined = left.store.clone();
        joined.absorb(&right.store);
        let resolved = joined
            .resolve(&item, &cancelled, None, context(1100))
            .unwrap();
        let mut partial = base.clone();
        for record_id in [&started, &cancelled] {
            partial
                .insert(Entry::Record(joined.record(record_id).unwrap().clone()))
                .unwrap();
        }
        let resolved = partial.insert(Entry::Record(resolved)).unwrap();
        assert!(!partial.contains(&released));
        assert_eq!(newer_of(&partial), [resolved]);
        // Without the creation record, the oldest record stands in for it: a real conflict
        // above the gap marks neither head.
        let mut rootless = Store::new();
        for (record_id, record) in a.store.records() {
            if record_id != &start && record.kind != RecordKind::Created {
                rootless.insert(Entry::Record(record.clone())).unwrap();
            }
        }
        let mut left = Fixture {
            store: rootless.clone(),
            clock: std::cell::Cell::new(1200),
        };
        let mut right = Fixture {
            store: rootless,
            clock: std::cell::Cell::new(1300),
        };
        left.perform("item", Operation::Release);
        right.perform("item", Operation::Cancel);
        left.store.absorb(&right.store);
        assert_eq!(heads_of(&left.store), [false, false]);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn generated_gap_heads_follow_parent_paths(seed in any::<u16>(), branches in prop::collection::vec(any::<bool>(), 0..4)) {
            let item = id("item");
            let mut base = Fixture::new();
            base.create("item", Kind::Issue, None, &[], seed as i64 + 10);
            let created = base.store.view().unwrap().head(&item).unwrap().clone();
            let mut left = Fixture { store: base.store.clone(), clock: std::cell::Cell::new(seed as i64 + 100) };
            left.perform("item", Operation::Start);
            let start = left.store.view().unwrap().head(&item).unwrap().clone();
            left.perform("item", Operation::Release);
            let released = left.store.view().unwrap().head(&item).unwrap().clone();
            let mut right = Fixture { store: base.store.clone(), clock: std::cell::Cell::new(seed as i64 + 200) };
            right.perform("item", Operation::Cancel);
            let cancelled = right.store.view().unwrap().head(&item).unwrap().clone();
            left.store.absorb(&right.store);
            let resolved = left.store.resolve(&item, &released, None, context(seed as i64 + 300)).unwrap();
            let resolved = left.store.insert(Entry::Record(resolved)).unwrap();

            let without = |source: &Store, omitted: &[RecordId]| {
                let mut store = Store::new();
                for (record_id, record) in source.records() {
                    if !omitted.contains(record_id) {
                        store.insert(Entry::Record(record.clone())).unwrap();
                    }
                }
                store
            };
            let mut true_left = Fixture { store: left.store.clone(), clock: std::cell::Cell::new(seed as i64 + 400) };
            let mut true_right = Fixture { store: left.store.clone(), clock: std::cell::Cell::new(seed as i64 + 500) };
            true_left.perform("item", Operation::Start);
            true_right.perform("item", Operation::Cancel);
            true_left.store.absorb(&true_right.store);
            let true_conflict = without(&true_left.store, std::slice::from_ref(&start));
            prop_assert_eq!(true_conflict.view().unwrap().heads(&item).unwrap().len(), 2);

            let mut later = Fixture { store: left.store.clone(), clock: std::cell::Cell::new(seed as i64 + 600) };
            later.perform("item", Operation::Start);
            later.perform("item", Operation::Release);
            later.perform("item", Operation::Start);
            let last = later.store.view().unwrap().head(&item).unwrap().clone();
            let mut false_conflict = without(&left.store, std::slice::from_ref(&start));
            false_conflict.insert(Entry::Record(later.store.record(&last).unwrap().clone())).unwrap();
            prop_assert_eq!(false_conflict.view().unwrap().heads(&item).unwrap().len(), 2);

            let mut branched = left.store.clone();
            for (index, extend) in branches.iter().enumerate() {
                let mut fork = Fixture { store: left.store.clone(), clock: std::cell::Cell::new(seed as i64 + 700 + index as i64 * 10) };
                fork.perform("item", Operation::Start);
                if *extend {
                    fork.perform("item", Operation::Release);
                }
                branched.absorb(&fork.store);
            }
            let cases = [
                (left.store.clone(), false),
                (without(&left.store, std::slice::from_ref(&start)), true),
                (without(&left.store, std::slice::from_ref(&released)), true),
                (without(&left.store, &[created.clone(), start.clone()]), true),
                (true_conflict, true),
                (false_conflict, true),
            ].into_iter().chain(std::iter::once((without(&branched, std::slice::from_ref(&start)), true)));
            for (store, has_gap) in cases {
                let derived = store.view().unwrap();
                let view = View::new(&store, &derived);
                let records: Vec<_> = store.records().filter(|(_, record)| record.entity == item).collect();
                let oldest = records.iter().filter(|(_, record)| record.kind == RecordKind::Created)
                    .min_by_key(|(record_id, record)| (record.at, *record_id))
                    .or_else(|| records.iter().filter(|(_, record)| record.parents.iter().all(|parent| !store.contains(parent)))
                        .min_by_key(|(record_id, record)| (record.at, *record_id)))
                    .unwrap().0;
                for head in view.heads(&item) {
                    let mut pending = vec![head.id];
                    let mut seen = BTreeSet::new();
                    while let Some(record_id) = pending.pop() {
                        if seen.insert(record_id) && let Some(record) = store.record(record_id) {
                            pending.extend(record.parents.iter());
                        }
                    }
                    let expected = derived.gaps().contains_key(&item)
                        && (head.record.parents.iter().any(|parent| !store.contains(parent))
                            || !seen.contains(oldest));
                    prop_assert_eq!(head.likely_newer, expected);
                }
                prop_assert_eq!(derived.gaps().contains_key(&item), has_gap);
            }
            prop_assert!(left.store.contains(&cancelled));
            prop_assert!(left.store.contains(&resolved));
        }
    }

    #[test]
    fn a_dependency_the_store_does_not_hold_is_a_listed_prerequisite() {
        let mut f = Fixture::new();
        f.create("dep", Kind::Issue, None, &[], 1);
        f.create("user", Kind::Issue, None, &["dep"], 2);
        f.perform("dep", Operation::Start);
        f.perform("dep", Operation::Complete);
        f.perform("user", Operation::Start);
        // The dependency's records vanish, as a revert leaves them.
        let mut alone = Fixture::new();
        for (_, entry) in f.store.entries() {
            if entry.entity() == &id("user") {
                alone.store.insert(entry.clone()).unwrap();
            }
        }
        let derived = alone.derived();
        assert!(
            derived
                .violations()
                .iter()
                .any(|v| { v.entity == id("user") && v.kind == ViolationKind::UnknownDependency })
        );
        let view = View::new(&alone.store, &derived);
        assert_eq!(view.status(&id("user")), Status::InProgressBlocked);
        let value = detail(&view, &id("user")).unwrap();
        let prerequisites = value.prerequisites.unwrap();
        assert_eq!(prerequisites.operation, PrerequisiteOperation::Complete);
        assert_eq!(related_ids(&prerequisites.dependencies), ["dep"]);
        assert!(prerequisites.dependencies[0].row.is_none());
    }

    #[test]
    fn missing_dependencies_are_listed_after_known_ones_on_every_stall_and_prerequisite_path() {
        let mut f = Fixture::new();
        f.create("gone-outer", Kind::Issue, None, &[], 1);
        f.create("gone-plan", Kind::Issue, None, &[], 2);
        f.create("gone-leaf", Kind::Issue, None, &[], 3);
        f.create("kept", Kind::Issue, None, &[], 4);
        f.create("outer", Kind::Group, None, &["gone-outer"], 5);
        f.create(
            "plan",
            Kind::Group,
            Some("outer"),
            &["kept", "gone-plan"],
            6,
        );
        f.create("leaf", Kind::Issue, Some("plan"), &["gone-leaf"], 7);
        // The records of every `gone-*` Issue vanish, as a revert leaves them.
        let mut alone = Fixture::new();
        for (_, entry) in f.store.entries() {
            if !entry.entity().as_ref().starts_with("gone-") {
                alone.store.insert(entry.clone()).unwrap();
            }
        }
        let derived = alone.derived();
        let view = View::new(&alone.store, &derived);
        let plan = detail(&view, &id("plan")).unwrap();
        assert_eq!(plan.row.status, Status::Blocked);
        let stall = plan.stall.unwrap();
        // Known dependencies first, then the missing one, which has no row.
        assert_eq!(related_ids(&stall.dependencies), ["kept", "gone-plan"]);
        assert!(stall.dependencies[0].row.is_some() && stall.dependencies[1].row.is_none());
        assert_eq!(related_ids(&stall.ancestor_dependencies), ["gone-outer"]);
        assert_eq!(
            stall
                .descendant_dependencies
                .iter()
                .map(|(d, dep)| (d.id.to_string(), dep.id.to_string(), dep.row.is_none()))
                .collect::<Vec<_>>(),
            [("leaf".to_owned(), "gone-leaf".to_owned(), true)]
        );
        let leaf = detail(&view, &id("leaf")).unwrap();
        let prerequisites = leaf.prerequisites.unwrap();
        assert_eq!(prerequisites.operation, PrerequisiteOperation::Start);
        assert_eq!(related_ids(&prerequisites.dependencies), ["gone-leaf"]);
        assert_eq!(
            related_ids(&prerequisites.ancestor_dependencies),
            ["kept", "gone-plan", "gone-outer"]
        );
    }

    #[test]
    fn an_unfinished_child_under_a_terminal_parent_is_not_unsurfaced_without_conditions() {
        // One side completes the Group while the other registers a child under it.
        let mut f = Fixture::new();
        f.create("plan", Kind::Group, None, &[], 1);
        let mut other = Fixture {
            store: f.store.clone(),
            clock: std::cell::Cell::new(500),
        };
        other.create("late", Kind::Issue, Some("plan"), &[], 600);
        f.perform("plan", Operation::Complete);
        f.store.absorb(&other.store);
        let derived = f.derived();
        assert!(
            derived
                .violations()
                .iter()
                .any(|v| { v.entity == id("late") && v.kind == ViolationKind::OpenUnderTerminal })
        );
        let view = View::new(&f.store, &derived);
        let value = detail(&view, &id("late")).unwrap();
        assert_eq!(value.row.status, Status::Blocked);
        assert!(value.row.invalid);
        let prerequisites = value.prerequisites.unwrap();
        assert_eq!(related_ids(&prerequisites.ancestors), ["plan"]);
        assert!(prerequisites.unsurfaced_ancestors.is_empty());
        // With conditions evaluated, the terminal ancestor hides it as the tasks row would.
        let value = detail_with::<Error>(&view, &id("late"), |_, _| Ok(true)).unwrap();
        assert_eq!(value.row.status, Status::Unsurfaced);
    }

    #[test]
    fn a_conflicted_ancestor_blocks_its_descendants_and_ends_the_condition_chain() {
        let mut f = Fixture::new();
        f.create("outer", Kind::Group, None, &[], 1);
        f.set_condition("outer", Some("outer"));
        f.create("plan", Kind::Group, Some("outer"), &[], 2);
        f.set_condition("plan", Some("plan"));
        f.create("work", Kind::Issue, Some("plan"), &[], 3);
        // Two sides change the Group differently: withdrawn on one, renamed on the other.
        // The child keeps one head, the parent has two.
        let mut other = Fixture {
            store: f.store.clone(),
            clock: std::cell::Cell::new(500),
        };
        let record = other
            .store
            .write(&id("plan"), Some("renamed".into()), None, other.tick())
            .unwrap()
            .unwrap();
        other.insert(record);
        f.perform("plan", Operation::Withdraw);
        f.store.absorb(&other.store);
        let derived = f.derived();
        assert!(derived.is_conflicted(&id("plan")));
        assert!(derived.is_settled(&id("work")));
        let view = View::new(&f.store, &derived);
        assert_eq!(view.status(&id("work")), Status::Blocked);
        let detail = detail(&view, &id("work")).unwrap();
        assert_eq!(
            related_ids(&detail.prerequisites.unwrap().ancestors),
            ["plan"]
        );
        assert_eq!(
            detail.parent.as_ref().unwrap().row.as_ref().unwrap().status,
            Status::Conflicted
        );
        let mut calls = Vec::new();
        let rows = candidates::<Error>(
            &view,
            CandidateList::Tasks,
            |_| true,
            |entity, _| {
                calls.push(entity.to_string());
                Ok(true)
            },
            None,
        )
        .unwrap();
        // The conflicted Group is no row, its child is Blocked, and no condition above the
        // conflict is evaluated for the child.
        assert_eq!(ids(&rows), ["outer", "work"]);
        assert_eq!(rows[1].status, Status::Blocked);
        assert_eq!(calls, ["outer"]);
    }

    #[test]
    fn a_missing_ancestor_is_named_without_a_row_as_the_reason_its_descendants_wait() {
        let mut f = Fixture::new();
        f.create("outer", Kind::Group, None, &[], 1);
        f.create("plan", Kind::Group, Some("outer"), &[], 2);
        f.create("work", Kind::Issue, Some("plan"), &[], 3);
        // The registration of the outer Group vanishes, as a revert of its file leaves it.
        let mut alone = Fixture::new();
        for (_, entry) in f.store.entries() {
            if entry.entity().as_ref() != "outer" {
                alone.store.insert(entry.clone()).unwrap();
            }
        }
        let derived = alone.derived();
        assert_eq!(
            derived.violations().iter().collect::<Vec<_>>(),
            [&record::Violation {
                entity: id("plan"),
                kind: ViolationKind::UnknownParent
            }]
        );
        let view = View::new(&alone.store, &derived);
        let rows =
            candidates::<Error>(&view, CandidateList::Tasks, |_| true, |_, _| Ok(true), None)
                .unwrap();
        assert_eq!(ids(&rows), ["plan", "work"]);
        assert!(rows.iter().all(|row| row.status == Status::Blocked));
        let evaluated = |name: &str| detail_with::<Error>(&view, &id(name), |_, _| Ok(true));
        for work in [
            detail(&view, &id("work")).unwrap(),
            evaluated("work").unwrap(),
        ] {
            assert_eq!(work.row.status, Status::Blocked);
            let prerequisites = work.prerequisites.unwrap();
            assert_eq!(prerequisites.operation, PrerequisiteOperation::Start);
            assert_eq!(related_ids(&prerequisites.ancestors), ["outer"]);
            assert!(prerequisites.ancestors[0].row.is_none());
        }
        // The Group the missing ancestor contains is stalled for the same reason.
        for plan in [
            detail(&view, &id("plan")).unwrap(),
            evaluated("plan").unwrap(),
        ] {
            assert_eq!(plan.row.status, Status::Blocked);
            let stall = plan.stall.unwrap();
            assert_eq!(related_ids(&stall.unadopted_ancestors), ["outer"]);
            assert!(stall.unadopted_ancestors[0].row.is_none());
            assert!(stall.undecided_ancestors.is_empty());
        }
    }

    #[test]
    fn a_missing_ancestor_is_named_as_a_complete_prerequisite_of_work_in_progress() {
        let mut f = Fixture::new();
        f.create("outer", Kind::Group, None, &[], 1);
        f.create("work", Kind::Issue, Some("outer"), &[], 2);
        f.perform("work", Operation::Start);
        let mut alone = Fixture::new();
        for (_, entry) in f.store.entries() {
            if entry.entity().as_ref() != "outer" {
                alone.store.insert(entry.clone()).unwrap();
            }
        }
        let derived = alone.derived();
        let view = View::new(&alone.store, &derived);
        let work = detail(&view, &id("work")).unwrap();
        let prerequisites = work.prerequisites.unwrap();
        assert_eq!(prerequisites.operation, PrerequisiteOperation::Complete);
        assert_eq!(related_ids(&prerequisites.ancestors), ["outer"]);
        assert!(prerequisites.ancestors[0].row.is_none());
    }

    #[test]
    fn a_conflicted_ancestor_is_the_stall_reason_of_a_group_below_it() {
        let mut f = Fixture::new();
        f.create("outer", Kind::Group, None, &[], 1);
        f.create("plan", Kind::Group, Some("outer"), &[], 2);
        f.create("work", Kind::Issue, Some("plan"), &[], 3);
        // Two sides rename the outer Group differently.
        let mut other = Fixture {
            store: f.store.clone(),
            clock: std::cell::Cell::new(500),
        };
        let record = other
            .store
            .write(&id("outer"), Some("theirs".into()), None, other.tick())
            .unwrap()
            .unwrap();
        other.insert(record);
        let record = f
            .store
            .write(&id("outer"), Some("ours".into()), None, f.tick())
            .unwrap()
            .unwrap();
        f.insert(record);
        f.store.absorb(&other.store);
        let derived = f.derived();
        assert!(derived.is_conflicted(&id("outer")));
        let view = View::new(&f.store, &derived);
        let plan = detail(&view, &id("plan")).unwrap();
        assert_eq!(plan.row.status, Status::Blocked);
        let stall = plan.stall.unwrap();
        assert_eq!(related_ids(&stall.unadopted_ancestors), ["outer"]);
        assert_eq!(
            stall.unadopted_ancestors[0].row.as_ref().unwrap().status,
            Status::Conflicted
        );
        let work = detail(&view, &id("work")).unwrap();
        assert_eq!(
            related_ids(&work.prerequisites.unwrap().ancestors),
            ["outer"]
        );
    }

    #[test]
    fn a_terminal_ancestor_is_named_as_unadopted_and_not_as_unsurfaced() {
        let mut f = Fixture::new();
        f.create("root", Kind::Group, None, &[], 1);
        f.create("done", Kind::Group, Some("root"), &[], 2);
        f.create("last", Kind::Issue, Some("done"), &[], 3);
        // One side adds open work under the Group while the other completes the Group.
        let mut other = Fixture {
            store: f.store.clone(),
            clock: std::cell::Cell::new(500),
        };
        other.create("plan", Kind::Group, Some("done"), &[], 4);
        other.create("work", Kind::Issue, Some("plan"), &[], 5);
        f.perform("last", Operation::Start);
        f.perform("last", Operation::Complete);
        f.perform("done", Operation::Complete);
        f.store.absorb(&other.store);
        let derived = f.derived();
        assert!(derived.violations().contains(&record::Violation {
            entity: id("plan"),
            kind: ViolationKind::OpenUnderTerminal
        }));
        let view = View::new(&f.store, &derived);
        let evaluated = |name: &str| detail_with::<Error>(&view, &id(name), |_, _| Ok(true));
        for plan in [
            detail(&view, &id("plan")).unwrap(),
            evaluated("plan").unwrap(),
        ] {
            assert_eq!(plan.row.status, Status::Blocked);
            let stall = plan.stall.unwrap();
            assert_eq!(related_ids(&stall.unadopted_ancestors), ["done"]);
            assert_eq!(
                stall.unadopted_ancestors[0].row.as_ref().unwrap().status,
                Status::Completed
            );
            assert!(stall.unsurfaced_ancestors.is_empty());
            assert!(stall.own_condition_unsatisfied.is_empty());
            assert!(stall.undecided_ancestors.is_empty());
        }
        for work in [
            detail(&view, &id("work")).unwrap(),
            evaluated("work").unwrap(),
        ] {
            let prerequisites = work.prerequisites.unwrap();
            assert_eq!(prerequisites.operation, PrerequisiteOperation::Start);
            assert_eq!(related_ids(&prerequisites.ancestors), ["done"]);
            assert!(prerequisites.unsurfaced_ancestors.is_empty());
        }
        // An unsatisfied condition above the terminal ancestor is still named, and nothing
        // below the terminal ancestor is evaluated.
        f.set_condition("root", Some("root"));
        f.set_condition("plan", Some("plan"));
        let derived = f.derived();
        let view = View::new(&f.store, &derived);
        let mut asked = Vec::new();
        let plan = detail_with::<Error>(&view, &id("plan"), |e, _| {
            asked.push(e.to_string());
            Ok(false)
        })
        .unwrap();
        assert_eq!(asked, ["root"]);
        let stall = plan.stall.unwrap();
        assert_eq!(related_ids(&stall.unadopted_ancestors), ["done"]);
        assert_eq!(ids(&stall.unsurfaced_ancestors), ["root"]);
        assert!(stall.own_condition_unsatisfied.is_empty());
    }

    #[test]
    fn an_unadopted_ancestor_violation_names_only_the_settled_ancestors_it_counts() {
        let mut f = Fixture::new();
        f.create("outer", Kind::Group, None, &[], 1);
        f.create("mid", Kind::Group, Some("outer"), &[], 2);
        f.create("work", Kind::Issue, Some("mid"), &[], 3);
        // One side withdraws the middle Group while the other starts the work under it; both
        // rename the outer Group differently, so it is conflicted above the violation.
        let mut other = Fixture {
            store: f.store.clone(),
            clock: std::cell::Cell::new(500),
        };
        other.perform("mid", Operation::Withdraw);
        let record = other
            .store
            .write(&id("outer"), Some("theirs".into()), None, other.tick())
            .unwrap()
            .unwrap();
        other.insert(record);
        f.perform("work", Operation::Start);
        let record = f
            .store
            .write(&id("outer"), Some("ours".into()), None, f.tick())
            .unwrap()
            .unwrap();
        f.insert(record);
        f.store.absorb(&other.store);
        let derived = f.derived();
        assert!(derived.is_conflicted(&id("outer")));
        assert_eq!(
            derived.violations().iter().collect::<Vec<_>>(),
            [&record::Violation {
                entity: id("work"),
                kind: ViolationKind::UnadoptedAncestor
            }]
        );
        let view = View::new(&f.store, &derived);
        let work = detail(&view, &id("work")).unwrap();
        let invalid = &work.violations[0];
        assert_eq!(
            invalid
                .related
                .iter()
                .map(|r| r.id.to_string())
                .collect::<Vec<_>>(),
            ["mid"],
            "the conflicted Group above the chain has no part in the violation"
        );
    }

    #[test]
    fn a_containment_cycle_is_reported_and_every_walk_terminates() {
        let mut f = Fixture::new();
        f.create("a", Kind::Group, None, &[], 1);
        f.create("b", Kind::Group, None, &[], 2);
        f.create("leaf", Kind::Issue, Some("b"), &[], 3);
        // Two replicas each move one Group under the other; the merge closes the cycle.
        let mut other = Fixture {
            store: f.store.clone(),
            clock: std::cell::Cell::new(500),
        };
        let record = other
            .store
            .set_parent(&id("a"), Some(id("b")), other.tick())
            .unwrap()
            .unwrap();
        other.insert(record);
        let record = f
            .store
            .set_parent(&id("b"), Some(id("a")), f.tick())
            .unwrap()
            .unwrap();
        f.insert(record);
        f.store.absorb(&other.store);
        let derived = f.derived();
        assert!(derived.conflicted().is_empty());
        for name in ["a", "b"] {
            assert!(
                derived
                    .violations()
                    .iter()
                    .any(|v| { v.entity == id(name) && v.kind == ViolationKind::ContainmentCycle })
            );
        }
        let view = View::new(&f.store, &derived);
        for name in ["a", "b"] {
            let value = detail(&view, &id(name)).unwrap();
            assert!(value.row.invalid, "{name}");
            let cycle = value
                .violations
                .iter()
                .find(|v| v.kind == ViolationKind::ContainmentCycle)
                .unwrap();
            assert!(cycle.related.iter().any(|r| r.id != id(name)));
            assert!(value.descendants.unwrap().entries.len() <= 3);
        }
        let leaf = detail(&view, &id("leaf")).unwrap();
        assert!(!leaf.row.invalid);
        assert_eq!(leaf.row.status, Status::Ready);
        let rows = list(&view, |_, _| true, None);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows.iter().filter(|r| r.invalid).count(), 2);
        let tasks =
            candidates::<Error>(&view, CandidateList::Tasks, |_| true, |_, _| Ok(true), None)
                .unwrap();
        assert_eq!(tasks.len(), 3);
    }
}
