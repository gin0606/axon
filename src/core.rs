use crate::derived::{Evaluation, EvaluationError, View};
use crate::domain::*;
use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::rc::Rc;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Evaluation(#[from] EvaluationError),
    #[error("entity {0} does not exist")]
    NoSuchEntity(String),
    #[error("{0} is not a group")]
    NotGroup(String),
    #[error("{id} cannot be started ({fact})")]
    CannotStart { id: String, fact: String },
    #[error("{id} cannot be {action} ({fact})")]
    CannotProgress {
        id: String,
        action: &'static str,
        fact: String,
    },
    #[error("{id}: {field} is already {current}")]
    Unchanged {
        id: String,
        field: &'static str,
        current: String,
    },
    #[error("invalid containment: {0}")]
    Containment(String),
    #[error("cycle would be created in the {projection} wait graph: {path}")]
    Cycle {
        projection: &'static str,
        path: String,
    },
    #[error("invalid import: {0}")]
    InvalidImport(String),
    #[error("a Note body must contain non-whitespace text")]
    EmptyNote,
    #[error("the plan declaration of {0} is fixed by its Disposition")]
    DeclarationFixed(String),
    #[error("invalid state: {0}")]
    InvalidState(String),
}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone)]
pub struct StoreSnapshot {
    pub evaluation: Rc<Evaluation>,
    pub entities: Vec<Entity>,
    pub dependencies: Vec<(EntityId, EntityId)>,
}

impl StoreSnapshot {
    pub fn view(&self) -> View {
        View::with_evaluation(
            self.entities.clone(),
            self.dependencies.clone(),
            self.evaluation.clone(),
        )
    }
}

