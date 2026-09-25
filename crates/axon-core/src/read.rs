//! Structured, storage-independent inspection of a snapshot.
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
    InProgressBlocked,
    InProgress,
    Completed,
    Cancelled,
    /// A NotStarted Group without direct children, whether or not it could complete.
    Empty,
    /// A NotStarted Group with children that completes once its final review passes.
    Confirmable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchLocation {
    Title,
    Description,
}

#[derive(Debug)]
pub struct Row<'a> {
    pub entity: &'a Entity,
    pub status: Status,
    pub matches: Vec<MatchLocation>,
}

pub fn matches_in(entity: &Entity, query: &str) -> Vec<MatchLocation> {
    let mut locations = Vec::new();
    if entity.current.title.contains(query) {
        locations.push(MatchLocation::Title);
    }
    if entity.current.description.contains(query) {
        locations.push(MatchLocation::Description);
    }
    locations
}

/// One snapshot with the derived structure that every row of a read needs: the children of
/// each Group and the Groups whose effective lifecycle is InProgress.
pub struct View<'a> {
    snapshot: &'a Snapshot,
    children: BTreeMap<&'a EntityId, Vec<&'a Entity>>,
    working: BTreeSet<EntityId>,
}

impl<'a> View<'a> {
    pub fn new(snapshot: &'a Snapshot) -> Self {
        let mut children = snapshot.children_by_parent();
        for siblings in children.values_mut() {
            siblings.sort_by(|a, b| (&a.created_at, &a.id).cmp(&(&b.created_at, &b.id)));
        }
        let working = snapshot.working_groups_in(&children);
        Self {
            snapshot,
            children,
            working,
        }
    }
    pub fn snapshot(&self) -> &'a Snapshot {
        self.snapshot
    }
    pub fn effective(&self, entity: &Entity) -> Lifecycle {
        if entity.kind == Kind::Group && self.working.contains(&entity.id) {
            Lifecycle::InProgress
        } else {
            entity.current.lifecycle
        }
    }
    pub fn children(&self, id: &EntityId) -> &[&'a Entity] {
        self.children.get(id).map(Vec::as_slice).unwrap_or_default()
    }
    /// Every Entity below the Group in tree order: siblings by creation time, parents first.
    pub fn descendants(&self, id: &EntityId) -> Vec<&'a Entity> {
        let mut found = Vec::new();
        let mut pending: Vec<_> = self.children(id).iter().rev().copied().collect();
        while let Some(entity) = pending.pop() {
            found.push(entity);
            pending.extend(self.children(&entity.id).iter().rev().copied());
        }
        found
    }
    /// An Issue that satisfies the prerequisites of `Start`.
    pub fn startable(&self, entity: &Entity) -> bool {
        entity.kind == Kind::Issue
            && entity.current.lifecycle == Lifecycle::NotStarted
            && self
                .snapshot
                .check_operation_in(&entity.id, Operation::Start, &self.children)
                .is_ok()
    }
    fn completable(&self, entity: &Entity) -> bool {
        self.snapshot
            .check_operation_in(&entity.id, Operation::Complete, &self.children)
            .is_ok()
    }
    fn unmet_dependencies(&self, entity: &'a Entity) -> Vec<&'a Entity> {
        sorted(
            entity
                .current
                .dependencies
                .iter()
                .filter_map(|id| self.snapshot.entity(id).ok())
                .filter(|e| e.current.lifecycle != Lifecycle::Completed)
                .collect(),
        )
    }
    fn ancestors(&self, entity: &Entity) -> Vec<&'a Entity> {
        self.snapshot.ancestors(entity).unwrap_or_default()
    }
    /// Unmet dependencies of every ancestor, each listed once.
    fn ancestor_dependencies(&self, ancestors: &[&'a Entity]) -> Vec<&'a Entity> {
        let mut found: Vec<&'a Entity> = Vec::new();
        for ancestor in ancestors {
            for dependency in self.unmet_dependencies(ancestor) {
                if !found.iter().any(|e| e.id == dependency.id) {
                    found.push(dependency);
                }
            }
        }
        found
    }
    /// The situation shown without evaluating conditions: a Group is `Ready` when a startable
    /// Issue is below it.
    pub fn status(&self, entity: &Entity) -> Status {
        let Ok(status) = self.status_with(entity, |_| Ok::<_, std::convert::Infallible>(true));
        status
    }
    /// The situation of a `tasks` row: a Group is `Ready` only when a startable Issue below it
    /// is also a candidate, which `candidate` decides from evaluated conditions.
    pub fn status_with<E>(
        &self,
        entity: &Entity,
        candidate: impl FnMut(&Entity) -> std::result::Result<bool, E>,
    ) -> std::result::Result<Status, E> {
        Ok(match (entity.kind, entity.current.lifecycle) {
            (_, Lifecycle::Undecided) => Status::Undecided,
            (_, Lifecycle::Completed) => Status::Completed,
            (_, Lifecycle::Cancelled) => Status::Cancelled,
            (Kind::Issue, Lifecycle::NotStarted) if self.startable(entity) => Status::Ready,
            (Kind::Issue, Lifecycle::NotStarted) => Status::Blocked,
            (_, Lifecycle::InProgress) if !self.unmet_dependencies(entity).is_empty() => {
                Status::InProgressBlocked
            }
            (_, Lifecycle::InProgress) => Status::InProgress,
            (Kind::Group, Lifecycle::NotStarted) => self.group_status(entity, candidate)?,
        })
    }
    /// Empty > Confirmable > Ready > InProgress > Blocked, as the model orders them.
    fn group_status<E>(
        &self,
        group: &Entity,
        mut candidate: impl FnMut(&Entity) -> std::result::Result<bool, E>,
    ) -> std::result::Result<Status, E> {
        if self.children(&group.id).is_empty() {
            return Ok(Status::Empty);
        }
        if self.completable(group) {
            return Ok(Status::Confirmable);
        }
        // Every startable descendant is asked, so a failing condition fails the list
        // regardless of which sibling happens to surface first.
        let mut ready = false;
        for descendant in self.descendants(&group.id) {
            if self.startable(descendant) && candidate(descendant)? {
                ready = true;
            }
        }
        Ok(if ready {
            Status::Ready
        } else if self.working.contains(&group.id) {
            Status::InProgress
        } else {
            Status::Blocked
        })
    }
    pub fn row(&self, entity: &'a Entity, query: Option<&str>) -> Row<'a> {
        Row {
            entity,
            status: self.status(entity),
            matches: query.map(|q| matches_in(entity, q)).unwrap_or_default(),
        }
    }
    /// Why a NotStarted Group is stalled: it cannot complete and no Issue below it is
    /// startable or InProgress. Conditions are not evaluated, so a startable Issue that does
    /// not surface is not a reason here.
    pub fn stall(&self, group: &'a Entity) -> Option<Stall<'a>> {
        if group.kind != Kind::Group || group.current.lifecycle != Lifecycle::NotStarted {
            return None;
        }
        let descendants = self.descendants(&group.id);
        if self.completable(group)
            || descendants.iter().any(|d| {
                self.startable(d)
                    || (d.kind == Kind::Issue && d.current.lifecycle == Lifecycle::InProgress)
            })
        {
            return None;
        }
        let ancestors = self.ancestors(group);
        let ancestor_dependencies = self.ancestor_dependencies(&ancestors);
        let rows = |entities: Vec<&'a Entity>| {
            entities
                .into_iter()
                .map(|e| self.row(e, None))
                .collect::<Vec<_>>()
        };
        Some(Stall {
            dependencies: rows(self.unmet_dependencies(group)),
            ancestor_dependencies: rows(ancestor_dependencies),
            descendant_dependencies: descendants
                .iter()
                .filter(|d| d.current.lifecycle.editable())
                .flat_map(|d| {
                    self.unmet_dependencies(d)
                        .into_iter()
                        .map(|dependency| (self.row(d, None), self.row(dependency, None)))
                })
                .collect(),
            undecided_children: rows(
                self.children(&group.id)
                    .iter()
                    .copied()
                    .filter(|c| c.current.lifecycle == Lifecycle::Undecided)
                    .collect(),
            ),
            open_subgroups: rows(
                self.children(&group.id)
                    .iter()
                    .copied()
                    .filter(|c| c.kind == Kind::Group && c.current.lifecycle.editable())
                    .collect(),
            ),
            undecided_ancestors: rows(
                ancestors
                    .into_iter()
                    .filter(|a| a.current.lifecycle == Lifecycle::Undecided)
                    .collect(),
            ),
        })
    }
}

pub fn status(snapshot: &Snapshot, entity: &Entity) -> Status {
    View::new(snapshot).status(entity)
}

pub fn sorted(mut entities: Vec<&Entity>) -> Vec<&Entity> {
    entities.sort_by(|a, b| (&a.created_at, &a.id).cmp(&(&b.created_at, &b.id)));
    entities
}

pub fn list<'a>(
    snapshot: &'a Snapshot,
    mut include: impl FnMut(&View<'a>, &Entity) -> bool,
    query: Option<&str>,
) -> Vec<Row<'a>> {
    let view = View::new(snapshot);
    sorted(snapshot.entities().filter(|e| include(&view, e)).collect())
        .into_iter()
        .map(|e| view.row(e, query))
        .collect()
}

pub fn candidates<'a, E: From<Error>>(
    snapshot: &'a Snapshot,
    kind: CandidateList,
    include: impl FnMut(&Entity) -> bool,
    evaluate: impl FnMut(&Entity, &str) -> std::result::Result<bool, E>,
    query: Option<&str>,
) -> std::result::Result<Vec<Row<'a>>, E> {
    let mut surfacing = Surfacing::new(snapshot, evaluate);
    let entities = list_candidates(snapshot, kind, include, &mut surfacing)?;
    let view = View::new(snapshot);
    let mut rows = Vec::with_capacity(entities.len());
    for entity in entities {
        let status = match kind {
            CandidateList::Proposals => view.status(entity),
            CandidateList::Tasks => view.status_with(entity, |issue| surfacing.surfaced(issue))?,
        };
        rows.push(Row {
            entity,
            status,
            matches: query.map(|q| matches_in(entity, q)).unwrap_or_default(),
        });
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
    /// Ancestors whose stored lifecycle is not NotStarted.
    pub ancestors: Vec<Row<'a>>,
    pub dependencies: Vec<Row<'a>>,
    pub ancestor_dependencies: Vec<Row<'a>>,
}

