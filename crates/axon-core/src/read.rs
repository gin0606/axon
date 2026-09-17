//! Structured, storage-independent inspection of a snapshot.
use crate::lifecycle::*;
use std::{collections::BTreeMap, ops::Range};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Undecided,
    Ready,
    Blocked,
    InProgressBlocked,
    InProgress,
    Completed,
    Cancelled,
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

pub fn row<'a>(snapshot: &Snapshot, entity: &'a Entity, query: Option<&str>) -> Row<'a> {
    Row {
        entity,
        status: status(snapshot, entity),
        matches: query.map(|q| matches_in(entity, q)).unwrap_or_default(),
    }
}

pub fn status(snapshot: &Snapshot, entity: &Entity) -> Status {
    match entity.current.lifecycle {
        Lifecycle::Undecided => Status::Undecided,
        Lifecycle::NotStarted
            if snapshot
                .check_operation(&entity.id, Operation::Start)
                .is_ok() =>
        {
            Status::Ready
        }
        Lifecycle::NotStarted => Status::Blocked,
        Lifecycle::InProgress
            if entity.current.dependencies.iter().any(|id| {
                snapshot
                    .entity(id)
                    .is_ok_and(|e| e.current.lifecycle != Lifecycle::Completed)
            }) =>
        {
            Status::InProgressBlocked
        }
        Lifecycle::InProgress => Status::InProgress,
        Lifecycle::Completed => Status::Completed,
        Lifecycle::Cancelled => Status::Cancelled,
    }
}

pub fn sorted(mut entities: Vec<&Entity>) -> Vec<&Entity> {
    entities.sort_by(|a, b| (&a.created_at, &a.id).cmp(&(&b.created_at, &b.id)));
    entities
}

pub fn list<'a>(
    snapshot: &'a Snapshot,
    mut include: impl FnMut(&Entity) -> bool,
    query: Option<&str>,
) -> Vec<Row<'a>> {
    sorted(snapshot.entities().filter(|e| include(e)).collect())
        .into_iter()
        .map(|e| row(snapshot, e, query))
        .collect()
}

pub fn candidates<'a, E: From<Error>>(
    snapshot: &'a Snapshot,
    kind: CandidateList,
    include: impl FnMut(&Entity) -> bool,
    evaluate: impl FnMut(&Entity, &str) -> std::result::Result<bool, E>,
    query: Option<&str>,
) -> std::result::Result<Vec<Row<'a>>, E> {
    candidates_filtered(snapshot, kind, include, evaluate).map(|entities| {
        entities
            .into_iter()
            .map(|e| row(snapshot, e, query))
            .collect()
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrerequisiteOperation {
    Start,
    Complete,
}

#[derive(Debug)]
pub struct Prerequisites<'a> {
    pub operation: PrerequisiteOperation,
    pub parent: Option<Row<'a>>,
    pub dependencies: Vec<Row<'a>>,
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
    pub note_count: usize,
    pub parent: Option<&'a Entity>,
    pub prerequisites: Option<Prerequisites<'a>>,
    pub dependencies: Vec<Row<'a>>,
    pub dependents: Vec<Row<'a>>,
    pub descendants: Option<Descendants<'a>>,
}

pub fn detail<'a>(snapshot: &'a Snapshot, id: &EntityId) -> Result<Detail<'a>> {
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
    let prerequisites = if matches!(
        entity.current.lifecycle,
        Lifecycle::NotStarted | Lifecycle::InProgress
    ) {
        let parent_wait = entity.current.lifecycle == Lifecycle::NotStarted
            && parent.is_some_and(|e| e.current.lifecycle != Lifecycle::InProgress);
        let unmet: Vec<_> = dependencies
            .iter()
            .copied()
            .filter(|e| e.current.lifecycle != Lifecycle::Completed)
            .map(|e| row(snapshot, e, None))
            .collect();
        if parent_wait || !unmet.is_empty() {
            Some(Prerequisites {
                operation: if entity.current.lifecycle == Lifecycle::NotStarted {
                    PrerequisiteOperation::Start
                } else {
                    PrerequisiteOperation::Complete
                },
                parent: parent
                    .filter(|_| parent_wait)
                    .map(|e| row(snapshot, e, None)),
                dependencies: unmet,
            })
        } else {
            None
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
    .map(|e| row(snapshot, e, None))
    .collect();
    let descendants = if entity.kind == Kind::Group {
        let mut children_by_parent = snapshot.children_by_parent();
        let children = sorted(children_by_parent.remove(id).unwrap_or_default());
        let count = children.len();
        let mut pending = children
            .into_iter()
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
                let children = sorted(children_by_parent.remove(&child.id).unwrap_or_default());
                let count = children.len();
                pending.extend(
                    children
                        .into_iter()
                        .enumerate()
                        .rev()
                        .map(|(i, child)| (child, ancestors.clone(), i + 1 == count)),
                );
            }
            entries.push(Descendant {
                row: row(snapshot, child, None),
                ancestor_last,
                last,
            });
        }
        Some(Descendants {
            entries,
            completed,
            cancelled,
            awaiting_confirmation: snapshot.check_operation(id, Operation::Complete).is_ok(),
        })
    } else {
        None
    };
    Ok(Detail {
        row: row(snapshot, entity, None),
        note_count: snapshot.notes(id)?.len(),
        parent,
        prerequisites,
        dependencies: dependencies
            .into_iter()
            .map(|e| row(snapshot, e, None))
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
        let value = detail(&snapshot, &id("child")).unwrap();
        assert_eq!(value.row.status, Status::Blocked);
        let prerequisites = value.prerequisites.unwrap();
        assert_eq!(prerequisites.operation, PrerequisiteOperation::Start);
        assert_eq!(prerequisites.parent.unwrap().entity.id, id("parent"));
        assert_eq!(prerequisites.dependencies[0].entity.id, id("dependency"));
        perform(&mut snapshot, "parent", Operation::Start);
        assert!(
            detail(&snapshot, &id("child"))
                .unwrap()
                .prerequisites
                .unwrap()
                .parent
                .is_none()
        );
        perform(&mut snapshot, "dependency", Operation::Start);
        perform(&mut snapshot, "dependency", Operation::Complete);
        assert_eq!(
            status(&snapshot, snapshot.entity(&id("child")).unwrap()),
            Status::Ready
        );
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
        assert_eq!(
            value.prerequisites.unwrap().operation,
            PrerequisiteOperation::Complete
        );
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
        perform(&mut snapshot, "root", Operation::Start);
        perform(&mut snapshot, "first", Operation::Cancel);
        perform(&mut snapshot, "subgroup", Operation::Start);
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
        let rows = list(&snapshot, |_| true, Some("needle"));
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