pub struct Ctx {
    pub actor: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyOutcome {
    Changed,
    Unchanged,
}

#[derive(Debug, Clone)]
pub enum Change {
    Start(Claim),
    Done,
    Release,
    Decide(Disposition),
    SetResurfaceCondition(ResurfaceCondition),
    SetTitle(String),
    SetDescription(Option<String>),
    SetParent(Option<EntityId>),
}

fn unchanged_transition(entity: &Entity, change: &Change) -> Option<(&'static str, String)> {
    match change {
        Change::Start(_) | Change::Done | Change::Release => None,
        Change::Decide(value) if *value == entity.disposition => {
            Some(("Disposition", value.label().to_string()))
        }
        Change::SetResurfaceCondition(value) if value == &entity.resurface_condition => {
            Some(("Resurface condition", value.label()))
        }
        _ => None,
    }
}

fn unchanged_setting(entity: &Entity, change: &Change) -> bool {
    match change {
        Change::SetTitle(value) => value == &entity.title,
        Change::SetDescription(value) => value == &entity.description,
        Change::SetParent(value) => value == &entity.parent,
        _ => false,
    }
}

fn decision_event(
    entity: &Entity,
    change: &Change,
) -> Option<(&'static str, Option<String>, Option<String>)> {
    match change {
        Change::Decide(value) => Some((
            "disposition",
            Some(entity.disposition.as_db().to_string()),
            Some(value.as_db().to_string()),
        )),
        Change::SetResurfaceCondition(value) => Some((
            "resurface_condition",
            (!matches!(entity.resurface_condition, ResurfaceCondition::Always))
                .then(|| entity.resurface_condition.label()),
            (!matches!(value, ResurfaceCondition::Always)).then(|| value.label()),
        )),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub id: RecordId,
    pub field: String,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
    pub revision: Option<RecordId>,
    pub actor: String,
    pub reason: Option<String>,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressEventKind {
    Start,
    Done,
    Release,
}

fn progress_event_kind(change: &Change) -> Option<ProgressEventKind> {
    match change {
        Change::Start(_) => Some(ProgressEventKind::Start),
        Change::Done => Some(ProgressEventKind::Done),
        Change::Release => Some(ProgressEventKind::Release),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressEvent {
    pub id: RecordId,
    pub kind: ProgressEventKind,
    pub actor: String,
    pub reason: Option<String>,
    pub at: DateTime<Utc>,
}

fn cannot_progress(id: &EntityId, action: &'static str, entity: &Entity) -> Error {
    Error::CannotProgress {
        id: id.to_string(),
        action,
        fact: format!("Progress is {}", entity.progress.label()),
    }
}

fn validate_structure(view: &View) -> Result<()> {
    let mut edges = HashSet::new();
    for (source, target) in view.dependencies() {
        for id in [source, target] {
            if view.get(id).is_none() {
                return Err(Error::NoSuchEntity(id.to_string()));
            }
        }
        if source == target || !edges.insert((source, target)) {
            return Err(Error::InvalidState(format!(
                "invalid dependency {source} -> {target}"
            )));
        }
    }
    for entity in view.iter() {
        if let ResurfaceCondition::AfterEntity(target) = &entity.resurface_condition
            && view.get(target).is_none()
        {
            return Err(Error::NoSuchEntity(target.to_string()));
        }
        if entity.parent.as_ref() == Some(&entity.id) {
            return Err(Error::Containment(format!(
                "{} cannot parent itself",
                entity.id
            )));
        }
    }
    for entity in view.iter() {
        if let Some(parent) = &entity.parent {
            let Some(parent) = view.get(parent) else {
                return Err(Error::Containment(format!(
                    "parent {} of {} does not exist",
                    parent, entity.id
                )));
            };
            if parent.kind != EntityKind::Group {
                return Err(Error::NotGroup(parent.id.to_string()));
            }
        }
    }
    for group in view.iter().filter(|entity| {
        entity.kind == EntityKind::Group && matches!(entity.progress, Progress::NotStarted)
    }) {
        if let Some(active) = view
            .descendants(&group.id)
            .into_iter()
            .find(|entity| matches!(entity.progress, Progress::InProgress(_)))
        {
            return Err(Error::Containment(format!(
                "InProgress entity {} cannot be below NotStarted group {}",
                active.id, group.id
            )));
        }
    }
    Ok(())
}

#[derive(Clone)]
struct GraphEdge {
    from: EntityId,
    to: EntityId,
}

fn validate_relations(view: &View) -> Result<()> {
    let mut logical = Vec::new();
    for (source, target) in view.dependencies() {
        expand_source(view, source, target, &mut logical);
    }
    for entity in view.iter() {
        if let ResurfaceCondition::AfterEntity(target) = &entity.resurface_condition {
            expand_source(view, &entity.id, target, &mut logical);
        }
    }

    let mut activation = logical.clone();
    let mut completion = logical;
    for entity in view.iter() {
        if let Some(parent) = &entity.parent {
            activation.push(GraphEdge {
                from: entity.id.clone(),
                to: parent.clone(),
            });
            completion.push(GraphEdge {
                from: parent.clone(),
                to: entity.id.clone(),
            });
        }
    }
    validate_acyclic(view, &activation, "activation")?;
    validate_acyclic(view, &completion, "completion")
}

fn expand_source(view: &View, source: &EntityId, target: &EntityId, edges: &mut Vec<GraphEdge>) {
    edges.push(GraphEdge {
        from: source.clone(),
        to: target.clone(),
    });
    if view
        .get(source)
        .is_some_and(|entity| entity.kind == EntityKind::Group)
    {
        edges.extend(
            view.descendants(source)
                .into_iter()
                .map(|entity| GraphEdge {
                    from: entity.id.clone(),
                    to: target.clone(),
                }),
        );
    }
}

fn validate_acyclic(view: &View, edges: &[GraphEdge], projection: &'static str) -> Result<()> {
    let live: Vec<_> = edges
        .iter()
        .filter(|edge| {
            [view.get(&edge.from), view.get(&edge.to)]
                .into_iter()
                .flatten()
                .all(|entity| !matches!(entity.progress, Progress::Ended))
        })
        .cloned()
        .collect();
    for entity in view
        .iter()
        .filter(|entity| !matches!(entity.progress, Progress::Ended))
    {
        if let Some(path) = path_between(&live, &entity.id, &entity.id, true) {
            let rendered = path
                .into_iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
                .join(" -> ");
            return Err(Error::Cycle {
                projection,
                path: rendered,
            });
        }
    }
    Ok(())
}

fn path_between(
    edges: &[GraphEdge],
    start: &EntityId,
    goal: &EntityId,
    require_edge: bool,
) -> Option<Vec<EntityId>> {
    let mut queue = VecDeque::new();
    let mut previous: HashMap<EntityId, EntityId> = HashMap::new();
    let mut seen = HashSet::new();
    queue.push_back(start.clone());
    while let Some(current) = queue.pop_front() {
        for edge in edges.iter().filter(|edge| edge.from == current) {
            if &edge.to == goal && (require_edge || current != *start) {
                let mut path = vec![goal.clone(), current.clone()];
                let mut cursor = current;
                while cursor != *start {
                    cursor = previous.get(&cursor)?.clone();
                    path.push(cursor.clone());
                }
                path.reverse();
                return Some(path);
            }
            if seen.insert(edge.to.clone()) {
                previous.insert(edge.to.clone(), current.clone());
                queue.push_back(edge.to.clone());
            }
        }
    }
    None
}

fn not_ready_fact(view: &View, entity: &Entity) -> Result<String> {
    Ok(if !matches!(entity.progress, Progress::NotStarted) {
        format!("Progress is {}", entity.progress.label())
    } else if entity.disposition != Disposition::Accepted {
        format!("Disposition is {}", entity.disposition.label())
    } else if !view.is_surfaced(entity)? {
        "resurface condition is not satisfied".to_string()
    } else if !view.within_active_scope(&entity.id)? {
        "outside active scope".to_string()
    } else if view.is_orphaned(&entity.id) {
        "a dependency is Rejected".to_string()
    } else if view.is_blocked(&entity.id) {
        "an unresolved dependency exists".to_string()
    } else {
        "not ready".to_string()
    })
}

pub fn validate_import_snapshot(before: &StoreSnapshot, desired: &StoreSnapshot) -> Result<()> {
    let before_view = before.view();
    let desired_view = desired.view();
    let desired_ids = desired
        .entities
        .iter()
        .map(|entity| entity.id.clone())
        .collect::<HashSet<_>>();
    if desired_ids.len() != desired.entities.len() {
        return Err(Error::InvalidImport(
            "the final snapshot contains duplicate Entity IDs".to_string(),
        ));
    }
    if let Some(missing) = before
        .entities
        .iter()
        .find(|entity| !desired_ids.contains(&entity.id))
    {
        return Err(Error::InvalidImport(format!(
            "the final snapshot removes {}",
            missing.id
        )));
    }

    for after in &desired.entities {
        let Some(current) = before_view.get(&after.id) else {
            if after.progress != Progress::NotStarted
                || after.disposition != Disposition::Accepted
                || after.resurface_condition != ResurfaceCondition::Always
            {
                return Err(Error::InvalidImport(format!(
                    "new Entity {} does not have the required initial state",
                    after.id
                )));
            }
            continue;
        };
        if current.kind != after.kind
            || current.progress != after.progress
            || current.disposition != after.disposition
            || current.current_revision != after.current_revision
            || current.resurface_condition != after.resurface_condition
        {
            return Err(Error::InvalidImport(format!(
                "read-only state changed for {}",
                after.id
            )));
        }
        let declaration_changed = current.title != after.title
            || current.description != after.description
            || current.parent != after.parent
            || direct_dependency_ids(&before_view, &current.id)
                != direct_dependency_ids(&desired_view, &after.id);
        if declaration_changed && current.disposition != Disposition::Undecided {
            return Err(Error::DeclarationFixed(after.id.to_string()));
        }
        if current.kind == EntityKind::Group
            && matches!(current.progress, Progress::Ended)
            && (current.parent != after.parent
                || direct_dependency_ids(&before_view, &current.id)
                    != direct_dependency_ids(&desired_view, &after.id))
        {
            return Err(Error::Containment(format!(
                "Ended group {} cannot change its parent or dependencies",
                after.id
            )));
        }
        if current.parent != after.parent
            && before_view
                .ancestors(&current.id)
                .into_iter()
                .any(|ancestor| matches!(ancestor.progress, Progress::Ended))
        {
            return Err(Error::Containment(format!(
                "{} cannot move within an Ended group",
                after.id
            )));
        }
    }

    for after in &desired.entities {
        let parent_changed = before_view
            .get(&after.id)
            .is_none_or(|before| before.parent != after.parent);
        if parent_changed
            && desired_view
                .ancestors(&after.id)
                .into_iter()
                .any(|ancestor| matches!(ancestor.progress, Progress::Ended))
        {
            return Err(Error::Containment(format!(
                "cannot add {} below an Ended group",
                after.id
            )));
        }
    }
    validate_structure(&desired_view)?;
    validate_relations(&desired_view)?;
    Ok(())
}

fn direct_dependency_ids(view: &View, id: &EntityId) -> Vec<EntityId> {
    let mut ids = view
        .dependencies()
        .iter()
        .filter(|(source, _)| source == id)
        .map(|(_, target)| target.clone())
        .collect::<Vec<_>>();
    ids.sort();
    ids
}

/// Complete logical state. Each history vector is in save order, never timestamp order.
#[derive(Clone)]
pub struct StateSnapshot {
    pub declaration: StoreSnapshot,
    pub metadata: BTreeMap<String, MetadataValue>,
    pub histories: BTreeMap<EntityId, History>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MetadataValue {
    Text(String),
    Integer(i64),
    Real(f64),
    Bytes(Vec<u8>),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct History {
    pub revisions: Vec<DeclarationRevision>,
    pub notes: Vec<Note>,
    pub decisions: Vec<Event>,
    pub progress: Vec<ProgressEvent>,
}

/// The caller supplies nondeterminism; Start carries the explicit worktree claim.
pub struct Context<'a> {
    pub at: DateTime<Utc>,
    pub evaluation: Rc<Evaluation>,
    pub actor: &'a str,
    pub reason: Option<&'a str>,
    pub ids: &'a mut dyn FnMut(RecordKind) -> RecordId,
}

pub enum Operation {
    Insert(Entity),
    Change(EntityId, Change),
    Dependency {
        source: EntityId,
        target: EntityId,
        present: bool,
    },
    AddNote {
        owner: EntityId,
        body: String,
    },
    Import(StoreSnapshot),
}

/// Only the core can construct a validated result. A failed operation leaves its input intact.
pub struct ValidatedChange {
    state: StateSnapshot,
    pub outcome: ApplyOutcome,
}
impl ValidatedChange {
    pub fn state(&self) -> &StateSnapshot {
        &self.state
    }
}

impl StateSnapshot {
    fn entity(&self, id: &EntityId) -> Result<&Entity> {
        self.declaration
            .entities
            .iter()
            .find(|e| e.id == *id)
            .ok_or_else(|| Error::NoSuchEntity(id.to_string()))
    }
    fn entity_mut(&mut self, id: &EntityId) -> &mut Entity {
        self.declaration
            .entities
            .iter_mut()
            .find(|e| e.id == *id)
            .expect("validated owner")
    }
    fn view(&self) -> View {
        self.declaration.view()
    }
    fn parent_target(&self, parent: Option<&EntityId>) -> Result<()> {
        if let Some(parent) = parent
            && self.entity(parent)?.kind != EntityKind::Group
        {
            return Err(Error::NotGroup(parent.to_string()));
        }
        Ok(())
    }
    fn record_id(&self, kind: RecordKind, ctx: &mut Context<'_>) -> Result<RecordId> {
        let id = (ctx.ids)(kind);
        let exists = self.histories.values().any(|h| {
            h.revisions.iter().any(|r| r.id == id)
                || h.notes.iter().any(|r| r.id == id)
                || h.decisions.iter().any(|r| r.id == id)
                || h.progress.iter().any(|r| r.id == id)
        });
        if id.kind() != kind || exists {
            return Err(Error::InvalidState(format!(
                "invalid or duplicate generated ID {id}"
            )));
        }
        Ok(id)
    }
    fn revision(
        &mut self,
        id: &EntityId,
        supplied: Option<RecordId>,
        ctx: &mut Context<'_>,
    ) -> Result<RecordId> {
        let entity = self.entity(id)?;
        let dependencies = direct_dependency_ids(&self.view(), id);
        if let Some(last) = self.histories.get(id).and_then(|h| h.revisions.last())
            && last.title == entity.title
            && last.description == entity.description
            && last.parent == entity.parent
            && last.dependencies == dependencies
        {
            return Ok(last.id);
        }
        let record = if let Some(record) = supplied {
            let mut once = |_| record;
            self.record_id(
                RecordKind::Revision,
                &mut Context {
                    at: ctx.at,
                    evaluation: ctx.evaluation.clone(),
                    actor: ctx.actor,
                    reason: ctx.reason,
                    ids: &mut once,
                },
            )?
        } else {
            self.record_id(RecordKind::Revision, ctx)?
        };
        let revision = DeclarationRevision {
            id: record,
            title: entity.title.clone(),
            description: entity.description.clone(),
            parent: entity.parent.clone(),
            dependencies,
            created_at: ctx.at,
            baseline: false,
        };
        self.histories
            .entry(id.clone())
            .or_default()
            .revisions
            .push(revision);
        Ok(record)
    }
    pub fn execute(&self, operation: Operation, ctx: &mut Context<'_>) -> Result<ValidatedChange> {
        let mut state = self.clone();
        state.declaration.evaluation = ctx.evaluation.clone();
        let outcome = match operation {
            Operation::Insert(entity) => {
                state.insert(entity, ctx)?;
                ApplyOutcome::Changed
            }
            Operation::Change(id, change) => state.apply(&id, change, ctx)?,
            Operation::Dependency {
                source,
                target,
                present,
            } => state.dependency(&source, &target, present)?,
            Operation::AddNote { owner, body } => {
                if body.trim().is_empty() {
                    return Err(Error::EmptyNote);
                }
                state.entity(&owner)?;
                let id = state.record_id(RecordKind::Note, ctx)?;
                state.histories.entry(owner).or_default().notes.push(Note {
                    id,
                    body,
                    actor: ctx.actor.into(),
                    created_at: ctx.at,
                });
                ApplyOutcome::Changed
            }
            Operation::Import(desired) => state.import(desired, ctx)?,
        };
        Ok(ValidatedChange { state, outcome })
    }
    fn insert(&mut self, entity: Entity, ctx: &mut Context<'_>) -> Result<()> {
        if self.declaration.entities.iter().any(|e| e.id == entity.id) {
            return Err(Error::InvalidState(format!(
                "duplicate Entity {}",
                entity.id
            )));
        }
        if (entity.disposition == Disposition::Undecided) != entity.current_revision.is_none() {
            return Err(Error::InvalidState(
                "Disposition and current Revision disagree".into(),
            ));
        }
        self.parent_target(entity.parent.as_ref())?;
        if let ResurfaceCondition::AfterEntity(target) = &entity.resurface_condition {
            self.entity(target)?;
        }
        if let Some(parent) = &entity.parent {
            let view = self.view();
            if matches!(self.entity(parent)?.progress, Progress::Ended)
                || view
                    .ancestors(parent)
                    .iter()
                    .any(|e| matches!(e.progress, Progress::Ended))
            {
                return Err(Error::Containment(format!(
                    "cannot add {} below an Ended group",
                    entity.id
                )));
            }
        }
        let id = entity.id.clone();
        let revision = entity.current_revision;
        let at = entity.created_at;
        self.declaration.entities.push(entity);
        if let Some(revision) = revision {
            let mut revision_ctx = Context {
                at,
                evaluation: ctx.evaluation.clone(),
                actor: ctx.actor,
                reason: ctx.reason,
                ids: ctx.ids,
            };
            self.revision(&id, Some(revision), &mut revision_ctx)?;
        }
        validate_structure(&self.view())?;
        validate_relations(&self.view())
    }
    fn dependency(
        &mut self,
        source: &EntityId,
        target: &EntityId,
        present: bool,
    ) -> Result<ApplyOutcome> {
        let entity = self.entity(source)?;
        self.entity(target)?;
        let edge = (source.clone(), target.clone());
        if self.declaration.dependencies.contains(&edge) == present {
            return Ok(ApplyOutcome::Unchanged);
        }
        if entity.kind == EntityKind::Group && matches!(entity.progress, Progress::Ended) {
            return Err(Error::Containment(format!(
                "Ended group {source} cannot change its dependencies"
            )));
        }
        if entity.disposition != Disposition::Undecided {
            return Err(Error::DeclarationFixed(source.to_string()));
        }
        if present {
            if source == target {
                return Err(Error::InvalidState(format!(
                    "invalid dependency {source} -> {target}"
                )));
            }
            self.declaration.dependencies.push(edge);
            self.declaration.dependencies.sort();
            validate_relations(&self.view())?;
        } else {
            self.declaration.dependencies.retain(|e| *e != edge);
        }
        Ok(ApplyOutcome::Changed)
    }
    fn import(&mut self, desired: StoreSnapshot, ctx: &mut Context<'_>) -> Result<ApplyOutcome> {
        validate_import_snapshot(&self.declaration, &desired)?;
        let before = self.view();
        let mut entities = Vec::new();
        let mut new_ids = Vec::new();
        for after in desired.entities {
            let entity = if let Some(current) = before.get(&after.id) {
                let mut entity = current.clone();
                if entity.title != after.title
                    || entity.description != after.description
                    || entity.parent != after.parent
                {
                    entity.title = after.title;
                    entity.description = after.description;
                    entity.parent = after.parent;
                    entity.updated_at = ctx.at;
                }
                entity
            } else {
                new_ids.push(after.id.clone());
                Entity {
                    created_at: ctx.at,
                    updated_at: ctx.at,
                    current_revision: None,
                    ..after
                }
            };
            entities.push(entity);
        }
        let outcome = if entities == self.declaration.entities
            && desired.dependencies == self.declaration.dependencies
        {
            ApplyOutcome::Unchanged
        } else {
            ApplyOutcome::Changed
        };
        self.declaration.entities = entities;
        self.declaration.dependencies = desired.dependencies;
        for id in new_ids {
            let revision = self.revision(&id, None, ctx)?;
            self.entity_mut(&id).current_revision = Some(revision);
        }
        Ok(outcome)
    }
}

impl StateSnapshot {
    fn apply(
        &mut self,
        id: &EntityId,
        change: Change,
        ctx: &mut Context<'_>,
    ) -> Result<ApplyOutcome> {
        let before = self.entity(id)?.clone();
        if let Some((field, current)) = unchanged_transition(&before, &change) {
            return Err(Error::Unchanged {
                id: id.to_string(),
                field,
                current,
            });
        }
        if unchanged_setting(&before, &change) {
            return Ok(ApplyOutcome::Unchanged);
        }
        if matches!(
            change,
            Change::SetTitle(_) | Change::SetDescription(_) | Change::SetParent(_)
        ) && before.disposition != Disposition::Undecided
            && !matches!(
                change,
                Change::SetParent(_)
                    if before.kind == EntityKind::Group
                        && matches!(before.progress, Progress::Ended)
            )
        {
            return Err(Error::DeclarationFixed(id.to_string()));
        }

        let now = ctx.at;
        let view = self.view().at(ctx.at);
        match &change {
            Change::Start(claim) => {
                if !view.is_ready(&before)? {
                    return Err(Error::CannotStart {
                        id: id.to_string(),
                        fact: not_ready_fact(&view, &before)?,
                    });
                }
                self.entity_mut(id).progress = Progress::InProgress(claim.clone());
            }
            Change::Done => {
                if !matches!(before.progress, Progress::InProgress(_)) {
                    return Err(cannot_progress(id, "ended", &before));
                }
                if before.kind == EntityKind::Group && !view.group_completion_satisfied(id) {
                    let remaining = view
                        .descendants(id)
                        .into_iter()
                        .filter(|entity| !entity.is_terminal())
                        .map(|entity| entity.id.to_string())
                        .collect::<Vec<_>>()
                        .join(", ");
                    return Err(Error::CannotProgress {
                        id: id.to_string(),
                        action: "ended",
                        fact: format!("non-terminal descendants: {remaining}"),
                    });
                }
                self.entity_mut(id).progress = Progress::Ended;
            }
            Change::Release => {
                if !matches!(before.progress, Progress::InProgress(_)) {
                    return Err(cannot_progress(id, "released", &before));
                }
                if before.kind == EntityKind::Group {
                    let active = view.in_progress_descendants(id);
                    if !active.is_empty() {
                        return Err(Error::CannotProgress {
                            id: id.to_string(),
                            action: "released",
                            fact: format!(
                                "InProgress descendants: {}",
                                active
                                    .iter()
                                    .map(|entity| entity.id.to_string())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                        });
                    }
                }
                self.entity_mut(id).progress = Progress::NotStarted;
            }
            Change::Decide(disposition) => {
                let current_revision = if *disposition == Disposition::Undecided {
                    None
                } else {
                    Some(self.revision(id, None, ctx)?)
                };
                self.entity_mut(id).disposition = *disposition;
                self.entity_mut(id).current_revision = current_revision;
                let updated = self.view();
                if updated
                    .ancestors(id)
                    .into_iter()
                    .any(|ancestor| matches!(ancestor.progress, Progress::Ended))
                    && !updated
                        .get(id)
                        .expect("updated entity exists")
                        .is_terminal()
                {
                    return Err(Error::Containment(format!(
                        "{id} must remain terminal below an Ended group"
                    )));
                }
            }
            Change::SetResurfaceCondition(condition) => {
                if let ResurfaceCondition::AfterEntity(target) = condition {
                    self.entity(target)?;
                }
                self.entity_mut(id).resurface_condition = condition.clone();
                if matches!(condition, ResurfaceCondition::AfterEntity(_)) {
                    validate_relations(&self.view())?;
                }
            }
            Change::SetTitle(title) => {
                self.entity_mut(id).title = title.clone();
            }
            Change::SetDescription(description) => {
                self.entity_mut(id).description = description.clone();
            }
            Change::SetParent(parent) => {
                self.parent_target(parent.as_ref())?;
                if before.kind == EntityKind::Group && matches!(before.progress, Progress::Ended) {
                    return Err(Error::Containment(format!("Ended group {id} cannot move")));
                }
                if view
                    .ancestors(id)
                    .into_iter()
                    .any(|ancestor| matches!(ancestor.progress, Progress::Ended))
                {
                    return Err(Error::Containment(format!(
                        "{id} cannot move within an Ended group"
                    )));
                }
                if let Some(parent) = parent {
                    let target = self.entity(parent)?;
                    if matches!(target.progress, Progress::Ended)
                        || view
                            .ancestors(parent)
                            .into_iter()
                            .any(|ancestor| matches!(ancestor.progress, Progress::Ended))
                    {
                        return Err(Error::Containment(format!(
                            "cannot add {id} below an Ended group"
                        )));
                    }
                }
                self.entity_mut(id).parent = parent.clone();
                if parent.is_some() {
                    let updated = self.view();
                    validate_structure(&updated)?;
                    validate_relations(&updated)?;
                }
            }
        }

        self.entity_mut(id).updated_at = now;
        if let Some(kind) = progress_event_kind(&change) {
            let record = self.record_id(RecordKind::Progress, ctx)?;
            self.histories
                .entry(id.clone())
                .or_default()
                .progress
                .push(ProgressEvent {
                    id: record,
                    kind,
                    actor: ctx.actor.into(),
                    reason: if kind == ProgressEventKind::Release {
                        ctx.reason.map(str::to_string)
                    } else {
                        None
                    },
                    at: now,
                });
        }
        if let Some((field, old_value, new_value)) = decision_event(&before, &change) {
            let revision = if matches!(change, Change::Decide(_)) {
                self.entity(id)?.current_revision
            } else {
                None
            };
            let record = self.record_id(RecordKind::Decision, ctx)?;
            self.histories
                .entry(id.clone())
                .or_default()
                .decisions
                .push(Event {
                    id: record,
                    field: field.into(),
                    old_value,
                    new_value,
                    revision,
                    actor: ctx.actor.into(),
                    reason: ctx.reason.map(str::to_string),
                    at: now,
                });
        }
        Ok(ApplyOutcome::Changed)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn id(value: &str) -> EntityId {
        EntityId::from_stored(value)
    }
    pub fn at() -> DateTime<Utc> {
        "2026-01-02T03:04:05Z".parse().unwrap()
    }
    pub fn entity(value: &str, kind: EntityKind, parent: Option<&str>) -> Entity {
        Entity {
            id: id(value),
            kind,
            title: value.into(),
            description: None,
            parent: parent.map(id),
            progress: Progress::NotStarted,
            disposition: Disposition::Undecided,
            current_revision: None,
            resurface_condition: ResurfaceCondition::Always,
            created_at: at(),
            updated_at: at(),
        }
    }
    pub fn empty() -> StateSnapshot {
        StateSnapshot {
            declaration: StoreSnapshot {
                evaluation: Rc::new(Evaluation::new(std::env::temp_dir())),
                entities: vec![],
                dependencies: vec![],
            },
            metadata: BTreeMap::new(),
            histories: BTreeMap::new(),
        }
    }
    pub fn execute(
        state: &StateSnapshot,
        operation: Operation,
        tick: u64,
    ) -> Result<ValidatedChange> {
        let mut sequence = 0u64;
        let mut ids = |kind| {
            sequence += 1;
            RecordId::deterministic(kind, format!("{tick}-{sequence}").as_bytes())
        };
        state.execute(
            operation,
            &mut Context {
                at: at() - chrono::Duration::seconds(tick as i64),
                actor: "contract",
                reason: Some("reason"),
                evaluation: state.declaration.evaluation.clone(),
                ids: &mut ids,
            },
        )
    }
    /// The same behavioral assertions run against memory and transactional SQLite publication.
    pub fn contract(
        mut run: impl FnMut(Operation) -> std::result::Result<(ApplyOutcome, StateSnapshot), String>,
    ) {
        let change = |name: &str, change| Operation::Change(id(name), change);
        let claim = || Claim {
            actor: "contract".into(),
            worktree: "/worktree/contract".into(),
            at: at(),
        };
        run(Operation::Insert(entity("g", EntityKind::Group, None))).unwrap();
        run(Operation::Insert(entity("i", EntityKind::Issue, Some("g")))).unwrap();
        assert!(run(change("i", Change::Start(claim()))).is_err());
        run(change("g", Change::Decide(Disposition::Accepted))).unwrap();
        let (_, state) = run(change("i", Change::Decide(Disposition::Accepted))).unwrap();
        let initial_revision = state.entity(&id("i")).unwrap().current_revision;
        assert!(run(change("i", Change::Start(claim()))).is_err());
        assert_eq!(
            run(change("i", Change::SetTitle("i".into()))).unwrap().0,
            ApplyOutcome::Unchanged
        );
        assert!(run(change("i", Change::SetTitle("changed".into()))).is_err());
        assert!(run(change("i", Change::Decide(Disposition::Accepted))).is_err());
        run(change("g", Change::Start(claim()))).unwrap();
        let (_, state) = run(change("i", Change::Start(claim()))).unwrap();
        assert_eq!(
            state
                .entity(&id("i"))
                .unwrap()
                .progress
                .claim()
                .unwrap()
                .worktree,
            "/worktree/contract"
        );
        assert!(run(change("i", Change::Start(claim()))).is_err());
        assert!(run(change("g", Change::Release)).is_err());
        assert!(run(change("g", Change::Done)).is_err());
        run(change("i", Change::Release)).unwrap();
        assert!(run(change("i", Change::Release)).is_err());
        run(change("i", Change::Decide(Disposition::Undecided))).unwrap();
        let (_, state) = run(change("i", Change::Decide(Disposition::Rejected))).unwrap();
        assert_eq!(
            state.entity(&id("i")).unwrap().current_revision,
            initial_revision
        );
        assert_eq!(state.histories[&id("i")].revisions.len(), 1);
        run(change("i", Change::Decide(Disposition::Undecided))).unwrap();
        run(change("i", Change::SetTitle("changed".into()))).unwrap();
        run(change(
            "i",
            Change::SetDescription(Some("description".into())),
        ))
        .unwrap();
        run(change("i", Change::SetParent(None))).unwrap();
        run(Operation::Dependency {
            source: id("i"),
            target: id("g"),
            present: true,
        })
        .unwrap();
        assert_eq!(
            run(Operation::Dependency {
                source: id("i"),
                target: id("g"),
                present: true
            })
            .unwrap()
            .0,
            ApplyOutcome::Unchanged
        );
        assert!(
            run(Operation::Dependency {
                source: id("i"),
                target: id("missing"),
                present: true
            })
            .is_err()
        );
        run(Operation::Dependency {
            source: id("i"),
            target: id("g"),
            present: false,
        })
        .unwrap();
        run(change("i", Change::SetParent(Some(id("g"))))).unwrap();
        assert!(run(change("i", Change::SetParent(Some(id("i"))))).is_err());
        assert!(
            run(change(
                "i",
                Change::SetResurfaceCondition(ResurfaceCondition::AfterEntity(id("g"))),
            ))
            .is_err()
        );
        run(change(
            "i",
            Change::SetResurfaceCondition(ResurfaceCondition::Manual),
        ))
        .unwrap();
        run(change(
            "i",
            Change::SetResurfaceCondition(ResurfaceCondition::Always),
        ))
        .unwrap();
        run(change("i", Change::Decide(Disposition::Accepted))).unwrap();
        run(change("i", Change::Decide(Disposition::Undecided))).unwrap();
        run(change("i", Change::SetTitle("i".into()))).unwrap();
        run(change("i", Change::SetDescription(None))).unwrap();
        let (_, state) = run(change("i", Change::Decide(Disposition::Accepted))).unwrap();
        assert_eq!(state.histories[&id("i")].revisions.len(), 3);
        assert_ne!(
            state.entity(&id("i")).unwrap().current_revision,
            initial_revision
        );
        run(change("i", Change::Start(claim()))).unwrap();
        run(change("i", Change::Done)).unwrap();
        assert!(run(change("i", Change::Done)).is_err());
        run(change("g", Change::Done)).unwrap();
        assert!(run(change("g", Change::SetParent(None))).is_ok()); // exact setting no-op
        assert!(
            run(Operation::Insert(entity(
                "late",
                EntityKind::Issue,
                Some("g")
            )))
            .is_err()
        );
        let (_, state) = run(Operation::AddNote {
            owner: id("i"),
            body: "first".into(),
        })
        .unwrap();
        let first = state.histories[&id("i")].notes[0].clone();
        let (_, state) = run(Operation::AddNote {
            owner: id("i"),
            body: "second".into(),
        })
        .unwrap();
        let history = &state.histories[&id("i")];
        assert_eq!(history.notes[0], first);
        assert!(history.notes[0].created_at > history.notes[1].created_at);
        assert!(
            history
                .progress
                .iter()
                .filter(|e| e.kind != ProgressEventKind::Release)
                .all(|e| e.reason.is_none())
        );
        assert_eq!(
            history
                .progress
                .iter()
                .find(|e| e.kind == ProgressEventKind::Release)
                .unwrap()
                .reason
                .as_deref(),
            Some("reason")
        );
        assert!(
            run(Operation::AddNote {
                owner: id("i"),
                body: " \n\t".into()
            })
            .is_err()
        );
        assert!(
            run(Operation::AddNote {
                owner: id("missing"),
                body: "text".into()
            })
            .is_err()
        );
        let mut desired = state.declaration.clone();
        let mut new = entity("new", EntityKind::Issue, None);
        new.disposition = Disposition::Accepted;
        new.current_revision = Some(RecordId::deterministic(
            RecordKind::Revision,
            b"placeholder",
        ));
        desired.entities.push(new);
        desired.dependencies.push((id("new"), id("i")));
        let (_, state) = run(Operation::Import(desired)).unwrap();
        assert_eq!(
            state.histories[&id("new")].revisions[0].dependencies,
            vec![id("i")]
        );
        assert!(state.histories[&id("new")].decisions.is_empty());
        let mut invalid = state.declaration.clone();
        invalid
            .entities
            .iter_mut()
            .find(|e| e.id == id("g"))
            .unwrap()
            .progress = Progress::NotStarted;
        assert!(run(Operation::Import(invalid)).is_err());
    }
    #[test]
    fn memory_contract() {
        let mut state = empty();
        let mut tick = 0;
        contract(|operation| {
            tick += 1;
            let result = execute(&state, operation, tick).map_err(|e| e.to_string())?;
            state = result.state;
            Ok((result.outcome, state.clone()))
        });
    }
    #[test]
    fn failed_generation_leaves_no_revision_or_control_change() {
        let state = execute(
            &empty(),
            Operation::Insert(entity("i", EntityKind::Issue, None)),
            1,
        )
        .unwrap()
        .state;
        let result = state.execute(
            Operation::Change(id("i"), Change::Decide(Disposition::Accepted)),
            &mut Context {
                at: at(),
                actor: "test",
                reason: None,
                evaluation: state.declaration.evaluation.clone(),
                ids: &mut |_| RecordId::deterministic(RecordKind::Revision, b"same"),
            },
        );
        assert!(result.is_err()); // wrong kind for the second generated record
        assert!(state.histories.is_empty());
        assert_eq!(
            state.entity(&id("i")).unwrap().disposition,
            Disposition::Undecided
        );
    }
    #[test]
    fn start_uses_supplied_date_and_command_context() {
        let mut input = entity("i", EntityKind::Issue, None);
        input.disposition = Disposition::Accepted;
        input.current_revision = Some(RecordId::deterministic(RecordKind::Revision, b"initial"));
        input.resurface_condition = ResurfaceCondition::AtDate(at().date_naive());
        let state = execute(&empty(), Operation::Insert(input), 1)
            .unwrap()
            .state;
        let start = || {
            Operation::Change(
                id("i"),
                Change::Start(Claim {
                    actor: "test".into(),
                    worktree: "/test".into(),
                    at: at(),
                }),
            )
        };
        assert!(execute(&state, start(), 100_000).is_err());
        assert!(execute(&state, start(), 0).is_ok());
        let state = execute(
            &state,
            Operation::Change(
                id("i"),
                Change::SetResurfaceCondition(ResurfaceCondition::Command("exit 2".into())),
            ),
            1,
        )
        .unwrap()
        .state;
        assert!(matches!(
            execute(&state, start(), 2),
            Err(Error::Evaluation(_))
        ));
        let cleared = execute(
            &state,
            Operation::Change(
                id("i"),
                Change::SetResurfaceCondition(ResurfaceCondition::Always),
            ),
            3,
        )
        .unwrap();
        assert!(execute(cleared.state(), start(), 4).is_ok());
    }
}