/// The reasons a Group is stalled; at least one is present.
#[derive(Debug)]
pub struct Stall<'a> {
    pub dependencies: Vec<Row<'a>>,
    pub ancestor_dependencies: Vec<Row<'a>>,
    /// An unfinished descendant and the dependency it waits for.
    pub descendant_dependencies: Vec<(Row<'a>, Row<'a>)>,
    pub undecided_children: Vec<Row<'a>>,
    pub open_subgroups: Vec<Row<'a>>,
    pub undecided_ancestors: Vec<Row<'a>>,
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

#[derive(Debug)]
pub struct Detail<'a> {
    pub row: Row<'a>,
    pub effective: Lifecycle,
    pub note_count: usize,
    pub parent: Option<&'a Entity>,
    pub prerequisites: Option<Prerequisites<'a>>,
    pub stall: Option<Stall<'a>>,
    pub dependencies: Vec<Row<'a>>,
    pub dependents: Vec<Row<'a>>,
    pub descendants: Option<Descendants<'a>>,
}

pub fn detail<'a>(snapshot: &'a Snapshot, id: &EntityId) -> Result<Detail<'a>> {
    let view = View::new(snapshot);
    let entity = snapshot.entity(id)?;
    let parent = entity
        .current
        .parent
        .as_ref()
        .map(|id| snapshot.entity(id))
        .transpose()?;
    let dependencies = sorted(
        entity
            .current
            .dependencies
            .iter()
            .map(|id| snapshot.entity(id))
            .collect::<Result<Vec<_>>>()?,
    );
    let stall = view.stall(entity);
    let prerequisites = if stall.is_none()
        && matches!(
            entity.current.lifecycle,
            Lifecycle::NotStarted | Lifecycle::InProgress
        ) {
        let starting =
            entity.kind == Kind::Issue && entity.current.lifecycle == Lifecycle::NotStarted;
        let ancestors = view.ancestors(entity);
        let unadopted: Vec<_> = ancestors
            .iter()
            .copied()
            .filter(|a| a.current.lifecycle != Lifecycle::NotStarted)
            .collect();
        let ancestor_dependencies = if starting {
            view.ancestor_dependencies(&ancestors)
        } else {
            Vec::new()
        };
        let unmet = view.unmet_dependencies(entity);
        if unadopted.is_empty() && unmet.is_empty() && ancestor_dependencies.is_empty() {
            None
        } else {
            Some(Prerequisites {
                operation: if starting {
                    PrerequisiteOperation::Start
                } else {
                    PrerequisiteOperation::Complete
                },
                ancestors: unadopted.into_iter().map(|e| view.row(e, None)).collect(),
                dependencies: unmet.into_iter().map(|e| view.row(e, None)).collect(),
                ancestor_dependencies: ancestor_dependencies
                    .into_iter()
                    .map(|e| view.row(e, None))
                    .collect(),
            })
        }
    } else {
        None
    };
    let dependents = sorted(
        snapshot
            .entities()
            .filter(|e| e.current.dependencies.contains(id))
            .collect(),
    )
    .into_iter()
    .map(|e| view.row(e, None))
    .collect();
    let descendants = if entity.kind == Kind::Group {
        let children = view.children(id);
        let count = children.len();
        let mut pending = children
            .iter()
            .copied()
            .enumerate()
            .rev()
            .map(|(i, child)| (child, Vec::new(), i + 1 == count))
            .collect::<Vec<_>>();
        let mut entries = Vec::new();
        let mut completed = 0;
        let mut cancelled = 0;
        while let Some((child, ancestor_last, last)) = pending.pop() {
            completed += usize::from(child.current.lifecycle == Lifecycle::Completed);
            cancelled += usize::from(child.current.lifecycle == Lifecycle::Cancelled);
            if child.kind == Kind::Group {
                let mut ancestors = ancestor_last.clone();
                ancestors.push(last);
                let children = view.children(&child.id);
                let count = children.len();
                pending.extend(
                    children
                        .iter()
                        .copied()
                        .enumerate()
                        .rev()
                        .map(|(i, child)| (child, ancestors.clone(), i + 1 == count)),
                );
            }
            entries.push(Descendant {
                row: view.row(child, None),
                ancestor_last,
                last,
            });
        }
        Some(Descendants {
            entries,
            completed,
            cancelled,
            awaiting_confirmation: view.status(entity) == Status::Confirmable,
        })
    } else {
        None
    };
    Ok(Detail {
        row: view.row(entity, None),
        effective: view.effective(entity),
        note_count: snapshot.notes(id)?.len(),
        parent,
        prerequisites,
        stall,
        dependencies: dependencies
            .into_iter()
            .map(|e| view.row(e, None))
            .collect(),
        dependents,
        descendants,
    })
}

