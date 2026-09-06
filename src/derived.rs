use crate::domain::*;
use chrono::Utc;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::rc::Rc;

#[derive(Debug, Clone, thiserror::Error)]
#[error("{0}")]
pub struct EvaluationError(String);

pub type Result<T> = std::result::Result<T, EvaluationError>;

pub struct Evaluation {
    root: PathBuf,
    results: RefCell<HashMap<EntityId, Result<bool>>>,
    trace: Option<RefCell<Box<dyn Write>>>,
}

impl Evaluation {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            results: RefCell::new(HashMap::new()),
            trace: None,
        }
    }

    pub fn tracing(root: PathBuf) -> Self {
        Self::with_trace_writer(root, Box::new(std::io::stderr()))
    }

    fn with_trace_writer(root: PathBuf, writer: Box<dyn Write>) -> Self {
        Self {
            root,
            results: RefCell::new(HashMap::new()),
            trace: Some(RefCell::new(writer)),
        }
    }

    fn command(&self, entity: &Entity, script: &str) -> Result<bool> {
        if let Some(result) = self.results.borrow().get(&entity.id) {
            return result.clone();
        }
        let result = self.run_command(entity, script);
        self.results
            .borrow_mut()
            .insert(entity.id.clone(), result.clone());
        result
    }

    fn run_command(&self, entity: &Entity, script: &str) -> Result<bool> {
        let output = Command::new("/bin/sh")
            .args(["-c", script])
            .current_dir(&self.root)
            .stdin(Stdio::null())
            .output()
            .map_err(|error| {
                EvaluationError(format!(
                    "{}: Command({script:?}) could not start in {}: {error}",
                    entity.id,
                    self.root.display()
                ))
            })?;
        match output.status.code() {
            Some(code @ (0 | 1)) => {
                self.write_trace(entity, code, &output.stdout, &output.stderr)?;
                Ok(code == 0)
            }
            _ => Err(EvaluationError(format!(
                "{}: Command({script:?}) failed: {}\nstdout:\n{}\nstderr:\n{}",
                entity.id,
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ))),
        }
    }

    fn write_trace(&self, entity: &Entity, code: i32, stdout: &[u8], stderr: &[u8]) -> Result<()> {
        let Some(writer) = &self.trace else {
            return Ok(());
        };
        let result = if code == 0 {
            "satisfied"
        } else {
            "not satisfied"
        };
        let mut block = format!(
            "Condition trace: {}\ncwd: {}\nresult: {result} (exit {code})\n",
            entity.id,
            self.root.display()
        );
        append_trace_stream(&mut block, "stdout", stdout);
        append_trace_stream(&mut block, "stderr", stderr);
        block.push_str(&format!("End condition trace: {}\n", entity.id));

        let mut writer = writer.borrow_mut();
        writer
            .write_all(block.as_bytes())
            .and_then(|()| writer.flush())
            .map_err(|error| {
                EvaluationError(format!(
                    "{}: Command condition trace could not be written: {error}",
                    entity.id
                ))
            })
    }
}

fn append_trace_stream(block: &mut String, label: &str, bytes: &[u8]) {
    block.push_str(label);
    block.push_str(":\n");
    if bytes.is_empty() {
        block.push_str("(empty)\n");
        return;
    }
    block.push_str(&String::from_utf8_lossy(bytes));
    if !bytes.ends_with(b"\n") {
        block.push('\n');
    }
}

#[derive(Clone)]
pub struct View {
    by_id: HashMap<EntityId, Entity>,
    order: Vec<EntityId>,
    deps: Vec<(EntityId, EntityId)>,
    evaluation: Rc<Evaluation>,
    date: Option<chrono::NaiveDate>,
}

impl View {
    #[cfg(test)]
    pub fn new(entities: Vec<Entity>, deps: Vec<(EntityId, EntityId)>) -> Self {
        Self::with_evaluation(
            entities,
            deps,
            Rc::new(Evaluation::new(std::env::current_dir().unwrap())),
        )
    }

