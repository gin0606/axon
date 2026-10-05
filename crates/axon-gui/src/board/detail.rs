//! What the detail pane shows of one Entity, owned so it outlives the read it came from.

use super::progress::{self, Step};
use super::{Board, Change, Edit, NewEntity, Rejection, State};
use axon::lifecycle::{
    EntityId, Kind, Label, Lifecycle,
    record::{Current, RecordKind, Recorder, ViolationKind},
};
use axon::read::{self, PrerequisiteOperation, Related};
use chrono::{DateTime, Utc};

/// Another Entity named by the detail. `title`, `kind` and `state` are absent when the store
/// does not hold it, so it cannot be opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub id: EntityId,
    pub known: Option<Known>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Known {
    pub kind: Kind,
    pub title: String,
    pub state: State,
}

/// Why the Entity waits, from the core's prerequisites of its next operation and, for a
/// stalled Group, the reasons it is stalled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaitKind {
    /// An ancestor that is not adopted keeps the Entity from starting or completing.
    UnadoptedAncestor,
    /// A dependency that is not Completed.
    Dependency,
    /// A dependency of an ancestor that is not Completed.
    AncestorDependency,
    /// An ancestor whose condition is unsatisfied.
    UnsurfacedAncestor,
    /// A descendant (`entity`) waits for a dependency (`via`).
    DescendantDependency,
    /// An Undecided child of a stalled Group.
    UndecidedChild,
    /// An unfinished sub-Group with nothing to work on.
    OpenSubgroup,
    /// A startable Issue below the Group whose condition is unsatisfied.
    UnsurfacedCandidate,
    /// The Group's own condition is unsatisfied.
    OwnCondition,
    /// An Undecided ancestor.
    UndecidedAncestor,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wait {
    pub kind: WaitKind,
    pub entity: Link,
    /// For [`WaitKind::DescendantDependency`], the dependency the descendant waits for.
    pub via: Option<Link>,
}