#[derive(Debug)]
pub struct RecordEntry<'a, T> {
    pub record: &'a T,
    pub concurrent_with_previous: bool,
}

pub fn history<'a>(
    snapshot: &'a Snapshot,
    id: &EntityId,
) -> Result<Vec<RecordEntry<'a, StateRecord>>> {
    ordered_records(snapshot, snapshot.history(id)?, |record| &record.id)
}

pub fn notes<'a>(snapshot: &'a Snapshot, id: &EntityId) -> Result<Vec<RecordEntry<'a, Note>>> {
    ordered_records(snapshot, snapshot.notes(id)?, |note| &note.id)
}

fn ordered_records<'a, T>(
    snapshot: &Snapshot,
    records: Vec<&'a T>,
    id: impl Fn(&T) -> &RecordId,
) -> Result<Vec<RecordEntry<'a, T>>> {
    let mut previous = None;
    records
        .into_iter()
        .map(|record| {
            let concurrent_with_previous = previous
                .map(|before| {
                    snapshot
                        .precedes(before, id(record))
                        .map(|ordered| !ordered)
                })
                .transpose()?
                .unwrap_or(false);
            previous = Some(id(record));
            Ok(RecordEntry {
                record,
                concurrent_with_previous,
            })
        })
        .collect()
}