    pub fn with_evaluation(
        entities: Vec<Entity>,
        deps: Vec<(EntityId, EntityId)>,
        evaluation: Rc<Evaluation>,
    ) -> Self {
        let order = entities.iter().map(|entity| entity.id.clone()).collect();
        let by_id = entities
            .into_iter()
            .map(|entity| (entity.id.clone(), entity))
            .collect();
        Self {
            by_id,
            order,
            deps,
            evaluation,
            date: None,
        }
    }

    pub fn at(mut self, at: chrono::DateTime<Utc>) -> Self {
        self.date = Some(at.date_naive());
        self
    }

    pub fn get(&self, id: &EntityId) -> Option<&Entity> {
        self.by_id.get(id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Entity> {
        self.order.iter().filter_map(|id| self.by_id.get(id))
    }

    pub fn dependencies(&self) -> &[(EntityId, EntityId)] {
        &self.deps
    }

    pub fn direct_dependencies(&self, id: &EntityId) -> Vec<&Entity> {
        self.deps
            .iter()
            .filter(|(source, _)| source == id)
            .filter_map(|(_, target)| self.get(target))
            .collect()
    }

    pub fn direct_dependents(&self, id: &EntityId) -> Vec<&Entity> {
        self.deps
            .iter()
            .filter(|(_, target)| target == id)
            .filter_map(|(source, _)| self.get(source))
            .collect()
    }

    pub fn ancestors(&self, id: &EntityId) -> Vec<&Entity> {
        let mut result = Vec::new();
        let mut seen = HashSet::new();
        let mut current = self.get(id).and_then(|entity| entity.parent.as_ref());
        while let Some(parent_id) = current {
            if !seen.insert(parent_id.clone()) {
                break;
            }
            let Some(parent) = self.get(parent_id) else {
                break;
            };
            result.push(parent);
            current = parent.parent.as_ref();
        }
        result
    }

    pub fn descendants(&self, id: &EntityId) -> Vec<&Entity> {
        let mut result = Vec::new();
        let mut seen = HashSet::new();
        let mut stack = vec![id.clone()];
        while let Some(parent) = stack.pop() {
            for child in self
                .iter()
                .filter(|entity| entity.parent.as_ref() == Some(&parent))
            {
                if seen.insert(child.id.clone()) {
                    result.push(child);
                    stack.push(child.id.clone());
                }
            }
        }
        result
    }

    pub fn direct_children(&self, id: &EntityId) -> Vec<&Entity> {
        self.iter()
            .filter(|entity| entity.parent.as_ref() == Some(id))
            .collect()
    }

    fn waiting_sources(&self, id: &EntityId) -> Vec<&Entity> {
        self.get(id).into_iter().chain(self.ancestors(id)).collect()
    }

    pub fn dependency_targets(&self, id: &EntityId) -> Vec<&Entity> {
        let mut seen = HashSet::new();
        self.waiting_sources(id)
            .into_iter()
            .flat_map(|source| self.direct_dependencies(&source.id))
            .filter(|target| seen.insert(target.id.clone()))
            .collect()
    }

    pub fn is_surfaced(&self, entity: &Entity) -> Result<bool> {
        Ok(match &entity.resurface_condition {
            ResurfaceCondition::Always => true,
            ResurfaceCondition::Manual => false,
            ResurfaceCondition::Command(script) => self.evaluation.command(entity, script)?,
            ResurfaceCondition::AtDate(date) => {
                *date <= self.date.unwrap_or_else(|| Utc::now().date_naive())
            }
            ResurfaceCondition::AfterEntity(target) => {
                self.get(target).is_none_or(Entity::is_terminal)
            }
        })
    }

    pub fn is_blocked(&self, id: &EntityId) -> bool {
        self.dependency_targets(id)
            .into_iter()
            .any(|target| !target.is_terminal())
    }

    pub fn is_orphaned(&self, id: &EntityId) -> bool {
        self.dependency_targets(id)
            .into_iter()
            .any(|target| target.disposition == Disposition::Rejected)
    }

    pub fn opens_descendants(&self, group: &Entity) -> Result<bool> {
        Ok(group.kind == EntityKind::Group
            && matches!(group.progress, Progress::InProgress(_))
            && group.disposition == Disposition::Accepted
            && self.is_surfaced(group)?
            && !self.is_blocked(&group.id)
            && !self.is_orphaned(&group.id))
    }

    pub fn within_active_scope(&self, id: &EntityId) -> Result<bool> {
        for group in self.ancestors(id) {
            if !self.opens_descendants(group)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub fn is_ready(&self, entity: &Entity) -> Result<bool> {
        Ok(matches!(entity.progress, Progress::NotStarted)
            && entity.disposition == Disposition::Accepted
            && self.is_surfaced(entity)?
            && self.within_active_scope(&entity.id)?
            && !self.is_blocked(&entity.id)
            && !self.is_orphaned(&entity.id))
    }

    pub fn ready(&self, include: impl Fn(&Entity) -> bool) -> Result<Vec<&Entity>> {
        let mut result = Vec::new();
        for entity in self.iter().filter(|entity| include(entity)) {
            if self.is_ready(entity)? {
                result.push(entity);
            }
        }
        Ok(result)
    }

    pub fn triage(
        &self,
        include: impl Fn(&Entity) -> bool,
    ) -> Result<Vec<(&Entity, TriageReason)>> {
        let mut result = Vec::new();
        for entity in self
            .iter()
            .filter(|entity| include(entity) && !entity.is_terminal())
        {
            let reason = if entity.disposition == Disposition::Undecided {
                TriageReason::Undecided
            } else if self.is_orphaned(&entity.id) {
                TriageReason::Orphaned
            } else {
                continue;
            };
            if self.is_surfaced(entity)? && self.within_active_scope(&entity.id)? {
                result.push((entity, reason));
            }
        }
        Ok(result)
    }

    pub fn claims(&self) -> Vec<(&Entity, &Claim)> {
        self.iter()
            .filter_map(|entity| entity.progress.claim().map(|claim| (entity, claim)))
            .collect()
    }

    pub fn group_completion_satisfied(&self, id: &EntityId) -> bool {
        self.descendants(id).into_iter().all(Entity::is_terminal)
    }

    pub fn can_complete_group(&self, id: &EntityId) -> bool {
        self.get(id).is_some_and(|entity| {
            entity.kind == EntityKind::Group
                && matches!(entity.progress, Progress::InProgress(_))
                && self.group_completion_satisfied(id)
        })
    }

    pub fn in_progress_descendants(&self, id: &EntityId) -> Vec<&Entity> {
        self.descendants(id)
            .into_iter()
            .filter(|entity| matches!(entity.progress, Progress::InProgress(_)))
            .collect()
    }

    pub fn group_summary(&self, id: &EntityId) -> GroupSummary {
        let direct = self.direct_children(id);
        let descendants = self.descendants(id);
        GroupSummary {
            direct: EntityCounts::from_entities(&direct),
            descendants: EntityCounts::from_entities(&descendants),
        }
    }

    fn is_blocking_root_cause(&self, entity: &Entity) -> Result<bool> {
        Ok(self.is_orphaned(&entity.id)
            || !self.is_surfaced(entity)?
            || !self.within_active_scope(&entity.id)?
            || self
                .dependency_targets(&entity.id)
                .into_iter()
                .all(Entity::is_terminal))
    }

    pub fn blocking_causes(&self, id: &EntityId) -> Result<Vec<&Entity>> {
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        self.walk_causes(id, &mut seen, &mut result)?;
        Ok(result)
    }

    fn walk_causes<'a>(
        &'a self,
        id: &EntityId,
        seen: &mut HashSet<EntityId>,
        result: &mut Vec<&'a Entity>,
    ) -> Result<()> {
        for target in self.dependency_targets(id) {
            if target.is_terminal() || !seen.insert(target.id.clone()) {
                continue;
            }
            if self.is_blocking_root_cause(target)? {
                result.push(target);
            } else {
                self.walk_causes(&target.id, seen, result)?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriageReason {
    Undecided,
    Orphaned,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct EntityCounts {
    pub total: usize,
    pub issues: usize,
    pub groups: usize,
    pub not_started: usize,
    pub in_progress: usize,
    pub ended: usize,
    pub undecided: usize,
    pub accepted: usize,
    pub rejected: usize,
    pub terminal: usize,
}

impl EntityCounts {
    fn from_entities(entities: &[&Entity]) -> Self {
        let mut counts = Self::default();
        for entity in entities {
            counts.total += 1;
            match entity.kind {
                EntityKind::Issue => counts.issues += 1,
                EntityKind::Group => counts.groups += 1,
            }
            match entity.progress {
                Progress::NotStarted => counts.not_started += 1,
                Progress::InProgress(_) => counts.in_progress += 1,
                Progress::Ended => counts.ended += 1,
            }
            match entity.disposition {
                Disposition::Undecided => counts.undecided += 1,
                Disposition::Accepted => counts.accepted += 1,
                Disposition::Rejected => counts.rejected += 1,
            }
            counts.terminal += usize::from(entity.is_terminal());
        }
        counts
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupSummary {
    pub direct: EntityCounts,
    pub descendants: EntityCounts,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    fn entity(
        id: &str,
        kind: EntityKind,
        progress: Progress,
        disposition: Disposition,
        parent: Option<&str>,
    ) -> Entity {
        let now = Utc::now();
        Entity {
            id: EntityId::from_stored(id),
            kind,
            title: id.to_string(),
            description: None,
            progress,
            disposition,
            current_revision: (disposition != Disposition::Undecided)
                .then(|| RecordId::new(RecordKind::Revision)),
            resurface_condition: ResurfaceCondition::Always,
            parent: parent.map(EntityId::from_stored),
            created_at: now,
            updated_at: now,
        }
    }

    fn claim() -> Claim {
        Claim {
            actor: "tester".to_string(),
            worktree: "/worktree".to_string(),
            at: Utc::now(),
        }
    }

    fn id(value: &str) -> EntityId {
        EntityId::from_stored(value)
    }

    #[test]
    fn command_spawn_failure_is_an_error_and_is_shared() {
        let entity = entity(
            "command",
            EntityKind::Issue,
            Progress::NotStarted,
            Disposition::Accepted,
            None,
        );
        let evaluation = Evaluation::new(
            std::env::temp_dir().join(format!("axon-missing-command-root-{}", std::process::id())),
        );
        let first = evaluation
            .command(&entity, "exit 0")
            .unwrap_err()
            .to_string();
        assert!(first.contains("command"));
        assert!(first.contains("could not start"));
        assert!(first.contains("exit 0"));
        assert_eq!(
            evaluation
                .command(&entity, "exit 0")
                .unwrap_err()
                .to_string(),
            first
        );
        assert_eq!(evaluation.results.borrow().len(), 1);
    }

    struct FailingWriter;

    impl Write for FailingWriter {
        fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("trace sink failed"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn command_trace_write_failure_is_an_evaluation_error_and_is_shared() {
        let entity = entity(
            "command",
            EntityKind::Issue,
            Progress::NotStarted,
            Disposition::Accepted,
            None,
        );
        let evaluation = Evaluation::with_trace_writer(
            std::env::current_dir().unwrap(),
            Box::new(FailingWriter),
        );
        let first = evaluation
            .command(&entity, "exit 0")
            .unwrap_err()
            .to_string();
        assert!(first.contains("command"));
        assert!(first.contains("trace could not be written"));
        assert!(first.contains("trace sink failed"));
        assert_eq!(
            evaluation
                .command(&entity, "exit 0")
                .unwrap_err()
                .to_string(),
            first
        );
        assert_eq!(evaluation.results.borrow().len(), 1);
    }

    #[test]
    fn group_gate_exposes_only_the_current_frontier() {
        let closed = View::new(
            vec![
                entity(
                    "g",
                    EntityKind::Group,
                    Progress::NotStarted,
                    Disposition::Accepted,
                    None,
                ),
                entity(
                    "i",
                    EntityKind::Issue,
                    Progress::NotStarted,
                    Disposition::Accepted,
                    Some("g"),
                ),
            ],
            vec![],
        );
        assert_eq!(
            closed
                .ready(|_| true)
                .unwrap()
                .iter()
                .map(|e| e.id.as_str())
                .collect::<Vec<_>>(),
            ["g"]
        );

        let open = View::new(
            vec![
                entity(
                    "g",
                    EntityKind::Group,
                    Progress::InProgress(claim()),
                    Disposition::Accepted,
                    None,
                ),
                entity(
                    "i",
                    EntityKind::Issue,
                    Progress::NotStarted,
                    Disposition::Accepted,
                    Some("g"),
                ),
            ],
            vec![],
        );
        assert_eq!(
            open.ready(|_| true)
                .unwrap()
                .iter()
                .map(|e| e.id.as_str())
                .collect::<Vec<_>>(),
            ["i"]
        );
    }

    #[test]
    fn group_dependencies_apply_to_all_descendants() {
        let view = View::new(
            vec![
                entity(
                    "g",
                    EntityKind::Group,
                    Progress::InProgress(claim()),
                    Disposition::Accepted,
                    None,
                ),
                entity(
                    "i",
                    EntityKind::Issue,
                    Progress::NotStarted,
                    Disposition::Accepted,
                    Some("g"),
                ),
                entity(
                    "x",
                    EntityKind::Issue,
                    Progress::NotStarted,
                    Disposition::Accepted,
                    None,
                ),
            ],
            vec![(id("g"), id("x"))],
        );
        assert!(view.is_blocked(&id("g")));
        assert!(view.is_blocked(&id("i")));
        assert!(!view.is_ready(view.get(&id("i")).unwrap()).unwrap());
    }

    #[test]
    fn rejected_group_does_not_make_descendants_terminal() {
        let view = View::new(
            vec![
                entity(
                    "parent",
                    EntityKind::Group,
                    Progress::InProgress(claim()),
                    Disposition::Accepted,
                    None,
                ),
                entity(
                    "child",
                    EntityKind::Group,
                    Progress::NotStarted,
                    Disposition::Rejected,
                    Some("parent"),
                ),
                entity(
                    "issue",
                    EntityKind::Issue,
                    Progress::NotStarted,
                    Disposition::Accepted,
                    Some("child"),
                ),
            ],
            vec![],
        );
        assert!(!view.group_completion_satisfied(&id("parent")));
    }

    #[test]
    fn triage_stops_at_an_inactive_parent() {
        let view = View::new(
            vec![
                entity(
                    "g",
                    EntityKind::Group,
                    Progress::NotStarted,
                    Disposition::Undecided,
                    None,
                ),
                entity(
                    "i",
                    EntityKind::Issue,
                    Progress::NotStarted,
                    Disposition::Undecided,
                    Some("g"),
                ),
            ],
            vec![],
        );
        assert_eq!(
            view.triage(|_| true)
                .unwrap()
                .iter()
                .map(|(e, _)| e.id.as_str())
                .collect::<Vec<_>>(),
            ["g"]
        );
    }

    #[test]
    fn completion_and_summary_include_all_descendants() {
        let view = View::new(
            vec![
                entity(
                    "g",
                    EntityKind::Group,
                    Progress::InProgress(claim()),
                    Disposition::Accepted,
                    None,
                ),
                entity(
                    "child",
                    EntityKind::Group,
                    Progress::Ended,
                    Disposition::Accepted,
                    Some("g"),
                ),
                entity(
                    "i",
                    EntityKind::Issue,
                    Progress::Ended,
                    Disposition::Accepted,
                    Some("child"),
                ),
            ],
            vec![],
        );
        assert!(view.can_complete_group(&id("g")));
        let summary = view.group_summary(&id("g"));
        assert_eq!(summary.direct.total, 1);
        assert_eq!(summary.descendants.total, 2);
        assert_eq!(summary.descendants.terminal, 2);
    }
}