/// One difference between a record's value and the value before it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Difference {
    Lifecycle(Lifecycle, Lifecycle),
    Kind(Kind, Kind),
    Title(String, String),
    Description,
    Label(Label, Label),
    Parent(Option<EntityId>, Option<EntityId>),
    DependencyAdded(EntityId),
    DependencyRemoved(EntityId),
    Condition,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryEntry {
    pub kind: RecordKind,
    pub at: DateTime<Utc>,
    pub actor: Option<String>,
    pub reason: Option<String>,
    /// The changes from the value before; empty for the creation record and when the value
    /// before is not known (a resolve record, a missing parent).
    pub changes: Vec<Difference>,
    /// The record is not ordered after the previous one: they were made concurrently.
    pub concurrent_with_previous: bool,
    pub parent_missing: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoteEntry {
    pub id: String,
    pub at: DateTime<Utc>,
    pub actor: Option<String>,
    pub reason: Option<String>,
    pub body: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Descendants {
    pub total: usize,
    pub completed: usize,
    pub cancelled: usize,
    pub awaiting_confirmation: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntityDetail {
    pub id: EntityId,
    pub kind: Kind,
    pub label: Label,
    pub title: String,
    pub description: String,
    pub state: State,
    pub status: read::Status,
    /// The stored lifecycle; none while conflicted. Differs from `state` for a working Group.
    pub stored: Option<Lifecycle>,
    /// The saved condition command. It is shown, never run.
    pub condition: Option<String>,
    /// Ancestors from the root down to the parent, as far as the store holds them.
    pub ancestors: Vec<Link>,
    pub parent: Option<Link>,
    /// Direct children in creation order: the Entities whose shown value names this one as
    /// their parent, as the tree nests them.
    pub children: Vec<Link>,
    pub descendants: Option<Descendants>,
    pub dependencies: Vec<Link>,
    pub dependents: Vec<Link>,
    /// The operation `waits` are prerequisites of; none for a stalled Group.
    pub waiting_for: Option<PrerequisiteOperation>,
    pub waits: Vec<Wait>,
    pub stalled: bool,
    pub violations: Vec<(ViolationKind, Vec<Link>)>,
    /// The number of heads of a conflicted Entity; 0 when settled.
    pub heads: usize,
    pub notes: Vec<NoteEntry>,
    pub history: Vec<HistoryEntry>,
    /// The lifecycle transitions the state menu offers; none while conflicted.
    pub progress: Vec<Step>,
    pub structure: Structure,
    /// Whether the core accepts editing the title, description and label on this read: it
    /// refuses a terminal Entity, and every Entity while any Entity of the store is
    /// conflicted. Notes are added in any state.
    pub editable: Result<(), Rejection>,
    /// For a Group, whether the core accepts creating an Entity inside it on this read.
    pub create_inside: Option<Result<(), Rejection>>,
}

/// Whether the core accepts, on this read, each structural change the detail offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Structure {
    /// Taking the Entity out of its Group; none without a parent.
    pub detach: Option<Result<(), Rejection>>,
    /// Converting to `convert.0`, the other kind.
    pub convert: (Kind, Result<(), Rejection>),
    /// Removing each of `dependencies`, in their order.
    pub removals: Vec<Result<(), Rejection>>,
    /// The Entity or another one is conflicted, so the core refuses every structural change
    /// until it is resolved.
    pub conflicted: bool,
}

pub(super) fn of(board: &Board, id: &EntityId) -> axon::lifecycle::Result<EntityDetail> {
    let view = board.read();
    let detail = read::detail(&view, id)?;
    let link = |id: &EntityId| board.link(id);
    let related = |related: &Related<'_>| link(&related.id);
    let id = detail.row.id.clone();
    let presented = view.presented(&id).expect("a detailed Entity is known");

    let mut waits = Vec::new();
    let mut push = |kind, list: &[Related<'_>]| {
        waits.extend(list.iter().map(|r| Wait {
            kind,
            entity: related(r),
            via: None,
        }))
    };
    if let Some(p) = &detail.prerequisites {
        push(WaitKind::UnadoptedAncestor, &p.ancestors);
        push(WaitKind::Dependency, &p.dependencies);
        push(WaitKind::AncestorDependency, &p.ancestor_dependencies);
    }
    if let Some(s) = &detail.stall {
        push(WaitKind::Dependency, &s.dependencies);
        push(WaitKind::AncestorDependency, &s.ancestor_dependencies);
        push(WaitKind::UnadoptedAncestor, &s.unadopted_ancestors);
    }
    let rows = |kind, rows: &[read::Row<'_>]| -> Vec<Wait> {
        rows.iter()
            .map(|row| Wait {
                kind,
                entity: link(row.id),
                via: None,
            })
            .collect()
    };
    if let Some(p) = &detail.prerequisites {
        waits.extend(rows(WaitKind::UnsurfacedAncestor, &p.unsurfaced_ancestors));
    }
    if let Some(s) = &detail.stall {
        waits.extend(
            s.descendant_dependencies
                .iter()
                .map(|(row, dependency)| Wait {
                    kind: WaitKind::DescendantDependency,
                    entity: link(row.id),
                    via: Some(related(dependency)),
                }),
        );
        waits.extend(rows(WaitKind::UndecidedChild, &s.undecided_children));
        waits.extend(rows(WaitKind::OpenSubgroup, &s.open_subgroups));
        waits.extend(rows(
            WaitKind::UnsurfacedCandidate,
            &s.unsurfaced_candidates,
        ));
        waits.extend(rows(WaitKind::OwnCondition, &s.own_condition_unsatisfied));
        waits.extend(rows(WaitKind::UndecidedAncestor, &s.undecided_ancestors));
        waits.extend(rows(WaitKind::UnsurfacedAncestor, &s.unsurfaced_ancestors));
    }

    let mut ancestors = Vec::new();
    let mut next = presented.parent.clone();
    while let Some(parent) = next {
        if parent == id || ancestors.iter().any(|a: &Link| a.id == parent) {
            break;
        }
        next = view.presented(&parent).and_then(|c| c.parent.clone());
        ancestors.push(link(&parent));
    }
    ancestors.reverse();

    let history = read::history(board.records(), &id)?
        .into_iter()
        .map(|entry| HistoryEntry {
            kind: entry.record.kind.clone(),
            at: entry.record.at,
            actor: actor(&entry.record.recorder),
            reason: entry.record.reason.clone(),
            changes: match (&entry.record.kind, entry.before) {
                (RecordKind::Created, _) | (_, None) => Vec::new(),
                (_, Some(before)) => changes(before, &entry.record.after),
            },
            concurrent_with_previous: entry.concurrent_with_previous,
            parent_missing: entry.parent_missing,
        })
        .collect();
    let notes = read::notes(board.records(), &id)
        .into_iter()
        .map(|(note_id, note)| NoteEntry {
            id: note_id.to_string(),
            at: note.at,
            actor: actor(&note.recorder),
            reason: note.reason.clone(),
            body: note.body.clone(),
        })
        .collect();

    let other = match detail.row.kind {
        Kind::Issue => Kind::Group,
        Kind::Group => Kind::Issue,
    };
    let check = |change: Change| change.check(board);
    let editable = check(Change::Edit {
        entity: id.clone(),
        edit: Edit::default(),
    });
    let create_inside = (detail.row.kind == Kind::Group).then(|| board.check_create_inside(&id));
    let structure = Structure {
        conflicted: !view.derived().conflicted().is_empty(),
        detach: presented.parent.as_ref().map(|_| {
            check(Change::Move {
                entity: id.clone(),
                parent: None,
            })
        }),
        convert: (
            other,
            check(Change::Convert {
                entity: id.clone(),
                kind: other,
            }),
        ),
        removals: detail
            .dependencies
            .iter()
            .map(|dependency| {
                check(Change::RemoveDependency {
                    entity: id.clone(),
                    target: dependency.id.clone(),
                })
            })
            .collect(),
    };

    Ok(EntityDetail {
        kind: detail.row.kind,
        label: detail.row.label,
        title: detail.row.title.to_owned(),
        description: detail.description.to_owned(),
        state: State::of(detail.effective),
        status: detail.row.status,
        stored: detail.stored,
        condition: detail.condition.map(str::to_owned),
        ancestors,
        parent: detail.parent.as_ref().map(related),
        children: board
            .items()
            .iter()
            .filter(|item| item.parent.as_ref() == Some(&id))
            .map(|item| link(&item.id))
            .collect(),
        descendants: detail.descendants.as_ref().map(|d| Descendants {
            total: d.entries.len(),
            completed: d.completed,
            cancelled: d.cancelled,
            awaiting_confirmation: d.awaiting_confirmation,
        }),
        dependencies: detail.dependencies.iter().map(related).collect(),
        dependents: detail.dependents.iter().map(|row| link(row.id)).collect(),
        waiting_for: detail.prerequisites.as_ref().map(|p| p.operation),
        stalled: detail.stall.is_some(),
        waits,
        violations: detail
            .violations
            .iter()
            .map(|v| (v.kind, v.related.iter().map(related).collect()))
            .collect(),
        heads: detail.heads.len(),
        notes,
        history,
        progress: progress::steps(board, &id),
        structure,
        editable,
        create_inside,
        id,
    })
}

impl Board {
    /// Whether the core accepts creating an Entity inside `group`, whatever it is: checked
    /// with a placeholder value and an ID no Entity has.
    pub fn check_create_inside(&self, group: &EntityId) -> Result<(), Rejection> {
        let mut serial = 0;
        let entity = self
            .fresh_id(|prefix| {
                serial += 1;
                EntityId::try_from(format!("{prefix}-{serial}"))
            })
            .map_err(Rejection::from)?;
        Change::Create {
            entity,
            value: NewEntity {
                kind: Kind::Issue,
                lifecycle: Lifecycle::Undecided,
                title: "-".into(),
                description: String::new(),
                label: Label::Feat,
                parent: Some(group.clone()),
            },
        }
        .check(self)
    }

    /// A link to `id`, with what the store holds of it.
    pub fn link(&self, id: &EntityId) -> Link {
        Link {
            id: id.clone(),
            known: self.item(id).map(|item| Known {
                kind: item.kind,
                title: item.title.clone(),
                state: item.state,
            }),
        }
    }
}

fn actor(recorder: &Option<Recorder>) -> Option<String> {
    recorder.as_ref().map(|r| r.actor.clone())
}

fn changes(before: &Current, after: &Current) -> Vec<Difference> {
    let mut changes = Vec::new();
    if before.lifecycle != after.lifecycle {
        changes.push(Difference::Lifecycle(before.lifecycle, after.lifecycle));
    }
    if before.kind != after.kind {
        changes.push(Difference::Kind(before.kind, after.kind));
    }
    if before.title != after.title {
        changes.push(Difference::Title(before.title.clone(), after.title.clone()));
    }
    if before.description != after.description {
        changes.push(Difference::Description);
    }
    if before.label != after.label {
        changes.push(Difference::Label(before.label, after.label));
    }
    if before.parent != after.parent {
        changes.push(Difference::Parent(
            before.parent.clone(),
            after.parent.clone(),
        ));
    }
    changes.extend(
        after
            .needs
            .difference(&before.needs)
            .cloned()
            .map(Difference::DependencyAdded),
    );
    changes.extend(
        before
            .needs
            .difference(&after.needs)
            .cloned()
            .map(Difference::DependencyRemoved),
    );
    if before.condition != after.condition {
        changes.push(Difference::Condition);
    }
    changes
}