#[derive(Debug)]
pub struct NoteMatch<'a> {
    pub note: &'a Note,
    /// Byte range of the first literal match, on UTF-8 boundaries.
    pub range: Range<usize>,
}

pub fn search_notes<'a>(snapshot: &'a Snapshot, query: &str) -> Result<Vec<NoteMatch<'a>>> {
    if query.is_empty() {
        return Err(Error("Note search query must not be empty".into()));
    }
    let mut grouped = BTreeMap::<_, Vec<_>>::new();
    for note in snapshot.all_notes()? {
        if let Some(position) = note.body.find(query) {
            grouped.entry(&note.entity).or_default().push(NoteMatch {
                note,
                range: position..position + query.len(),
            });
        }
    }
    Ok(sorted(snapshot.entities().collect())
        .into_iter()
        .flat_map(|entity| grouped.remove(&entity.id).unwrap_or_default())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use std::collections::BTreeSet;

    fn id(value: &str) -> EntityId {
        value.to_owned().try_into().unwrap()
    }
    fn context(at: i64) -> Context {
        Context {
            at: Utc.timestamp_opt(at, 0).unwrap(),
            recorder: None,
        }
    }
    fn create(
        snapshot: &mut Snapshot,
        name: &str,
        kind: Kind,
        parent: Option<&str>,
        dependencies: &[&str],
        at: i64,
    ) {
        snapshot
            .create(
                id(name),
                kind,
                Current {
                    title: name.into(),
                    description: "body needle".into(),
                    lifecycle: Lifecycle::NotStarted,
                    condition: None,
                    parent: parent.map(id),
                    dependencies: dependencies.iter().map(|d| id(d)).collect::<BTreeSet<_>>(),
                },
                context(at),
            )
            .unwrap();
    }
    fn perform(snapshot: &mut Snapshot, name: &str, operation: Operation) {
        snapshot
            .perform(&id(name), operation, None, context(20))
            .unwrap();
    }
    fn status_of(snapshot: &Snapshot, name: &str) -> Status {
        status(snapshot, snapshot.entity(&id(name)).unwrap())
    }
    fn ids(rows: &[Row<'_>]) -> Vec<String> {
        rows.iter().map(|r| r.entity.id.to_string()).collect()
    }

    #[test]
    fn statuses_and_immediate_prerequisites_follow_lifecycle_guards() {
        let mut snapshot = Snapshot::empty();
        create(&mut snapshot, "parent", Kind::Group, None, &[], 1);
        create(&mut snapshot, "dependency", Kind::Issue, None, &[], 2);
        create(
            &mut snapshot,
            "child",
            Kind::Issue,
            Some("parent"),
            &["dependency"],
            3,
        );
        perform(&mut snapshot, "parent", Operation::Withdraw);
        let value = detail(&snapshot, &id("child")).unwrap();
        assert_eq!(value.row.status, Status::Blocked);
        let prerequisites = value.prerequisites.unwrap();
        assert_eq!(prerequisites.operation, PrerequisiteOperation::Start);
        assert_eq!(ids(&prerequisites.ancestors), ["parent"]);
        assert_eq!(ids(&prerequisites.dependencies), ["dependency"]);
        assert!(prerequisites.ancestor_dependencies.is_empty());
        perform(&mut snapshot, "parent", Operation::Accept);
        assert!(
            detail(&snapshot, &id("child"))
                .unwrap()
                .prerequisites
                .unwrap()
                .ancestors
                .is_empty()
        );
        perform(&mut snapshot, "dependency", Operation::Start);
        perform(&mut snapshot, "dependency", Operation::Complete);
        assert_eq!(status_of(&snapshot, "child"), Status::Ready);
        assert!(
            detail(&snapshot, &id("child"))
                .unwrap()
                .prerequisites
                .is_none()
        );
        perform(&mut snapshot, "child", Operation::Start);
        assert_eq!(
            detail(&snapshot, &id("child")).unwrap().row.status,
            Status::InProgress
        );
        create(&mut snapshot, "new-dependency", Kind::Issue, None, &[], 4);
        snapshot
            .add_dependency(&id("child"), &id("new-dependency"))
            .unwrap();
        let value = detail(&snapshot, &id("child")).unwrap();
        assert_eq!(value.row.status, Status::InProgressBlocked);
        let prerequisites = value.prerequisites.unwrap();
        assert_eq!(prerequisites.operation, PrerequisiteOperation::Complete);
        assert_eq!(ids(&prerequisites.dependencies), ["new-dependency"]);
        perform(&mut snapshot, "new-dependency", Operation::Cancel);
        assert_eq!(
            detail(&snapshot, &id("child")).unwrap().row.status,
            Status::InProgressBlocked
        );
        assert_eq!(
            detail(&snapshot, &id("new-dependency")).unwrap().row.status,
            Status::Cancelled
        );
        assert_eq!(
            detail(&snapshot, &id("dependency")).unwrap().row.status,
            Status::Completed
        );
        create(&mut snapshot, "proposal", Kind::Issue, None, &[], 5);
        perform(&mut snapshot, "proposal", Operation::Withdraw);
        assert_eq!(
            detail(&snapshot, &id("proposal")).unwrap().row.status,
            Status::Undecided
        );
    }

    #[test]
    fn ancestor_dependencies_are_start_prerequisites_and_the_parent_line_is_deduplicated() {
        let mut snapshot = Snapshot::empty();
        create(&mut snapshot, "outer", Kind::Group, None, &[], 1);
        create(&mut snapshot, "gate", Kind::Issue, None, &[], 2);
        create(
            &mut snapshot,
            "inner",
            Kind::Group,
            Some("outer"),
            &["gate"],
            3,
        );
        create(&mut snapshot, "leaf", Kind::Issue, Some("inner"), &[], 4);
        let value = detail(&snapshot, &id("leaf")).unwrap();
        assert_eq!(value.row.status, Status::Blocked);
        let prerequisites = value.prerequisites.unwrap();
        assert!(prerequisites.ancestors.is_empty() && prerequisites.dependencies.is_empty());
        assert_eq!(ids(&prerequisites.ancestor_dependencies), ["gate"]);
        assert_eq!(status_of(&snapshot, "inner"), Status::Blocked);
        assert_eq!(status_of(&snapshot, "outer"), Status::Blocked);
        perform(&mut snapshot, "gate", Operation::Start);
        perform(&mut snapshot, "gate", Operation::Complete);
        assert_eq!(status_of(&snapshot, "leaf"), Status::Ready);
        assert_eq!(status_of(&snapshot, "inner"), Status::Ready);
        assert_eq!(status_of(&snapshot, "outer"), Status::Ready);
    }

    #[test]
    fn group_statuses_follow_the_priority_order() {
        let mut snapshot = Snapshot::empty();
        create(&mut snapshot, "plan", Kind::Group, None, &[], 1);
        assert_eq!(status_of(&snapshot, "plan"), Status::Empty);
        assert!(
            snapshot
                .check_operation(&id("plan"), Operation::Complete)
                .is_ok()
        );
        assert!(detail(&snapshot, &id("plan")).unwrap().stall.is_none());
        create(&mut snapshot, "first", Kind::Issue, Some("plan"), &[], 2);
        assert_eq!(status_of(&snapshot, "plan"), Status::Ready);
        assert!(detail(&snapshot, &id("plan")).unwrap().stall.is_none());
        perform(&mut snapshot, "first", Operation::Start);
        assert_eq!(status_of(&snapshot, "plan"), Status::InProgress);
        assert_eq!(
            detail(&snapshot, &id("plan")).unwrap().effective,
            Lifecycle::InProgress
        );
        create(&mut snapshot, "second", Kind::Issue, Some("plan"), &[], 3);
        // Ready wins over InProgress while a startable Issue remains.
        assert_eq!(status_of(&snapshot, "plan"), Status::Ready);
        perform(&mut snapshot, "second", Operation::Withdraw);
        assert_eq!(status_of(&snapshot, "plan"), Status::InProgress);
        perform(&mut snapshot, "first", Operation::Complete);
        perform(&mut snapshot, "second", Operation::Cancel);
        assert_eq!(status_of(&snapshot, "plan"), Status::Confirmable);
        assert!(
            detail(&snapshot, &id("plan"))
                .unwrap()
                .descendants
                .unwrap()
                .awaiting_confirmation
        );
        create(&mut snapshot, "third", Kind::Issue, Some("plan"), &[], 4);
        perform(&mut snapshot, "third", Operation::Withdraw);
        assert_eq!(status_of(&snapshot, "plan"), Status::InProgress);
        perform(&mut snapshot, "third", Operation::Cancel);
        perform(&mut snapshot, "plan", Operation::Complete);
        assert_eq!(status_of(&snapshot, "plan"), Status::Completed);
        perform(&mut snapshot, "plan", Operation::Reopen);
        assert_eq!(status_of(&snapshot, "plan"), Status::Confirmable);
        assert_eq!(
            detail(&snapshot, &id("plan")).unwrap().effective,
            Lifecycle::InProgress
        );
        let mut empty = Snapshot::empty();
        create(&mut empty, "blocked", Kind::Group, None, &[], 1);
        create(&mut empty, "draft", Kind::Issue, Some("blocked"), &[], 2);
        perform(&mut empty, "draft", Operation::Withdraw);
        assert_eq!(status_of(&empty, "blocked"), Status::Blocked);
    }

    #[test]
    fn stalled_groups_list_their_reasons_on_blocked_working_and_empty_rows() {
        let mut snapshot = Snapshot::empty();
        create(&mut snapshot, "gate", Kind::Issue, None, &[], 1);
        create(&mut snapshot, "outer", Kind::Group, None, &[], 2);
        perform(&mut snapshot, "outer", Operation::Withdraw);
        create(
            &mut snapshot,
            "plan",
            Kind::Group,
            Some("outer"),
            &["gate"],
            3,
        );
        create(&mut snapshot, "draft", Kind::Issue, Some("plan"), &[], 4);
        perform(&mut snapshot, "draft", Operation::Withdraw);
        create(&mut snapshot, "sub", Kind::Group, Some("plan"), &[], 5);
        create(
            &mut snapshot,
            "leaf",
            Kind::Issue,
            Some("sub"),
            &["gate"],
            6,
        );
        let value = detail(&snapshot, &id("plan")).unwrap();
        assert_eq!(value.row.status, Status::Blocked);
        assert!(value.prerequisites.is_none());
        let stall = value.stall.unwrap();
        assert_eq!(ids(&stall.dependencies), ["gate"]);
        assert!(stall.ancestor_dependencies.is_empty());
        assert_eq!(
            stall
                .descendant_dependencies
                .iter()
                .map(|(d, dep)| (d.entity.id.to_string(), dep.entity.id.to_string()))
                .collect::<Vec<_>>(),
            [("leaf".to_owned(), "gate".to_owned())]
        );
        assert_eq!(ids(&stall.undecided_children), ["draft"]);
        assert_eq!(ids(&stall.open_subgroups), ["sub"]);
        assert_eq!(ids(&stall.undecided_ancestors), ["outer"]);
        let sub = detail(&snapshot, &id("sub")).unwrap().stall.unwrap();
        assert_eq!(ids(&sub.ancestor_dependencies), ["gate"]);
        assert_eq!(ids(&sub.undecided_ancestors), ["outer"]);
        // An empty Group that cannot complete is stalled; one that can is not.
        create(&mut snapshot, "hollow", Kind::Group, Some("outer"), &[], 7);
        let hollow = detail(&snapshot, &id("hollow")).unwrap();
        assert_eq!(hollow.row.status, Status::Empty);
        assert_eq!(ids(&hollow.stall.unwrap().undecided_ancestors), ["outer"]);
        perform(&mut snapshot, "outer", Operation::Accept);
        assert!(detail(&snapshot, &id("hollow")).unwrap().stall.is_none());
        // A startable or InProgress grandchild keeps the Group out of the stalled set.
        perform(&mut snapshot, "gate", Operation::Start);
        perform(&mut snapshot, "gate", Operation::Complete);
        assert_eq!(status_of(&snapshot, "plan"), Status::Ready);
        assert!(detail(&snapshot, &id("plan")).unwrap().stall.is_none());
        perform(&mut snapshot, "leaf", Operation::Start);
        assert_eq!(status_of(&snapshot, "plan"), Status::InProgress);
        assert!(detail(&snapshot, &id("plan")).unwrap().stall.is_none());
        // A working row with completed but no InProgress descendants is stalled too.
        perform(&mut snapshot, "leaf", Operation::Complete);
        let value = detail(&snapshot, &id("plan")).unwrap();
        assert_eq!(value.row.status, Status::InProgress);
        let stall = value.stall.unwrap();
        assert_eq!(ids(&stall.undecided_children), ["draft"]);
        assert_eq!(ids(&stall.open_subgroups), ["sub"]);
        assert!(stall.dependencies.is_empty() && stall.undecided_ancestors.is_empty());
        // A working row whose Issue waits for a dependency added during work is not stalled;
        // the dependency is a completion prerequisite of the Group only when it is its own.
        perform(&mut snapshot, "draft", Operation::Accept);
        perform(&mut snapshot, "draft", Operation::Start);
        create(&mut snapshot, "late", Kind::Issue, None, &[], 8);
        snapshot.add_dependency(&id("plan"), &id("late")).unwrap();
        let value = detail(&snapshot, &id("plan")).unwrap();
        assert_eq!(value.row.status, Status::InProgress);
        assert!(value.stall.is_none());
        let prerequisites = value.prerequisites.unwrap();
        assert_eq!(prerequisites.operation, PrerequisiteOperation::Complete);
        assert_eq!(ids(&prerequisites.dependencies), ["late"]);
    }

    #[test]
    fn confirmable_follows_the_complete_prerequisites_not_only_terminal_children() {
        let mut snapshot = Snapshot::empty();
        create(&mut snapshot, "gate", Kind::Issue, None, &[], 1);
        create(&mut snapshot, "plan", Kind::Group, None, &["gate"], 2);
        create(&mut snapshot, "done", Kind::Issue, Some("plan"), &[], 3);
        perform(&mut snapshot, "gate", Operation::Start);
        perform(&mut snapshot, "gate", Operation::Complete);
        perform(&mut snapshot, "done", Operation::Start);
        perform(&mut snapshot, "done", Operation::Complete);
        perform(&mut snapshot, "gate", Operation::Reopen);
        let value = detail(&snapshot, &id("plan")).unwrap();
        assert_eq!(value.row.status, Status::InProgress);
        assert!(!value.descendants.unwrap().awaiting_confirmation);
        let stall = value.stall.unwrap();
        assert_eq!(ids(&stall.dependencies), ["gate"]);
        assert!(
            stall.ancestor_dependencies.is_empty()
                && stall.descendant_dependencies.is_empty()
                && stall.undecided_children.is_empty()
                && stall.open_subgroups.is_empty()
                && stall.undecided_ancestors.is_empty()
        );
        create(&mut snapshot, "outer", Kind::Group, None, &[], 4);
        perform(&mut snapshot, "outer", Operation::Withdraw);
        create(&mut snapshot, "inner", Kind::Group, Some("outer"), &[], 5);
        create(&mut snapshot, "dropped", Kind::Issue, Some("inner"), &[], 6);
        perform(&mut snapshot, "dropped", Operation::Cancel);
        let value = detail(&snapshot, &id("inner")).unwrap();
        assert_eq!(value.row.status, Status::Blocked);
        assert!(!value.descendants.unwrap().awaiting_confirmation);
        assert_eq!(ids(&value.stall.unwrap().undecided_ancestors), ["outer"]);
    }

    #[test]
    fn tasks_rows_use_evaluated_conditions_for_group_readiness() {
        let mut snapshot = Snapshot::empty();
        create(&mut snapshot, "plan", Kind::Group, None, &[], 1);
        create(&mut snapshot, "hidden", Kind::Issue, Some("plan"), &[], 2);
        snapshot
            .set_condition(&id("hidden"), Some("exit 1".into()))
            .unwrap();
        create(&mut snapshot, "working", Kind::Group, None, &[], 3);
        create(
            &mut snapshot,
            "veiled",
            Kind::Issue,
            Some("working"),
            &[],
            4,
        );
        snapshot
            .set_condition(&id("veiled"), Some("exit 1".into()))
            .unwrap();
        create(&mut snapshot, "done", Kind::Issue, Some("working"), &[], 5);
        perform(&mut snapshot, "done", Operation::Start);
        perform(&mut snapshot, "done", Operation::Complete);
        // Without conditions both Groups have a startable descendant.
        assert_eq!(status_of(&snapshot, "plan"), Status::Ready);
        assert_eq!(status_of(&snapshot, "working"), Status::Ready);
        let mut calls = Vec::new();
        let rows = candidates::<Error>(
            &snapshot,
            CandidateList::Tasks,
            |e| e.kind == Kind::Group,
            |entity, _| {
                calls.push(entity.id.to_string());
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
        create(&mut snapshot, "broken", Kind::Issue, Some("plan"), &[], 6);
        snapshot
            .set_condition(&id("broken"), Some("exit 2".into()))
            .unwrap();
        for filter in [true, false] {
            let error = candidates::<Error>(
                &snapshot,
                CandidateList::Tasks,
                move |e| !filter || e.kind == Kind::Group,
                |entity, command| {
                    if command == "exit 2" {
                        Err(Error(format!("{} failed", entity.id)))
                    } else {
                        Ok(true)
                    }
                },
                None,
            )
            .unwrap_err();
            assert!(error.to_string().contains("broken failed"), "{error}");
        }
        snapshot.set_condition(&id("broken"), None).unwrap();
        snapshot.set_condition(&id("hidden"), None).unwrap();
        snapshot.set_condition(&id("veiled"), None).unwrap();
        let rows = candidates::<Error>(
            &snapshot,
            CandidateList::Tasks,
            |_| true,
            |_, _| Ok(true),
            None,
        )
        .unwrap();
        assert_eq!(
            ids(&rows),
            ["plan", "hidden", "working", "veiled", "broken"]
        );
        assert!(rows.iter().all(|r| r.status == Status::Ready));
    }

    #[test]
    fn descendants_are_preorder_with_sibling_structure_and_terminal_counts() {
        let mut snapshot = Snapshot::empty();
        create(&mut snapshot, "root", Kind::Group, None, &[], 1);
        create(&mut snapshot, "subgroup", Kind::Group, Some("root"), &[], 3);
        create(&mut snapshot, "first", Kind::Issue, Some("root"), &[], 2);
        create(
            &mut snapshot,
            "nested",
            Kind::Issue,
            Some("subgroup"),
            &[],
            4,
        );
        perform(&mut snapshot, "first", Operation::Cancel);
        perform(&mut snapshot, "nested", Operation::Start);
        perform(&mut snapshot, "nested", Operation::Complete);
        let tree = detail(&snapshot, &id("root")).unwrap().descendants.unwrap();
        assert_eq!(
            tree.entries
                .iter()
                .map(|e| e.row.entity.id.to_string())
                .collect::<Vec<_>>(),
            ["first", "subgroup", "nested"]
        );
        assert_eq!(tree.entries[1].row.status, Status::Confirmable);
        assert!(!tree.entries[0].last);
        assert!(tree.entries[1].last);
        assert_eq!(tree.entries[2].ancestor_last, [true]);
        assert_eq!((tree.completed, tree.cancelled), (1, 1));
        assert!(!tree.awaiting_confirmation);
        perform(&mut snapshot, "subgroup", Operation::Complete);
        assert!(
            detail(&snapshot, &id("root"))
                .unwrap()
                .descendants
                .unwrap()
                .awaiting_confirmation
        );
    }

    #[test]
    fn search_and_candidates_return_values_without_formatting() {
        let mut snapshot = Snapshot::empty();
        create(&mut snapshot, "needle", Kind::Issue, None, &[], 2);
        create(&mut snapshot, "earlier", Kind::Issue, None, &[], 1);
        snapshot
            .set_condition(&id("needle"), Some("check".into()))
            .unwrap();
        let rows = list(&snapshot, |_, _| true, Some("needle"));
        assert_eq!(rows[0].entity.id, id("earlier"));
        assert_eq!(rows[0].matches, [MatchLocation::Description]);
        assert_eq!(
            rows[1].matches,
            [MatchLocation::Title, MatchLocation::Description]
        );
        let mut calls = Vec::new();
        let rows = candidates::<Error>(
            &snapshot,
            CandidateList::Tasks,
            |_| true,
            |entity, command| {
                calls.push((entity.id.clone(), command.to_owned()));
                Ok(false)
            },
            None,
        )
        .unwrap();
        assert_eq!(calls, [(id("needle"), "check".to_owned())]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].entity.id, id("earlier"));
        snapshot
            .add_note(&id("needle"), "日本語 needle needle".into(), context(0))
            .unwrap();
        snapshot
            .add_note(&id("earlier"), "needle first".into(), context(99))
            .unwrap();
        let matches = search_notes(&snapshot, "needle").unwrap();
        assert_eq!(matches[0].note.entity, id("earlier"));
        assert_eq!(matches[1].range, 10..16);
        assert!(search_notes(&snapshot, "NEEDLE").unwrap().is_empty());
        assert!(search_notes(&snapshot, "").is_err());
    }

    #[test]
    fn list_filters_see_the_effective_lifecycle_of_groups() {
        let mut snapshot = Snapshot::empty();
        create(&mut snapshot, "plan", Kind::Group, None, &[], 1);
        create(&mut snapshot, "work", Kind::Issue, Some("plan"), &[], 2);
        perform(&mut snapshot, "work", Operation::Start);
        let working = list(
            &snapshot,
            |view, e| view.effective(e) == Lifecycle::InProgress,
            None,
        );
        assert_eq!(ids(&working), ["plan", "work"]);
        assert!(
            list(
                &snapshot,
                |view, e| view.effective(e) == Lifecycle::NotStarted,
                None
            )
            .is_empty()
        );
    }

    #[test]
    fn record_order_preserves_causality_and_exposes_concurrent_branches() {
        let mut base = Snapshot::empty();
        create(&mut base, "item", Kind::Issue, None, &[], 10);
        base.add_note(&id("item"), "base".into(), context(40))
            .unwrap();
        let mut left = base.clone();
        let mut right = base;
        left.add_note(&id("item"), "left".into(), context(1))
            .unwrap();
        right
            .add_note(&id("item"), "right".into(), context(50))
            .unwrap();
        perform(&mut left, "item", Operation::Start);
        perform(&mut right, "item", Operation::Cancel);
        let merged = left
            .integrate(
                &right,
                &BTreeMap::from([(id("item"), Side::Left)]),
                None,
                context(0),
            )
            .unwrap();
        let notes = notes(&merged, &id("item")).unwrap();
        assert_eq!(notes[0].record.body, "base");
        assert!(!notes[0].concurrent_with_previous);
        assert_eq!(
            notes.iter().filter(|n| n.concurrent_with_previous).count(),
            1
        );
        let history = history(&merged, &id("item")).unwrap();
        assert!(matches!(
            history[0].record.event,
            StateEvent::Created { .. }
        ));
        assert_eq!(
            history
                .iter()
                .filter(|r| r.concurrent_with_previous)
                .count(),
            1
        );
        assert!(matches!(
            history.last().unwrap().record.event,
            StateEvent::Integration { .. }
        ));
    }
}
