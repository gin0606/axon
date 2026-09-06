pub mod migration;
mod record_id;
use crate::core;
pub use crate::core::{
    ApplyOutcome, Change, Ctx, Event, ProgressEvent, ProgressEventKind, StoreSnapshot,
};

pub fn validate_import_snapshot(before: &StoreSnapshot, desired: &StoreSnapshot) -> Result<()> {
    Ok(core::validate_import_snapshot(before, desired)?)
}

use crate::derived::{Evaluation, EvaluationError, View};
use crate::domain::*;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error(transparent)]
    Migration(#[from] migration::Failure),
    #[error("{0}")]
    Evaluation(#[from] EvaluationError),
    #[error("storage: {0}")]
    Storage(String),
    #[error("SQLite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("could not resolve Git common root: {0}")]
    GitRoot(String),
    #[error("axon is not initialized; run `axon init`")]
    NotInitialized,
    #[error("axon is already initialized at {0}")]
    AlreadyInitialized(PathBuf),
    #[error("could not read a stored value: {0}")]
    Parse(#[from] ParseError),
    #[error("invalid stored progress event kind: {0}")]
    InvalidProgressEvent(String),
    #[error("entity {0} does not exist")]
    NoSuchEntity(String),
    #[error("{id} matches multiple entities: {candidates}")]
    AmbiguousId { id: String, candidates: String },
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
    #[error("record reference {0} is ambiguous; use a longer stable ID")]
    AmbiguousRecord(String),
    #[error("Note {number} does not exist for {id}")]
    NoSuchNote { id: String, number: String },
    #[error("Declaration Revision {number} does not exist for {id}")]
    NoSuchRevision { id: String, number: String },
    #[error("the plan declaration of {0} is fixed by its Disposition")]
    DeclarationFixed(String),
    #[error("Entity {0} cannot depend on itself")]
    SelfDependency(String),
    #[error("{source}\nApplied: {applied}")]
    Partial {
        applied: String,
        #[source]
        source: Box<DbError>,
    },
    #[error("{0}")]
    Initialization(#[source] Box<DbError>),
    #[error("{path} {phase}: {source}\n{result}: storage update at {path}")]
    Boundary {
        path: String,
        phase: &'static str,
        result: &'static str,
        #[source]
        source: Box<DbError>,
    },
    #[error("invalid axon schema: {0}")]
    InvalidSchema(String),
    #[error("unsupported axon schema version {found}; expected {expected}")]
    UnsupportedSchema { found: i64, expected: i64 },
}

pub type Result<T> = std::result::Result<T, DbError>;

const SCHEMA_VERSION: i64 = 13;

#[cfg(test)]
const SCHEMA_V1: &str = r#"
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE issues (
  id TEXT PRIMARY KEY, title TEXT NOT NULL, description TEXT,
  progress TEXT NOT NULL CHECK (progress IN ('not_started','in_progress','ended')),
  commitment TEXT NOT NULL CHECK (commitment IN ('undecided','accepted','rejected')),
  cond_kind TEXT CHECK (cond_kind IN ('date','after_issue')), cond_date TEXT, cond_ref TEXT,
  claimed_actor TEXT, claimed_session TEXT, claimed_pid INTEGER, claimed_at TEXT,
  created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
  CHECK ((progress = 'in_progress' AND claimed_session IS NOT NULL) OR
         (progress <> 'in_progress' AND claimed_session IS NULL)),
  CHECK ((cond_kind IS NULL AND cond_date IS NULL AND cond_ref IS NULL) OR
         (cond_kind = 'date' AND cond_date IS NOT NULL AND cond_ref IS NULL) OR
         (cond_kind = 'after_issue' AND cond_ref IS NOT NULL AND cond_date IS NULL))
);
CREATE TABLE issue_deps (
  issue_id TEXT NOT NULL REFERENCES issues(id) ON DELETE CASCADE,
  depends_on_id TEXT NOT NULL REFERENCES issues(id) ON DELETE CASCADE,
  PRIMARY KEY (issue_id, depends_on_id), CHECK (issue_id <> depends_on_id)
);
"#;

#[cfg(test)]
const SCHEMA_V2: &str = r#"
CREATE TABLE groups (
  id TEXT PRIMARY KEY, slug TEXT NOT NULL UNIQUE, name TEXT NOT NULL, description TEXT,
  parent_id TEXT REFERENCES groups(id), created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
  CHECK (id <> parent_id)
);
CREATE TABLE group_deps (
  group_id TEXT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
  depends_on_id TEXT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
  PRIMARY KEY (group_id, depends_on_id), CHECK (group_id <> depends_on_id)
);
ALTER TABLE issues ADD COLUMN group_id TEXT REFERENCES groups(id);
"#;

#[cfg(test)]
const SCHEMA_V3: &str = r#"
CREATE TABLE events (
  id INTEGER PRIMARY KEY, issue_id TEXT NOT NULL REFERENCES issues(id) ON DELETE CASCADE,
  field TEXT NOT NULL, old_value TEXT, new_value TEXT, actor TEXT NOT NULL, reason TEXT,
  at TEXT NOT NULL
);
CREATE INDEX idx_events_issue ON events (issue_id, at);
"#;

#[cfg(test)]
const SCHEMA_V4: &str = "DELETE FROM events WHERE field NOT IN ('commitment', 'condition');";

#[cfg(test)]
const SCHEMA_V5: &str = r#"
CREATE TABLE progress_events (
  id INTEGER PRIMARY KEY, issue_id TEXT NOT NULL REFERENCES issues(id) ON DELETE CASCADE,
  kind TEXT NOT NULL CHECK (kind IN ('start','done','release')), actor TEXT NOT NULL,
  reason TEXT, at TEXT NOT NULL
);
CREATE INDEX idx_progress_events_issue ON progress_events (issue_id, at);
"#;

#[cfg(test)]
const SCHEMA_V6: &str = r#"
ALTER TABLE issues RENAME COLUMN commitment TO disposition;
ALTER TABLE issues RENAME COLUMN cond_kind TO resurface_kind;
ALTER TABLE issues RENAME COLUMN cond_date TO resurface_date;
ALTER TABLE issues RENAME COLUMN cond_ref TO resurface_ref;
UPDATE events SET field = 'disposition' WHERE field = 'commitment';
UPDATE events SET old_value = CASE
  WHEN old_value LIKE 'after %' THEN 'AfterIssue(' || substr(old_value, 7) || ')'
  ELSE 'AtDate(' || old_value || ')' END
WHERE field = 'condition' AND old_value IS NOT NULL;
UPDATE events SET new_value = CASE
  WHEN new_value LIKE 'after %' THEN 'AfterIssue(' || substr(new_value, 7) || ')'
  ELSE 'AtDate(' || new_value || ')' END
WHERE field = 'condition' AND new_value IS NOT NULL;
UPDATE events SET field = 'resurface_condition' WHERE field = 'condition';
"#;

#[cfg(test)]
const SCHEMA_V7: &str = r#"
ALTER TABLE issues RENAME COLUMN claimed_session TO claimed_worktree;
UPDATE issues SET claimed_worktree = 'unknown' WHERE progress = 'in_progress';
ALTER TABLE issues DROP COLUMN claimed_pid;
"#;

#[cfg(test)]
const SCHEMA_V8: &str = r#"
CREATE TABLE entities (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL CHECK (kind IN ('issue','group')),
  title TEXT NOT NULL,
  description TEXT,
  progress TEXT NOT NULL CHECK (progress IN ('not_started','in_progress','ended')),
  disposition TEXT NOT NULL CHECK (disposition IN ('undecided','accepted','rejected')),
  resurface_kind TEXT CHECK (resurface_kind IN ('date','after_entity')),
  resurface_date TEXT,
  resurface_ref TEXT,
  parent_id TEXT REFERENCES entities(id),
  claimed_actor TEXT,
  claimed_worktree TEXT,
  claimed_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  CHECK (id <> parent_id),
  CHECK ((progress = 'in_progress' AND claimed_actor IS NOT NULL AND
          claimed_worktree IS NOT NULL AND claimed_at IS NOT NULL) OR
         (progress <> 'in_progress' AND claimed_actor IS NULL AND
          claimed_worktree IS NULL AND claimed_at IS NULL)),
  CHECK ((resurface_kind IS NULL AND resurface_date IS NULL AND resurface_ref IS NULL) OR
         (resurface_kind = 'date' AND resurface_date IS NOT NULL AND resurface_ref IS NULL) OR
         (resurface_kind = 'after_entity' AND resurface_ref IS NOT NULL AND resurface_date IS NULL))
);

INSERT INTO entities (
  id, kind, title, description, progress, disposition,
  resurface_kind, resurface_date, resurface_ref, parent_id,
  claimed_actor, claimed_worktree, claimed_at, created_at, updated_at
)
SELECT id, 'issue', title, description, progress, disposition,
       CASE resurface_kind WHEN 'after_issue' THEN 'after_entity' ELSE resurface_kind END,
       resurface_date, resurface_ref, NULL,
       CASE WHEN progress = 'in_progress' THEN coalesce(claimed_actor, 'unknown') END,
       CASE WHEN progress = 'in_progress' THEN coalesce(claimed_worktree, 'unknown') END,
       CASE WHEN progress = 'in_progress' THEN coalesce(claimed_at, updated_at) END,
       created_at, updated_at
FROM issues;

CREATE TABLE entity_deps (
  entity_id TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
  depends_on_id TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
  PRIMARY KEY (entity_id, depends_on_id), CHECK (entity_id <> depends_on_id)
);
INSERT INTO entity_deps SELECT issue_id, depends_on_id FROM issue_deps;

CREATE TABLE entity_events (
  id INTEGER PRIMARY KEY,
  entity_id TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
  field TEXT NOT NULL, old_value TEXT, new_value TEXT,
  actor TEXT NOT NULL, reason TEXT, at TEXT NOT NULL
);
INSERT INTO entity_events
SELECT id, issue_id, field,
       replace(old_value, 'AfterIssue(', 'AfterEntity('),
       replace(new_value, 'AfterIssue(', 'AfterEntity('), actor, reason, at
FROM events;
CREATE INDEX idx_entity_events_entity ON entity_events (entity_id, id);

CREATE TABLE entity_progress_events (
  id INTEGER PRIMARY KEY,
  entity_id TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
  kind TEXT NOT NULL CHECK (kind IN ('start','done','release')),
  actor TEXT NOT NULL, reason TEXT, at TEXT NOT NULL
);
INSERT INTO entity_progress_events SELECT id, issue_id, kind, actor, reason, at
FROM progress_events;
CREATE INDEX idx_entity_progress_events_entity ON entity_progress_events (entity_id, id);

DROP TABLE group_deps;
DROP TABLE issue_deps;
DROP TABLE events;
DROP TABLE progress_events;
DROP TABLE issues;
DROP TABLE groups;
"#;

const FRESH_SCHEMA: &str = concat!(
    include_str!("db/schema_v12.sql"),
    include_str!("db/schema_causal.sql")
);

#[cfg(test)]
const MIGRATIONS: &[&str] = &[
    SCHEMA_V1, SCHEMA_V2, SCHEMA_V3, SCHEMA_V4, SCHEMA_V5, SCHEMA_V6, SCHEMA_V7, SCHEMA_V8,
];

fn configure(conn: &Connection) -> Result<()> {
    conn.execute_batch("PRAGMA foreign_keys = ON")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(())
}

fn initialize_schema(conn: &mut Connection) -> Result<()> {
    configure(conn)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute_batch(FRESH_SCHEMA)?;
    tx.execute(
        "INSERT INTO meta VALUES ('store_id',?1)",
        params![RecordId::new(RecordKind::Store)],
    )?;
    tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    tx.commit()?;
    Ok(())
}

fn require_current_schema(conn: &Connection) -> Result<()> {
    configure(conn)?;
    let found: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if found != SCHEMA_VERSION {
        return Err(DbError::UnsupportedSchema {
            found,
            expected: SCHEMA_VERSION,
        });
    }
    Ok(())
}

pub struct Store {
    conn: Connection,
    evaluation: Rc<Evaluation>,
}

pub struct ShowSnapshot {
    pub id: EntityId,
    pub view: View,
    pub decision_events: Vec<Event>,
    pub progress_events: Vec<ProgressEvent>,
    pub revisions: Vec<DeclarationRevision>,
    pub notes: Vec<Note>,
    pub counts: RecordCounts,
    pub causal: crate::history::CausalState,
}

pub struct RevisionSnapshot {
    pub entity: Entity,
    pub revisions: Vec<DeclarationRevision>,
}

impl RevisionSnapshot {
    pub fn revision(&self, number: &str) -> Result<&DeclarationRevision> {
        let matches: Vec<_> = self
            .revisions
            .iter()
            .filter(|r| r.id.matches(number))
            .collect();
        match matches.as_slice() {
            [record] => Ok(record),
            [] => Err(DbError::NoSuchRevision {
                id: self.entity.id.to_string(),
                number: number.into(),
            }),
            _ => Err(DbError::AmbiguousRecord(number.into())),
        }
    }
}

impl Store {
    fn from_conn(conn: Connection) -> Result<Self> {
        require_current_schema(&conn)?;
        debug_assert_eq!(
            conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .ok(),
            Some(SCHEMA_VERSION)
        );
        Ok(Self {
            conn,
            evaluation: Rc::new(Evaluation::new(std::env::current_dir()?)),
        })
    }

    pub fn init_at(path: &Path, prefix: &str) -> Result<()> {
        let mut conn = Connection::open(path)?;
        initialize_schema(&mut conn)?;
        conn.execute(
            "INSERT INTO meta (key,value) VALUES ('prefix',?1)",
            params![prefix],
        )?;
        conn.close().map_err(|(_, error)| error)?;
        Ok(())
    }

    pub fn open_at(
        path: &Path,
        evaluation: Rc<Evaluation>,
        report: impl FnOnce(&migration::Outcome),
    ) -> Result<Self> {
        let (conn, outcome) = migration::open(path)?;
        report(&outcome);
        let mut store = Self::from_conn(conn)?;
        store.evaluation = evaluation;
        Ok(store)
    }

    pub fn state(&self) -> Result<core::StateSnapshot> {
        let tx = self.conn.unchecked_transaction()?;
        let state = read_state(&tx, self.evaluation.clone())?;
        tx.commit()?;
        Ok(state)
    }

    #[cfg(test)]
    pub fn in_memory(prefix: &str) -> Result<Self> {
        let mut conn = Connection::open_in_memory()?;
        initialize_schema(&mut conn)?;
        let store = Self::from_conn(conn)?;
        store.conn.execute(
            "INSERT INTO meta (key, value) VALUES ('prefix', ?1)",
            params![prefix],
        )?;
        Ok(store)
    }

    #[cfg(test)]
    pub fn contract_step(
        &mut self,
        op: core::Operation,
        tick: u64,
    ) -> std::result::Result<(ApplyOutcome, core::StateSnapshot), String> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let before = read_state(&tx, self.evaluation.clone()).unwrap();
        let change = core::tests::execute(&before, op, tick).map_err(|e| e.to_string())?;
        publish(&tx, &before, &change).unwrap();
        let state = read_state(&tx, self.evaluation.clone()).unwrap();
        tx.commit().unwrap();
        Ok((change.outcome, state))
    }

    pub fn prefix(&self) -> Result<String> {
        Ok(self
            .conn
            .query_row("SELECT value FROM meta WHERE key = 'prefix'", [], |row| {
                row.get(0)
            })?)
    }

    #[cfg(test)]
    pub fn all(&self) -> Result<Vec<Entity>> {
        read_all(&self.conn)
    }

    #[cfg(test)]
    pub fn deps(&self) -> Result<Vec<(EntityId, EntityId)>> {
        read_deps(&self.conn)
    }

    pub fn view(&mut self) -> Result<View> {
        let tx = self.conn.transaction()?;
        let view = read_view(&tx, self.evaluation.clone())?;
        tx.commit()?;
        Ok(view)
    }

    pub fn status_snapshot(&mut self, group: Option<&str>) -> Result<(Option<EntityId>, View)> {
        let tx = self.conn.transaction()?;
        let id = group.map(|raw| resolve_id(&tx, raw)).transpose()?;
        let view = read_view(&tx, self.evaluation.clone())?;
        if let Some(id) = &id
            && view
                .get(id)
                .is_some_and(|entity| entity.kind != EntityKind::Group)
        {
            return Err(DbError::NotGroup(id.to_string()));
        }
        tx.commit()?;
        Ok((id, view))
    }

    pub fn snapshot(&mut self) -> Result<StoreSnapshot> {
        let tx = self.conn.transaction()?;
        let snapshot = read_snapshot(&tx, self.evaluation.clone())?;
        tx.commit()?;
        Ok(snapshot)
    }

    pub fn transactional_import<T, E, F>(
        &mut self,
        build: F,
    ) -> std::result::Result<(T, StoreSnapshot), E>
    where
        E: From<DbError>,
        F: FnOnce(StoreSnapshot) -> std::result::Result<(T, StoreSnapshot), E>,
    {
        let path = self.conn.path().unwrap_or(":memory:").to_string();
        let boundary = |source, phase, result| DbError::Boundary {
            path: path.clone(),
            phase,
            result,
            source: Box::new(source),
        };
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| boundary(DbError::from(e), "begin transaction", "Not applied"))?;
        let before = read_state(&tx, self.evaluation.clone()).map_err(E::from)?;
        let (value, desired) = build(before.declaration.clone())?;
        let result = before
            .execute(
                core::Operation::Import(desired),
                &mut core::Context {
                    at: Utc::now(),
                    evaluation: self.evaluation.clone(),
                    actor: "",
                    reason: None,
                    ids: &mut RecordId::new,
                },
            )
            .map_err(DbError::from)?;
        publish(&tx, &before, &result)
            .map_err(|e| boundary(e, "save transaction", "Not applied"))?;
        let after = read_snapshot(&tx, self.evaluation.clone()).map_err(E::from)?;
        tx.commit()
            .map_err(|e| boundary(DbError::from(e), "commit transaction", "Result unknown"))?;
        Ok((value, after))
    }

    pub fn resolve_id(&self, input: &str) -> Result<EntityId> {
        resolve_id(&self.conn, input)
    }

    pub fn get(&self, id: &EntityId) -> Result<Entity> {
        read_entity(&self.conn, id)
    }

    pub fn notes(&self, id: &EntityId) -> Result<Vec<Note>> {
        let tx = self.conn.unchecked_transaction()?;
        read_entity(&tx, id)?;
        let notes = read_state(&tx, self.evaluation.clone())?
            .histories
            .get(id)
            .cloned()
            .unwrap_or_default()
            .notes;
        tx.commit()?;
        Ok(notes)
    }

    pub fn note(&self, id: &EntityId, number: &str) -> Result<Note> {
        let tx = self.conn.unchecked_transaction()?;
        read_entity(&tx, id)?;
        let note = read_note(&tx, id, number)?;
        tx.commit()?;
        Ok(note)
    }

    pub fn add_note(&mut self, id: &EntityId, body: &str, actor: &str) -> Result<Note> {
        let result = self.mutate(
            core::Operation::AddNote {
                owner: id.clone(),
                body: body.into(),
            },
            &Ctx {
                actor: actor.into(),
                reason: None,
            },
        )?;
        Ok(result.state().histories[id]
            .notes
            .last()
            .expect("added Note")
            .clone())
    }

    #[cfg(test)]
    pub fn revisions(&self, id: &EntityId) -> Result<Vec<DeclarationRevision>> {
        read_entity(&self.conn, id)?;
        read_revisions(&self.conn, id)
    }

    pub fn revision_snapshot(&mut self, id: &EntityId) -> Result<RevisionSnapshot> {
        let tx = self.conn.transaction()?;
        let snapshot = read_revision_snapshot(&tx, id)?;
        tx.commit()?;
        Ok(snapshot)
    }

    fn mutate(&mut self, operation: core::Operation, ctx: &Ctx) -> Result<core::ValidatedChange> {
        let path = self.conn.path().unwrap_or(":memory:").to_string();
        let boundary = |source, phase, result| DbError::Boundary {
            path: path.clone(),
            phase,
            result,
            source: Box::new(source),
        };
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| boundary(DbError::from(e), "begin transaction", "Not applied"))?;
        let before = read_state(&tx, self.evaluation.clone())?;
        let result = before.execute(
            operation,
            &mut core::Context {
                at: Utc::now(),
                evaluation: self.evaluation.clone(),
                actor: &ctx.actor,
                reason: ctx.reason.as_deref(),
                ids: &mut RecordId::new,
            },
        )?;
        publish(&tx, &before, &result)
            .map_err(|e| boundary(e, "save transaction", "Not applied"))?;
        tx.commit()
            .map_err(|e| boundary(DbError::from(e), "commit transaction", "Result unknown"))?;
        Ok(result)
    }

    pub fn insert(&mut self, entity: &Entity) -> Result<()> {
        self.mutate(
            core::Operation::Insert(entity.clone()),
            &Ctx {
                actor: String::new(),
                reason: None,
            },
        )?;
        Ok(())
    }

    pub fn add_dep(&mut self, source: &EntityId, target: &EntityId) -> Result<ApplyOutcome> {
        self.dependency(source, target, true)
    }
    pub fn remove_dep(&mut self, source: &EntityId, target: &EntityId) -> Result<ApplyOutcome> {
        self.dependency(source, target, false)
    }
    fn dependency(
        &mut self,
        source: &EntityId,
        target: &EntityId,
        present: bool,
    ) -> Result<ApplyOutcome> {
        Ok(self
            .mutate(
                core::Operation::Dependency {
                    source: source.clone(),
                    target: target.clone(),
                    present,
                },
                &Ctx {
                    actor: String::new(),
                    reason: None,
                },
            )?
            .outcome)
    }
    pub fn apply(&mut self, id: &EntityId, change: Change, ctx: &Ctx) -> Result<ApplyOutcome> {
        Ok(self
            .mutate(core::Operation::Change(id.clone(), change), ctx)?
            .outcome)
    }

    pub fn events(&self, id: &EntityId) -> Result<Vec<Event>> {
        let tx = self.conn.unchecked_transaction()?;
        read_entity(&tx, id)?;
        let records = read_state(&tx, self.evaluation.clone())?
            .histories
            .get(id)
            .cloned()
            .unwrap_or_default()
            .decisions;
        tx.commit()?;
        Ok(records)
    }

    #[cfg(test)]
    pub fn progress_events(&self, id: &EntityId) -> Result<Vec<ProgressEvent>> {
        read_progress_events(&self.conn, id)
    }

    pub fn show_snapshot(&mut self, input: &str) -> Result<ShowSnapshot> {
        let tx = self.conn.transaction()?;
        let snapshot = read_show_snapshot(&tx, input, self.evaluation.clone())?;
        tx.commit()?;
        Ok(snapshot)
    }
}

fn write_entity(conn: &Connection, entity: &Entity) -> Result<()> {
    let claim = entity.progress.claim();
    conn.execute(
        "INSERT INTO entities (
          id,kind,title,description,progress,disposition,current_revision,
          resurface_kind,resurface_date,resurface_ref,parent_id,
          claimed_actor,claimed_worktree,claimed_at,created_at,updated_at,resurface_command
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
        params![
            entity.id.as_str(),
            entity.kind.as_db(),
            entity.title,
            entity.description,
            entity.progress.as_db(),
            entity.disposition.as_db(),
            entity.current_revision,
            entity.resurface_condition.kind_db(),
            resurface_date(&entity.resurface_condition),
            resurface_ref(&entity.resurface_condition),
            entity.parent.as_ref().map(EntityId::as_str),
            claim.map(|claim| claim.actor.as_str()),
            claim.map(|claim| claim.worktree.as_str()),
            claim.map(|claim| claim.at.to_rfc3339()),
            entity.created_at.to_rfc3339(),
            entity.updated_at.to_rfc3339(),
            resurface_command(&entity.resurface_condition),
        ],
    )?;
    Ok(())
}

fn write_revision(
    conn: &Connection,
    owner: &EntityId,
    revision: &DeclarationRevision,
) -> Result<()> {
    conn.execute(
        "INSERT INTO declaration_revisions
         (entity_id,revision,title,description,parent_id,created_at,baseline,sequence)
         VALUES (?1,?2,?3,?4,?5,?6,?7,(SELECT coalesce(max(sequence),0)+1 FROM declaration_revisions WHERE entity_id=?1))",
        params![owner.as_str(), revision.id, revision.title, revision.description,
            revision.parent.as_ref().map(EntityId::as_str), revision.created_at.to_rfc3339(), revision.baseline],
    )?;
    for dependency in &revision.dependencies {
        conn.execute("INSERT INTO revision_dependencies (entity_id,revision,depends_on_id) VALUES (?1,?2,?3)",
            params![owner.as_str(), revision.id, dependency.as_str()])?;
    }
    Ok(())
}

fn read_snapshot(conn: &Connection, evaluation: Rc<Evaluation>) -> Result<StoreSnapshot> {
    Ok(StoreSnapshot {
        evaluation,
        entities: read_all(conn)?,
        dependencies: read_deps(conn)?,
    })
}

fn write_dependency(
    conn: &Connection,
    source: &EntityId,
    target: &EntityId,
    present: bool,
) -> Result<()> {
    if present {
        conn.execute(
            "INSERT INTO entity_deps (entity_id, depends_on_id) VALUES (?1,?2)",
            params![source.as_str(), target.as_str()],
        )?;
    } else {
        conn.execute(
            "DELETE FROM entity_deps WHERE entity_id=?1 AND depends_on_id=?2",
            params![source.as_str(), target.as_str()],
        )?;
    }
    Ok(())
}

fn read_view(conn: &Connection, evaluation: Rc<Evaluation>) -> Result<View> {
    Ok(View::with_evaluation(
        read_all(conn)?,
        read_deps(conn)?,
        evaluation,
    ))
}

fn read_all(conn: &Connection) -> Result<Vec<Entity>> {
    let mut statement = conn.prepare(
        "SELECT id,kind,title,description,progress,disposition,current_revision,
         resurface_kind,resurface_date,resurface_ref,parent_id,
         claimed_actor,claimed_worktree,claimed_at,created_at,updated_at,resurface_command
         FROM entities ORDER BY created_at,id",
    )?;
    let rows = statement.query_map([], RawEntity::from_row)?;
    rows.map(|row| row?.into_entity()).collect()
}

fn read_entity(conn: &Connection, id: &EntityId) -> Result<Entity> {
    let raw = conn
        .query_row(
            "SELECT id,kind,title,description,progress,disposition,current_revision,
             resurface_kind,resurface_date,resurface_ref,parent_id,
             claimed_actor,claimed_worktree,claimed_at,created_at,updated_at,resurface_command
             FROM entities WHERE id=?1",
            params![id.as_str()],
            RawEntity::from_row,
        )
        .optional()?
        .ok_or_else(|| DbError::NoSuchEntity(id.to_string()))?;
    raw.into_entity()
}

fn read_deps(conn: &Connection) -> Result<Vec<(EntityId, EntityId)>> {
    let mut statement = conn.prepare(
        "SELECT entity_id,depends_on_id FROM entity_deps ORDER BY entity_id,depends_on_id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            EntityId::from_stored(&row.get::<_, String>(0)?),
            EntityId::from_stored(&row.get::<_, String>(1)?),
        ))
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

fn read_notes(conn: &Connection, id: &EntityId) -> Result<Vec<Note>> {
    let mut statement = conn.prepare(
        "SELECT note,body,actor,at,record_id FROM entity_notes WHERE entity_id=?1 ORDER BY note",
    )?;
    let rows = statement.query_map(params![id.as_str()], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, RecordId>(4)?,
        ))
    })?;
    rows.map(|row| {
        let (_sequence, body, actor, at, record_id) = row?;
        Ok(Note {
            id: record_id,
            body,
            actor,
            created_at: parse_time(&at)?,
        })
    })
    .collect()
}

fn read_note(conn: &Connection, id: &EntityId, number: &str) -> Result<Note> {
    let matches: Vec<_> = read_notes(conn, id)?
        .into_iter()
        .filter(|n| n.id.matches(number))
        .collect();
    match matches.len() {
        1 => Ok(matches.into_iter().next().unwrap()),
        0 => Err(DbError::NoSuchNote {
            id: id.to_string(),
            number: number.into(),
        }),
        _ => Err(DbError::AmbiguousRecord(number.into())),
    }
}

fn read_revision_dependencies(
    conn: &Connection,
    id: &EntityId,
    revision: RecordId,
) -> Result<Vec<EntityId>> {
    let mut statement = conn.prepare(
        "SELECT depends_on_id FROM revision_dependencies
         WHERE entity_id=?1 AND revision=?2 ORDER BY depends_on_id",
    )?;
    let rows = statement.query_map(params![id.as_str(), revision], |row| {
        row.get::<_, String>(0)
            .map(|value| EntityId::from_stored(&value))
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

fn read_revision(
    conn: &Connection,
    id: &EntityId,
    number: RecordId,
) -> Result<DeclarationRevision> {
    let row = conn
        .query_row(
            "SELECT title,description,parent_id,created_at,baseline,sequence
             FROM declaration_revisions WHERE entity_id=?1 AND revision=?2",
            params![id.as_str(), number],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, bool>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| DbError::NoSuchRevision {
            id: id.to_string(),
            number: number.to_string(),
        })?;
    Ok(DeclarationRevision {
        id: number,
        title: row.0,
        description: row.1,
        parent: row.2.as_deref().map(EntityId::from_stored),
        dependencies: read_revision_dependencies(conn, id, number)?,
        created_at: parse_time(&row.3)?,
        baseline: row.4,
    })
}

fn read_revisions(conn: &Connection, id: &EntityId) -> Result<Vec<DeclarationRevision>> {
    let mut statement = conn.prepare(
        "SELECT revision FROM declaration_revisions WHERE entity_id=?1 ORDER BY sequence",
    )?;
    let rows = statement.query_map(params![id.as_str()], |row| row.get::<_, RecordId>(0))?;
    let numbers = rows.collect::<std::result::Result<Vec<_>, _>>()?;
    numbers
        .into_iter()
        .map(|number| read_revision(conn, id, number))
        .collect()
}

fn read_revision_snapshot(conn: &Connection, id: &EntityId) -> Result<RevisionSnapshot> {
    let entity = read_entity(conn, id)?;
    let state = read_state(conn, Rc::new(Evaluation::new(PathBuf::from("."))))?;
    Ok(RevisionSnapshot {
        entity,
        revisions: state
            .histories
            .get(id)
            .cloned()
            .unwrap_or_default()
            .revisions,
    })
}

fn resolve_id(conn: &Connection, input: &str) -> Result<EntityId> {
    let mut statement =
        conn.prepare("SELECT id FROM entities WHERE id=?1 OR id LIKE '%' || ?1 ORDER BY id")?;
    let rows = statement.query_map(params![input], |row| row.get::<_, String>(0))?;
    let matches = rows.collect::<std::result::Result<Vec<_>, _>>()?;
    match matches.as_slice() {
        [id] => Ok(EntityId::from_stored(id)),
        [] => Err(DbError::NoSuchEntity(input.to_string())),
        candidates => Err(DbError::AmbiguousId {
            id: input.to_string(),
            candidates: candidates.join(", "),
        }),
    }
}

struct RawEntity {
    id: String,
    kind: String,
    title: String,
    description: Option<String>,
    progress: String,
    disposition: String,
    current_revision: Option<RecordId>,
    resurface_kind: Option<String>,
    resurface_date: Option<String>,
    resurface_ref: Option<String>,
    resurface_command: Option<String>,
    parent: Option<String>,
    claimed_actor: Option<String>,
    claimed_worktree: Option<String>,
    claimed_at: Option<String>,
    created_at: String,
    updated_at: String,
}

impl RawEntity {
    fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            kind: row.get(1)?,
            title: row.get(2)?,
            description: row.get(3)?,
            progress: row.get(4)?,
            disposition: row.get(5)?,
            current_revision: row.get(6)?,
            resurface_kind: row.get(7)?,
            resurface_date: row.get(8)?,
            resurface_ref: row.get(9)?,
            parent: row.get(10)?,
            claimed_actor: row.get(11)?,
            claimed_worktree: row.get(12)?,
            claimed_at: row.get(13)?,
            created_at: row.get(14)?,
            updated_at: row.get(15)?,
            resurface_command: row.get(16)?,
        })
    }

    fn into_entity(self) -> Result<Entity> {
        let claim = match (self.claimed_actor, self.claimed_worktree, self.claimed_at) {
            (Some(actor), Some(worktree), Some(at)) => Some(Claim {
                actor,
                worktree,
                at: parse_time(&at)?,
            }),
            (None, None, None) => None,
            _ => return Err(ParseError::MissingClaim.into()),
        };
        let disposition = Disposition::from_db(&self.disposition)?;
        if (disposition == Disposition::Undecided) != self.current_revision.is_none() {
            return Err(ParseError::Disposition(format!(
                "{} with current revision {:?}",
                disposition.label(),
                self.current_revision
            ))
            .into());
        }
        Ok(Entity {
            id: EntityId::from_stored(&self.id),
            kind: EntityKind::from_db(&self.kind)?,
            title: self.title,
            description: self.description,
            progress: Progress::from_db(&self.progress, claim)?,
            disposition,
            current_revision: self.current_revision,
            resurface_condition: ResurfaceCondition::from_db(
                self.resurface_kind.as_deref(),
                self.resurface_date.as_deref(),
                self.resurface_ref.as_deref(),
                self.resurface_command.as_deref(),
            )?,
            parent: self.parent.as_deref().map(EntityId::from_stored),
            created_at: parse_time(&self.created_at)?,
            updated_at: parse_time(&self.updated_at)?,
        })
    }
}

fn parse_time(value: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|time| time.with_timezone(&Utc))
        .map_err(|_| ParseError::ResurfaceCondition(format!("invalid timestamp: {value}")).into())
}

fn resurface_date(condition: &ResurfaceCondition) -> Option<String> {
    match condition {
        ResurfaceCondition::AtDate(date) => Some(date.to_string()),
        _ => None,
    }
}

fn resurface_command(condition: &ResurfaceCondition) -> Option<&str> {
    match condition {
        ResurfaceCondition::Command(script) => Some(script),
        _ => None,
    }
}

fn resurface_ref(condition: &ResurfaceCondition) -> Option<String> {
    match condition {
        ResurfaceCondition::AfterEntity(id) => Some(id.to_string()),
        _ => None,
    }
}

fn read_events(conn: &Connection, id: &EntityId) -> Result<Vec<Event>> {
    let mut statement = conn.prepare(
        "SELECT field,old_value,new_value,revision,actor,reason,at,record_id
         FROM entity_events WHERE entity_id=?1 ORDER BY id",
    )?;
    let rows = statement.query_map(params![id.as_str()], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<RecordId>>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, RecordId>(7)?,
        ))
    })?;
    rows.map(|row| {
        let (field, old_value, new_value, revision, actor, reason, at, record_id) = row?;
        Ok(Event {
            id: record_id,
            field,
            old_value,
            new_value,
            revision,
            actor,
            reason,
            at: parse_time(&at)?,
        })
    })
    .collect()
}

impl ProgressEventKind {
    fn as_db(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Done => "done",
            Self::Release => "release",
        }
    }

    fn from_db(value: &str) -> Result<Self> {
        match value {
            "start" => Ok(Self::Start),
            "done" => Ok(Self::Done),
            "release" => Ok(Self::Release),
            other => Err(DbError::InvalidProgressEvent(other.to_string())),
        }
    }
}

fn read_progress_events(conn: &Connection, id: &EntityId) -> Result<Vec<ProgressEvent>> {
    let mut statement = conn.prepare(
        "SELECT kind,actor,reason,at,record_id FROM entity_progress_events
         WHERE entity_id=?1 ORDER BY id",
    )?;
    let rows = statement.query_map(params![id.as_str()], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, RecordId>(4)?,
        ))
    })?;
    rows.map(|row| {
        let (kind, actor, reason, at, record_id) = row?;
        Ok(ProgressEvent {
            id: record_id,
            kind: ProgressEventKind::from_db(&kind)?,
            actor,
            reason,
            at: parse_time(&at)?,
        })
    })
    .collect()
}

fn read_show_snapshot(
    conn: &Connection,
    input: &str,
    evaluation: Rc<Evaluation>,
) -> Result<ShowSnapshot> {
    let id = resolve_id(conn, input)?;
    let state = read_state(conn, evaluation)?;
    let history = state.histories.get(&id).cloned().unwrap_or_default();
    let counts = RecordCounts {
        notes: history.notes.len(),
        revisions: history.revisions.len(),
        decisions: history.decisions.len(),
        progressions: history.progress.len(),
    };
    Ok(ShowSnapshot {
        id,
        view: state.declaration.view(),
        decision_events: history.decisions,
        progress_events: history.progress,
        revisions: history.revisions,
        notes: history.notes,
        counts,
        causal: state.causal,
    })
}

impl From<core::Error> for DbError {
    fn from(error: core::Error) -> Self {
        match error {
            core::Error::Evaluation(e) => Self::Evaluation(e),
            core::Error::NoSuchEntity(e) => Self::NoSuchEntity(e),
            core::Error::NotGroup(e) => Self::NotGroup(e),
            core::Error::CannotStart { id, fact } => Self::CannotStart { id, fact },
            core::Error::CannotProgress { id, action, fact } => {
                Self::CannotProgress { id, action, fact }
            }
            core::Error::Unchanged { id, field, current } => Self::Unchanged { id, field, current },
            core::Error::Containment(e) => Self::Containment(e),
            core::Error::Cycle { projection, path } => Self::Cycle { projection, path },
            core::Error::InvalidImport(e) => Self::InvalidImport(e),
            core::Error::EmptyNote => Self::EmptyNote,
            core::Error::DeclarationFixed(e) => Self::DeclarationFixed(e),
            core::Error::SelfDependency(e) => Self::SelfDependency(e),
            core::Error::InvalidState(e) => Self::InvalidSchema(e),
        }
    }
}

fn read_linear_state(conn: &Connection, evaluation: Rc<Evaluation>) -> Result<core::StateSnapshot> {
    let declaration = read_snapshot(conn, evaluation)?;
    let mut metadata = std::collections::BTreeMap::new();
    let mut statement = conn.prepare("SELECT key,value FROM meta ORDER BY key")?;
    for row in statement.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, rusqlite::types::Value>(1)?,
        ))
    })? {
        let (key, value) = row?;
        let value = match value {
            rusqlite::types::Value::Text(value) => core::MetadataValue::Text(value),
            rusqlite::types::Value::Integer(value) => core::MetadataValue::Integer(value),
            rusqlite::types::Value::Real(value) => core::MetadataValue::Real(value),
            rusqlite::types::Value::Blob(value) => core::MetadataValue::Bytes(value),
            rusqlite::types::Value::Null => {
                return Err(DbError::InvalidSchema("NULL metadata".into()));
            }
        };
        metadata.insert(key, value);
    }
    let mut histories = std::collections::BTreeMap::new();
    for entity in &declaration.entities {
        histories.insert(
            entity.id.clone(),
            core::History {
                revisions: read_revisions(conn, &entity.id)?,
                notes: read_notes(conn, &entity.id)?,
                decisions: read_events(conn, &entity.id)?,
                progress: read_progress_events(conn, &entity.id)?,
            },
        );
    }
    Ok(core::StateSnapshot {
        declaration,
        metadata,
        histories,
        causal: Default::default(),
    })
}
fn read_state(conn: &Connection, evaluation: Rc<Evaluation>) -> Result<core::StateSnapshot> {
    let mut state = read_linear_state(conn, evaluation)?;
    state.causal = read_causal(conn)?;
    state.validate_history()?;
    let ranks = state
        .causal
        .order()?
        .into_iter()
        .enumerate()
        .map(|(i, id)| (id, i))
        .collect();
    for history in state.histories.values_mut() {
        crate::codec::order_history(history, &ranks);
    }
    Ok(state)
}

fn publish(
    conn: &Connection,
    before: &core::StateSnapshot,
    change: &core::ValidatedChange,
) -> Result<()> {
    let after = change.state();
    debug_assert_eq!(before.metadata, after.metadata);
    let before_view = before.declaration.view();
    // Parents can be later in an import. Insert all owners before publishing relations.
    for entity in &after.declaration.entities {
        if before_view.get(&entity.id).is_none() {
            let mut initial = entity.clone();
            initial.parent = None;
            write_entity(conn, &initial)?;
        }
    }
    for entity in &after.declaration.entities {
        let current = before_view.get(&entity.id);
        if let Some(current) = current {
            update_entity(conn, current, entity)?;
        } else if entity.parent.is_some() {
            conn.execute(
                "UPDATE entities SET parent_id=?2 WHERE id=?1",
                params![
                    entity.id.as_str(),
                    entity.parent.as_ref().map(EntityId::as_str)
                ],
            )?;
        }
    }
    let old_edges: HashSet<_> = before.declaration.dependencies.iter().collect();
    let new_edges: HashSet<_> = after.declaration.dependencies.iter().collect();
    for (source, target) in old_edges.difference(&new_edges) {
        write_dependency(conn, source, target, false)?;
    }
    for (source, target) in new_edges.difference(&old_edges) {
        write_dependency(conn, source, target, true)?;
    }
    for (owner, history) in &after.histories {
        let empty = core::History::default();
        let old = before.histories.get(owner).unwrap_or(&empty);
        for revision in &history.revisions[old.revisions.len()..] {
            write_revision(conn, owner, revision)?;
        }
        for note in &history.notes[old.notes.len()..] {
            conn.execute("INSERT INTO entity_notes (entity_id,note,body,actor,at,record_id)
                VALUES (?1,(SELECT coalesce(max(note),0)+1 FROM entity_notes WHERE entity_id=?1),?2,?3,?4,?5)",
                params![owner.as_str(), note.body, note.actor, note.created_at.to_rfc3339(), note.id])?;
        }
        for event in &history.decisions[old.decisions.len()..] {
            conn.execute("INSERT INTO entity_events (entity_id,field,old_value,new_value,revision,actor,reason,at,record_id)
                VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                params![owner.as_str(), event.field, event.old_value, event.new_value, event.revision, event.actor, event.reason, event.at.to_rfc3339(), event.id])?;
        }
        for event in &history.progress[old.progress.len()..] {
            conn.execute("INSERT INTO entity_progress_events (entity_id,kind,actor,reason,at,record_id) VALUES (?1,?2,?3,?4,?5,?6)",
                params![owner.as_str(), event.kind.as_db(), event.actor, event.reason, event.at.to_rfc3339(), event.id])?;
        }
    }
    publish_causal(conn, &before.causal, &after.causal)?;
    Ok(())
}

fn update_entity(conn: &Connection, before: &Entity, after: &Entity) -> Result<()> {
    use rusqlite::types::Value;
    let mut columns = Vec::new();
    let mut values = vec![Value::Text(after.id.to_string())];
    let mut set = |column: &'static str, value: Value| {
        values.push(value);
        columns.push(format!("{column}=?{}", values.len()));
    };
    fn optional(value: Option<String>) -> Value {
        value.map(Value::Text).unwrap_or(Value::Null)
    }
    if before.title != after.title {
        set("title", Value::Text(after.title.clone()));
    }
    if before.description != after.description {
        set("description", optional(after.description.clone()));
    }
    if before.parent != after.parent {
        set(
            "parent_id",
            optional(after.parent.as_ref().map(ToString::to_string)),
        );
    }
    if before.progress != after.progress {
        let claim = after.progress.claim();
        set("progress", Value::Text(after.progress.as_db().into()));
        set("claimed_actor", optional(claim.map(|c| c.actor.clone())));
        set(
            "claimed_worktree",
            optional(claim.map(|c| c.worktree.clone())),
        );
        set("claimed_at", optional(claim.map(|c| c.at.to_rfc3339())));
    }
    if before.disposition != after.disposition || before.current_revision != after.current_revision
    {
        set("disposition", Value::Text(after.disposition.as_db().into()));
        set(
            "current_revision",
            optional(after.current_revision.map(|id| id.to_string())),
        );
    }
    if before.resurface_condition != after.resurface_condition {
        let condition = &after.resurface_condition;
        set(
            "resurface_kind",
            optional(condition.kind_db().map(str::to_string)),
        );
        set("resurface_date", optional(resurface_date(condition)));
        set("resurface_ref", optional(resurface_ref(condition)));
        set(
            "resurface_command",
            optional(resurface_command(condition).map(str::to_string)),
        );
    }
    if before.updated_at != after.updated_at {
        set("updated_at", Value::Text(after.updated_at.to_rfc3339()));
    }
    if !columns.is_empty() {
        conn.execute(
            &format!("UPDATE entities SET {} WHERE id=?1", columns.join(",")),
            rusqlite::params_from_iter(values),
        )?;
    }
    Ok(())
}

fn read_causal(conn: &Connection) -> Result<crate::history::CausalState> {
    fn rows<K: std::str::FromStr + Ord, V: serde::de::DeserializeOwned>(
        conn: &Connection,
        table: &str,
        key: &str,
    ) -> Result<std::collections::BTreeMap<K, V>> {
        let mut stmt = conn.prepare(&format!("SELECT {key},payload FROM {table}"))?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        rows.map(|r| {
            let (key, payload) = r?;
            Ok((
                key.parse()
                    .map_err(|_| DbError::InvalidSchema("invalid causal key".into()))?,
                serde_json::from_str(&payload)
                    .map_err(|e| DbError::InvalidSchema(e.to_string()))?,
            ))
        })
        .collect()
    }
    let mut owners = std::collections::BTreeMap::new();
    let mut stmt = conn.prepare("SELECT entity_id,payload FROM history_lineage")?;
    for r in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (id, payload) = r?;
        owners.insert(
            EntityId::from_stored(&id),
            serde_json::from_str(&payload).map_err(|e| DbError::InvalidSchema(e.to_string()))?,
        );
    }
    Ok(crate::history::CausalState {
        owners,
        links: rows(conn, "causal_links", "record_id")?,
        baselines: rows(conn, "history_baselines", "record_id")?,
        merges: rows(conn, "history_merges", "record_id")?,
    })
}
fn publish_causal(
    conn: &Connection,
    before: &crate::history::CausalState,
    after: &crate::history::CausalState,
) -> Result<()> {
    fn append<V: serde::Serialize>(
        conn: &Connection,
        table: &str,
        old: &std::collections::BTreeMap<RecordId, V>,
        new: &std::collections::BTreeMap<RecordId, V>,
    ) -> Result<()> {
        for (id, value) in new.iter().filter(|(id, _)| !old.contains_key(id)) {
            let payload =
                serde_json::to_string(value).map_err(|e| DbError::InvalidSchema(e.to_string()))?;
            conn.execute(
                &format!("INSERT INTO {table}(record_id,payload) VALUES (?1,?2)"),
                params![id, payload],
            )?;
        }
        Ok(())
    }
    append(conn, "causal_links", &before.links, &after.links)?;
    append(
        conn,
        "history_baselines",
        &before.baselines,
        &after.baselines,
    )?;
    append(conn, "history_merges", &before.merges, &after.merges)?;
    for (id, lineage) in &after.owners {
        if before.owners.get(id) != Some(lineage) {
            let payload = serde_json::to_string(lineage)
                .map_err(|e| DbError::InvalidSchema(e.to_string()))?;
            conn.execute("INSERT INTO history_lineage(entity_id,payload) VALUES (?1,?2) ON CONFLICT(entity_id) DO UPDATE SET payload=excluded.payload",params![id.as_str(),payload])?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::hooks::{AuthAction, Authorization};
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    };

    fn id(value: &str) -> EntityId {
        EntityId::from_stored(value)
    }

    fn entity(value: &str, kind: EntityKind, parent: Option<&str>) -> Entity {
        let now = Utc::now();
        Entity {
            id: id(value),
            kind,
            title: value.to_string(),
            description: None,
            progress: Progress::NotStarted,
            disposition: Disposition::Accepted,
            current_revision: Some(RecordId::new(RecordKind::Revision)),
            resurface_condition: ResurfaceCondition::Always,
            parent: parent.map(id),
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

    fn ctx() -> Ctx {
        Ctx {
            actor: "tester".to_string(),
            reason: Some("reason".to_string()),
        }
    }

    #[test]
    fn sqlite_failure_rolls_back_every_imported_entity() {
        let mut store = Store::in_memory("test").unwrap();
        store.conn.execute_batch("CREATE TRIGGER reject_import BEFORE INSERT ON entities WHEN NEW.title = 'second' BEGIN SELECT RAISE(ABORT, 'injected import failure'); END;").unwrap();
        let result = store.transactional_import(|mut desired| {
            desired.entities = vec![
                entity("first", EntityKind::Issue, None),
                entity("second", EntityKind::Issue, None),
            ];
            Ok::<_, DbError>(((), desired))
        });
        assert!(
            matches!(result, Err(DbError::Boundary { result: "Not applied", ref source, .. }) if source.to_string().contains("injected import failure"))
        );
        assert!(store.all().unwrap().is_empty());
        let revisions: i64 = store
            .conn
            .query_row("SELECT count(*) FROM declaration_revisions", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(revisions, 0);
    }

    #[test]
    fn manual_storage_requires_no_payload_and_round_trips() {
        let mut store = Store::in_memory("test").unwrap();
        store.insert(&entity("m", EntityKind::Issue, None)).unwrap();
        for payload in [
            "resurface_date='2026-09-05'",
            "resurface_ref='m'",
            "resurface_command='exit 0'",
        ] {
            assert!(
                store
                    .conn
                    .execute(
                        &format!(
                            "UPDATE entities SET resurface_kind='manual',{payload} WHERE id='m'"
                        ),
                        []
                    )
                    .is_err()
            );
        }
        store
            .apply(
                &id("m"),
                Change::SetResurfaceCondition(ResurfaceCondition::Manual),
                &ctx(),
            )
            .unwrap();
        assert_eq!(
            store.get(&id("m")).unwrap().resurface_condition,
            ResurfaceCondition::Manual
        );
    }

    #[test]
    fn command_storage_rejects_mixed_fields_and_round_trips() {
        let mut store = Store::in_memory("test").unwrap();
        store.insert(&entity("c", EntityKind::Issue, None)).unwrap();
        for update in [
            "resurface_command='exit 0'",
            "resurface_kind='command'",
            "resurface_kind='command',resurface_command='exit 0',resurface_ref='c'",
            "resurface_kind='date',resurface_date='2026-09-05',resurface_command='exit 0'",
        ] {
            assert!(
                store
                    .conn
                    .execute(&format!("UPDATE entities SET {update} WHERE id='c'"), [])
                    .is_err()
            );
        }
        store
            .apply(
                &id("c"),
                Change::SetResurfaceCondition(ResurfaceCondition::Command("exit 0".into())),
                &ctx(),
            )
            .unwrap();
        assert_eq!(
            store.get(&id("c")).unwrap().resurface_condition,
            ResurfaceCondition::Command("exit 0".into())
        );
    }

    #[test]
    fn issues_and_groups_share_ids_state_and_dependencies() {
        let mut store = Store::in_memory("t").unwrap();
        store
            .insert(&entity("t-g", EntityKind::Group, None))
            .unwrap();
        store
            .insert(&entity("t-i", EntityKind::Issue, None))
            .unwrap();
        store
            .apply(&id("t-g"), Change::Decide(Disposition::Undecided), &ctx())
            .unwrap();
        store.add_dep(&id("t-g"), &id("t-i")).unwrap();
        assert_eq!(store.all().unwrap().len(), 2);
        assert_eq!(store.deps().unwrap(), [(id("t-g"), id("t-i"))]);
        assert_eq!(store.resolve_id("g").unwrap(), id("t-g"));
    }

    #[test]
    fn parent_gate_and_group_completion_are_enforced_atomically() {
        let mut store = Store::in_memory("t").unwrap();
        store.insert(&entity("g", EntityKind::Group, None)).unwrap();
        store
            .insert(&entity("i", EntityKind::Issue, Some("g")))
            .unwrap();
        assert!(matches!(
            store.apply(&id("i"), Change::Start(claim()), &ctx()),
            Err(DbError::CannotStart { .. })
        ));
        store
            .apply(&id("g"), Change::Start(claim()), &ctx())
            .unwrap();
        store
            .apply(&id("i"), Change::Start(claim()), &ctx())
            .unwrap();
        assert!(matches!(
            store.apply(&id("g"), Change::Done, &ctx()),
            Err(DbError::CannotProgress { .. })
        ));
        store.apply(&id("i"), Change::Done, &ctx()).unwrap();
        store.apply(&id("g"), Change::Done, &ctx()).unwrap();
    }

    #[test]
    fn group_release_waits_for_in_progress_descendants() {
        let mut store = Store::in_memory("t").unwrap();
        store.insert(&entity("g", EntityKind::Group, None)).unwrap();
        store
            .insert(&entity("i", EntityKind::Issue, Some("g")))
            .unwrap();
        store
            .apply(&id("g"), Change::Start(claim()), &ctx())
            .unwrap();
        store
            .apply(&id("i"), Change::Start(claim()), &ctx())
            .unwrap();
        assert!(store.apply(&id("g"), Change::Release, &ctx()).is_err());
        store.apply(&id("i"), Change::Release, &ctx()).unwrap();
        store.apply(&id("g"), Change::Release, &ctx()).unwrap();
    }

    #[test]
    fn cross_relation_cycles_are_rejected() {
        let mut store = Store::in_memory("t").unwrap();
        store.insert(&entity("g", EntityKind::Group, None)).unwrap();
        store
            .insert(&entity("i", EntityKind::Issue, Some("g")))
            .unwrap();
        store
            .apply(&id("g"), Change::Decide(Disposition::Undecided), &ctx())
            .unwrap();
        let error = store.add_dep(&id("g"), &id("i")).unwrap_err();
        assert!(matches!(error, DbError::Cycle { .. }));
        assert!(store.deps().unwrap().is_empty());
    }

    #[test]
    fn ended_group_structure_is_fixed() {
        let mut store = Store::in_memory("t").unwrap();
        store.insert(&entity("g", EntityKind::Group, None)).unwrap();
        store
            .apply(&id("g"), Change::Start(claim()), &ctx())
            .unwrap();
        store.apply(&id("g"), Change::Done, &ctx()).unwrap();
        store
            .insert(&entity("other", EntityKind::Group, None))
            .unwrap();
        assert!(
            store
                .apply(&id("g"), Change::SetParent(Some(id("other"))), &ctx())
                .is_err()
        );
        let child = entity("i", EntityKind::Issue, None);
        store.insert(&child).unwrap();
        assert!(
            store
                .apply(&id("i"), Change::SetParent(Some(id("g"))), &ctx())
                .is_err()
        );
    }

    #[test]
    fn decisions_and_progress_have_separate_histories_for_groups() {
        let mut store = Store::in_memory("t").unwrap();
        store.insert(&entity("g", EntityKind::Group, None)).unwrap();
        store
            .apply(&id("g"), Change::Decide(Disposition::Rejected), &ctx())
            .unwrap();
        assert_eq!(store.events(&id("g")).unwrap().len(), 1);
        assert!(store.progress_events(&id("g")).unwrap().is_empty());
    }

    #[test]
    fn decided_entities_have_an_initial_revision_but_drafts_do_not() {
        let mut store = Store::in_memory("t").unwrap();
        store
            .insert(&entity("accepted", EntityKind::Issue, None))
            .unwrap();
        let mut draft = entity("draft", EntityKind::Issue, None);
        draft.disposition = Disposition::Undecided;
        draft.current_revision = None;
        store.insert(&draft).unwrap();

        let accepted = store.get(&id("accepted")).unwrap();
        assert_eq!(
            accepted.current_revision,
            Some(store.revisions(&accepted.id).unwrap()[0].id)
        );
        assert_eq!(store.revisions(&accepted.id).unwrap().len(), 1);
        assert_eq!(store.get(&id("draft")).unwrap().current_revision, None);
        assert!(store.revisions(&id("draft")).unwrap().is_empty());
    }

    #[test]
    fn declaration_edits_require_undecided_and_redecisions_point_to_revisions() {
        let mut store = Store::in_memory("t").unwrap();
        store.insert(&entity("i", EntityKind::Issue, None)).unwrap();
        let before = store.get(&id("i")).unwrap();

        assert_eq!(
            store
                .apply(&id("i"), Change::SetTitle(before.title.clone()), &ctx(),)
                .unwrap(),
            ApplyOutcome::Unchanged
        );
        assert!(matches!(
            store.apply(&id("i"), Change::SetTitle("changed".into()), &ctx()),
            Err(DbError::DeclarationFixed(_))
        ));
        assert_eq!(store.get(&id("i")).unwrap().updated_at, before.updated_at);

        store
            .apply(&id("i"), Change::Decide(Disposition::Undecided), &ctx())
            .unwrap();
        assert_eq!(store.get(&id("i")).unwrap().current_revision, None);
        assert_eq!(store.revisions(&id("i")).unwrap().len(), 1);
        store
            .apply(&id("i"), Change::SetTitle("changed".into()), &ctx())
            .unwrap();
        store
            .apply(&id("i"), Change::Decide(Disposition::Accepted), &ctx())
            .unwrap();
        assert_eq!(
            store.get(&id("i")).unwrap().current_revision,
            Some(store.revisions(&id("i")).unwrap()[1].id)
        );
        assert_eq!(store.revisions(&id("i")).unwrap().len(), 2);
        assert_eq!(
            store.events(&id("i")).unwrap().last().unwrap().revision,
            Some(store.revisions(&id("i")).unwrap()[1].id)
        );

        store
            .apply(&id("i"), Change::Decide(Disposition::Undecided), &ctx())
            .unwrap();
        store
            .apply(&id("i"), Change::Decide(Disposition::Rejected), &ctx())
            .unwrap();
        assert_eq!(
            store.get(&id("i")).unwrap().current_revision,
            Some(store.revisions(&id("i")).unwrap()[1].id)
        );
        assert_eq!(store.revisions(&id("i")).unwrap().len(), 2);
    }

    #[test]
    fn notes_are_append_only_numbered_and_reject_blank_bodies() {
        let mut store = Store::in_memory("t").unwrap();
        store.insert(&entity("i", EntityKind::Issue, None)).unwrap();
        let first = store.add_note(&id("i"), "first", "alice").unwrap();
        let second = store.add_note(&id("i"), "first", "alice").unwrap();
        assert_ne!(first.id, second.id);
        assert_eq!(store.note(&id("i"), &first.id.to_string()).unwrap(), first);
        assert_eq!(store.notes(&id("i")).unwrap(), [first, second]);
        assert!(matches!(
            store.add_note(&id("i"), " \n\t", "alice"),
            Err(DbError::EmptyNote)
        ));
    }

    #[test]
    fn declaration_guards_and_notes_cover_every_kind_progress_and_disposition() {
        let mut store = Store::in_memory("matrix").unwrap();
        let kinds = [EntityKind::Issue, EntityKind::Group];
        let progresses = ["not-started", "in-progress", "ended"];
        let dispositions = [
            Disposition::Undecided,
            Disposition::Accepted,
            Disposition::Rejected,
        ];

        for (kind_index, kind) in kinds.into_iter().enumerate() {
            for (progress_index, progress) in progresses.into_iter().enumerate() {
                for (disposition_index, disposition) in dispositions.into_iter().enumerate() {
                    let value = format!("m-{kind_index}{progress_index}{disposition_index}");
                    let mut candidate = entity(&value, kind, None);
                    candidate.progress = match progress {
                        "not-started" => Progress::NotStarted,
                        "in-progress" => Progress::InProgress(claim()),
                        "ended" => Progress::Ended,
                        _ => unreachable!(),
                    };
                    candidate.disposition = disposition;
                    candidate.current_revision = (disposition != Disposition::Undecided)
                        .then(|| RecordId::new(RecordKind::Revision));
                    store.insert(&candidate).unwrap();

                    store
                        .add_note(&candidate.id, "matrix note", "tester")
                        .unwrap();
                    assert_eq!(store.notes(&candidate.id).unwrap().len(), 1);

                    let changed_title = format!("{value}-changed");
                    if disposition == Disposition::Undecided {
                        store
                            .apply(
                                &candidate.id,
                                Change::SetTitle(changed_title.clone()),
                                &ctx(),
                            )
                            .unwrap();
                        store
                            .apply(&candidate.id, Change::Decide(Disposition::Accepted), &ctx())
                            .unwrap();
                        assert_eq!(store.revisions(&candidate.id).unwrap().len(), 1);
                    } else {
                        assert!(matches!(
                            store.apply(&candidate.id, Change::SetTitle(changed_title), &ctx()),
                            Err(DbError::DeclarationFixed(_))
                        ));
                        assert_eq!(store.revisions(&candidate.id).unwrap().len(), 1);
                    }
                }
            }
        }
    }

    #[test]
    fn sqlite_runs_the_core_contract_and_publishes_identical_state() {
        let mut store = Store::in_memory("t").unwrap();
        let mut tick = 0;
        core::tests::contract(|operation| {
            tick += 1;
            let tx = store
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .unwrap();
            let before = read_state(&tx, store.evaluation.clone()).unwrap();
            let result =
                core::tests::execute(&before, operation, tick).map_err(|e| e.to_string())?;
            publish(&tx, &before, &result).unwrap();
            let after = read_state(&tx, store.evaluation.clone()).unwrap();
            let expected = result.state();
            let mut actual_entities = after.declaration.entities.clone();
            let mut expected_entities = expected.declaration.entities.clone();
            actual_entities.sort_by(|a, b| a.id.cmp(&b.id));
            expected_entities.sort_by(|a, b| a.id.cmp(&b.id));
            assert_eq!(actual_entities, expected_entities);
            assert_eq!(
                after.declaration.dependencies,
                expected.declaration.dependencies
            );
            for entity in &after.declaration.entities {
                assert_eq!(
                    after.histories[&entity.id],
                    expected
                        .histories
                        .get(&entity.id)
                        .cloned()
                        .unwrap_or_default()
                );
            }
            assert_eq!(after.metadata, before.metadata);
            tx.commit().unwrap();
            Ok((result.outcome, after))
        });
    }

    #[test]
    fn sqlite_preserves_branches_and_continues_from_the_merge_head() {
        let mut store = Store::in_memory("t").unwrap();
        let tx = store
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let before = read_state(&tx, store.evaluation.clone()).unwrap();
        let mut branch = crate::history::tests::branching();
        branch.metadata = before.metadata.clone();
        let result = core::tests::execute(
            &branch,
            core::Operation::AddNote {
                owner: id("i"),
                body: "persist branches".into(),
            },
            20,
        )
        .unwrap();
        publish(&tx, &before, &result).unwrap();
        tx.commit().unwrap();
        let selected = store.get(&id("i")).unwrap().current_revision;
        store
            .apply(&id("i"), Change::Decide(Disposition::Undecided), &ctx())
            .unwrap();
        store
            .apply(&id("i"), Change::Decide(Disposition::Accepted), &ctx())
            .unwrap();
        let snapshot = store.show_snapshot("i").unwrap();
        assert_eq!(
            snapshot.view.get(&id("i")).unwrap().current_revision,
            selected
        );
        assert_eq!(snapshot.revisions.len(), 2);
        assert_eq!(snapshot.notes.len(), 3);
        assert!(
            snapshot
                .causal
                .display(&id("i"))
                .contains("Merge selected input")
        );
    }

    #[test]
    fn publication_failure_rolls_back_control_revision_and_history() {
        let mut store = Store::in_memory("t").unwrap();
        store
            .insert(&core::tests::entity("i", EntityKind::Issue, None))
            .unwrap();
        store
            .conn
            .execute_batch(
                "CREATE TEMP TRIGGER reject_decision BEFORE INSERT ON entity_events
            BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
            )
            .unwrap();
        assert!(matches!(
            store.apply(&id("i"), Change::Decide(Disposition::Accepted), &ctx()),
            Err(DbError::Boundary {
                result: "Not applied",
                ..
            })
        ));
        assert_eq!(
            store.get(&id("i")).unwrap().disposition,
            Disposition::Undecided
        );
        assert!(store.revisions(&id("i")).unwrap().is_empty());
        assert!(store.events(&id("i")).unwrap().is_empty());
        store
            .conn
            .execute_batch("DROP TRIGGER reject_decision")
            .unwrap();
        store
            .apply(&id("i"), Change::Decide(Disposition::Accepted), &ctx())
            .unwrap();
        store
            .conn
            .execute_batch(
                "CREATE TEMP TRIGGER reject_progress BEFORE INSERT ON entity_progress_events
            BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
            )
            .unwrap();
        assert!(
            store
                .apply(&id("i"), Change::Start(claim()), &ctx())
                .is_err()
        );
        assert_eq!(store.get(&id("i")).unwrap().progress, Progress::NotStarted);
        assert!(store.progress_events(&id("i")).unwrap().is_empty());
    }

    #[test]
    fn unrelated_writes_preserve_legacy_bytes_metadata_and_sequence_gaps() {
        let mut store = Store::in_memory("t").unwrap();
        store
            .insert(&core::tests::entity("i", EntityKind::Issue, None))
            .unwrap();
        let note = store.add_note(&id("i"), "old", "legacy").unwrap();
        store
            .conn
            .execute_batch(
                "UPDATE entities SET created_at='2026-01-02T03:04:05.000Z';
            UPDATE entity_notes SET note=17, at='2020-01-02T03:04:05.000Z';
            INSERT INTO meta VALUES ('custom',x'00ff');",
            )
            .unwrap();
        store
            .apply(&id("i"), Change::SetTitle("updated".into()), &ctx())
            .unwrap();
        store.add_note(&id("i"), "new", "new").unwrap();
        assert_eq!(
            store
                .conn
                .query_row("SELECT created_at FROM entities WHERE id='i'", [], |r| {
                    r.get::<_, String>(0)
                })
                .unwrap(),
            "2026-01-02T03:04:05.000Z"
        );
        assert_eq!(
            store
                .conn
                .query_row(
                    "SELECT at FROM entity_notes WHERE record_id=?1",
                    [note.id],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
            "2020-01-02T03:04:05.000Z"
        );
        assert_eq!(
            store
                .conn
                .query_row("SELECT max(note) FROM entity_notes", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            18
        );
        assert_eq!(
            store
                .conn
                .query_row("SELECT value FROM meta WHERE key='custom'", [], |r| r
                    .get::<_, Vec<u8>>(0))
                .unwrap(),
            [0, 255]
        );
    }

    #[test]
    fn concurrent_note_additions_do_not_overwrite_each_other() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "axon-note-concurrency-{}-{nonce}.db",
            std::process::id()
        ));
        let mut connection = Connection::open(&path).unwrap();
        initialize_schema(&mut connection).unwrap();
        let mut setup = Store::from_conn(connection).unwrap();
        setup.insert(&entity("i", EntityKind::Issue, None)).unwrap();
        drop(setup);

        let threads = (0..8)
            .map(|index| {
                let path = path.clone();
                std::thread::spawn(move || {
                    let mut store = Store::from_conn(Connection::open(path).unwrap()).unwrap();
                    store
                        .add_note(&id("i"), &format!("note {index}"), "worker")
                        .unwrap()
                })
            })
            .collect::<Vec<_>>();
        let records = threads
            .into_iter()
            .map(|thread| thread.join().unwrap().id)
            .collect::<Vec<_>>();
        assert_eq!(records.into_iter().collect::<HashSet<_>>().len(), 8);

        let store = Store::from_conn(Connection::open(&path).unwrap()).unwrap();
        assert_eq!(store.notes(&id("i")).unwrap().len(), 8);
        drop(store);
        for candidate in [
            path.clone(),
            PathBuf::from(format!("{}-wal", path.display())),
            PathBuf::from(format!("{}-shm", path.display())),
        ] {
            let _ = std::fs::remove_file(candidate);
        }
    }

    #[test]
    fn show_snapshot_keeps_state_and_history_at_one_point_in_time() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "axon-show-snapshot-{}-{nonce}.db",
            std::process::id()
        ));
        let mut connection = Connection::open(&path).unwrap();
        connection
            .execute_batch("PRAGMA journal_mode = WAL")
            .unwrap();
        initialize_schema(&mut connection).unwrap();
        let mut reader = Store::from_conn(connection).unwrap();
        reader
            .insert(&entity("t-1", EntityKind::Issue, None))
            .unwrap();
        let mut writer = Store::from_conn(Connection::open(&path).unwrap()).unwrap();

        let tx = reader.conn.transaction().unwrap();
        assert_eq!(read_all(&tx).unwrap().len(), 1);
        writer
            .apply(&id("t-1"), Change::Start(claim()), &ctx())
            .unwrap();
        let snapshot = read_show_snapshot(&tx, "t-1", reader.evaluation.clone()).unwrap();
        assert_eq!(
            snapshot.view.get(&snapshot.id).unwrap().progress,
            Progress::NotStarted
        );
        assert!(snapshot.progress_events.is_empty());
        tx.commit().unwrap();

        let fresh = reader.show_snapshot("t-1").unwrap();
        assert!(matches!(
            fresh.view.get(&fresh.id).unwrap().progress,
            Progress::InProgress(_)
        ));
        assert_eq!(fresh.progress_events.len(), 1);

        drop(reader);
        drop(writer);
        for candidate in [
            path.clone(),
            PathBuf::from(format!("{}-wal", path.display())),
            PathBuf::from(format!("{}-shm", path.display())),
        ] {
            let _ = std::fs::remove_file(candidate);
        }
    }

    #[test]
    fn revision_snapshot_keeps_current_marker_and_rows_at_one_point_in_time() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "axon-revision-snapshot-{}-{nonce}.db",
            std::process::id()
        ));
        let mut connection = Connection::open(&path).unwrap();
        connection
            .execute_batch("PRAGMA journal_mode = WAL")
            .unwrap();
        initialize_schema(&mut connection).unwrap();
        let mut reader = Store::from_conn(connection).unwrap();
        reader
            .insert(&entity("i", EntityKind::Issue, None))
            .unwrap();
        let writer_path = path.clone();
        let write_attempted = Arc::new(AtomicBool::new(false));
        let write_error = Arc::new(Mutex::new(None));
        let attempted_from_hook = Arc::clone(&write_attempted);
        let error_from_hook = Arc::clone(&write_error);

        reader
            .conn
            .authorizer(Some(move |context: rusqlite::hooks::AuthContext<'_>| {
                if matches!(
                    context.action,
                    AuthAction::Read {
                        table_name: "declaration_revisions",
                        ..
                    }
                ) && !attempted_from_hook.swap(true, Ordering::SeqCst)
                    && let Err(error) = (|| -> Result<()> {
                        let mut writer = Store::from_conn(Connection::open(&writer_path)?)?;
                        writer.apply(&id("i"), Change::Decide(Disposition::Undecided), &ctx())?;
                        writer.apply(&id("i"), Change::SetTitle("changed".into()), &ctx())?;
                        writer.apply(&id("i"), Change::Decide(Disposition::Accepted), &ctx())?;
                        Ok(())
                    })()
                {
                    *error_from_hook.lock().unwrap() = Some(error.to_string());
                }
                Authorization::Allow
            }))
            .unwrap();

        let snapshot = reader.revision_snapshot(&id("i")).unwrap();
        assert!(write_attempted.load(Ordering::SeqCst));
        assert_eq!(write_error.lock().unwrap().as_deref(), None);
        assert_eq!(
            snapshot.entity.current_revision,
            Some(snapshot.revisions[0].id)
        );
        assert_eq!(snapshot.revisions.len(), 1);

        let fresh = reader.revision_snapshot(&id("i")).unwrap();
        assert_eq!(fresh.entity.current_revision, Some(fresh.revisions[1].id));
        assert_eq!(fresh.revisions.len(), 2);

        drop(reader);
        for candidate in [
            path.clone(),
            PathBuf::from(format!("{}-wal", path.display())),
            PathBuf::from(format!("{}-shm", path.display())),
        ] {
            let _ = std::fs::remove_file(candidate);
        }
    }

    #[test]
    fn view_keeps_entities_and_dependencies_at_one_point_in_time() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "axon-view-snapshot-{}-{nonce}.db",
            std::process::id()
        ));
        let mut connection = Connection::open(&path).unwrap();
        connection
            .execute_batch("PRAGMA journal_mode = WAL")
            .unwrap();
        initialize_schema(&mut connection).unwrap();
        let mut reader = Store::from_conn(connection).unwrap();
        reader
            .insert(&entity("a", EntityKind::Issue, None))
            .unwrap();
        reader
            .insert(&entity("b", EntityKind::Issue, None))
            .unwrap();
        let writer = Store::from_conn(Connection::open(&path).unwrap()).unwrap();
        let write_attempted = Arc::new(AtomicBool::new(false));
        let write_error = Arc::new(Mutex::new(None));
        let attempted_from_hook = Arc::clone(&write_attempted);
        let error_from_hook = Arc::clone(&write_error);

        reader
            .conn
            .authorizer(Some(move |context: rusqlite::hooks::AuthContext<'_>| {
                if matches!(
                    context.action,
                    AuthAction::Read {
                        table_name: "entity_deps",
                        ..
                    }
                ) && !attempted_from_hook.swap(true, Ordering::SeqCst)
                    && let Err(error) = writer.conn.execute_batch(
                        "BEGIN IMMEDIATE;
                         UPDATE entities SET disposition='rejected' WHERE id='b';
                         INSERT INTO entity_deps (entity_id,depends_on_id) VALUES ('a','b');
                         COMMIT;",
                    )
                {
                    *error_from_hook.lock().unwrap() = Some(error.to_string());
                }
                Authorization::Allow
            }))
            .unwrap();

        let view = reader.view().unwrap();
        assert!(write_attempted.load(Ordering::SeqCst));
        assert_eq!(write_error.lock().unwrap().as_deref(), None);
        assert_eq!(
            view.get(&id("b")).unwrap().disposition,
            Disposition::Accepted
        );
        assert!(view.dependency_targets(&id("a")).is_empty());

        let fresh = reader.view().unwrap();
        assert_eq!(
            fresh.get(&id("b")).unwrap().disposition,
            Disposition::Rejected
        );
        assert_eq!(fresh.dependency_targets(&id("a")).len(), 1);

        drop(reader);
        for candidate in [
            path.clone(),
            PathBuf::from(format!("{}-wal", path.display())),
            PathBuf::from(format!("{}-shm", path.display())),
        ] {
            let _ = std::fs::remove_file(candidate);
        }
    }

    #[test]
    fn relation_removals_can_repair_multiple_existing_cycles() {
        let mut store = Store::in_memory("t").unwrap();
        for value in ["a", "b", "c", "d"] {
            store
                .insert(&entity(value, EntityKind::Issue, None))
                .unwrap();
        }
        store.insert(&entity("g", EntityKind::Group, None)).unwrap();
        store
            .insert(&entity("e", EntityKind::Issue, Some("g")))
            .unwrap();
        for (source, target) in [("a", "b"), ("b", "a"), ("c", "d"), ("d", "c")] {
            store
                .conn
                .execute(
                    "UPDATE entities SET resurface_kind='after_entity', resurface_ref=?2
                     WHERE id=?1",
                    params![source, target],
                )
                .unwrap();
        }

        // Build a consistent history boundary for this deliberately cyclic stored fixture.
        let mut fixture = read_linear_state(&store.conn, store.evaluation.clone()).unwrap();
        fixture.migrate_causality(12).unwrap();
        store.conn.execute_batch("DELETE FROM history_baselines; DELETE FROM history_merges; DELETE FROM causal_links; DELETE FROM history_lineage;").unwrap();
        publish_causal(&store.conn, &Default::default(), &fixture.causal).unwrap();

        store
            .apply(
                &id("a"),
                Change::SetResurfaceCondition(ResurfaceCondition::Always),
                &ctx(),
            )
            .unwrap();
        store
            .apply(
                &id("c"),
                Change::SetResurfaceCondition(ResurfaceCondition::AtDate(
                    "2026-01-01".parse().unwrap(),
                )),
                &ctx(),
            )
            .unwrap();
        store
            .apply(&id("e"), Change::Decide(Disposition::Undecided), &ctx())
            .unwrap();
        store
            .apply(&id("e"), Change::SetParent(None), &ctx())
            .unwrap();

        assert_eq!(
            store.get(&id("a")).unwrap().resurface_condition,
            ResurfaceCondition::Always
        );
        assert!(matches!(
            store.get(&id("c")).unwrap().resurface_condition,
            ResurfaceCondition::AtDate(_)
        ));
        assert_eq!(store.get(&id("e")).unwrap().parent, None);
    }

    #[test]
    fn legacy_schema_is_rejected_without_implicit_migration() {
        let connection = Connection::open_in_memory().unwrap();
        for (index, sql) in MIGRATIONS.iter().take(7).enumerate() {
            connection.execute_batch(sql).unwrap();
            connection
                .pragma_update(None, "user_version", index as i64 + 1)
                .unwrap();
        }
        connection
            .execute("INSERT INTO meta VALUES ('prefix','old')", [])
            .unwrap();
        connection
            .execute(
                "INSERT INTO issues (
                 id,title,progress,disposition,created_at,updated_at
                 ) VALUES ('old-1','kept','not_started','accepted',?1,?1)",
                params![Utc::now().to_rfc3339()],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO issues (
                 id,title,progress,disposition,claimed_worktree,created_at,updated_at
                 ) VALUES ('old-2','active','in_progress','accepted','unknown',?1,?1)",
                params![Utc::now().to_rfc3339()],
            )
            .unwrap();
        assert!(matches!(
            Store::from_conn(connection),
            Err(DbError::UnsupportedSchema {
                found: 7,
                expected: SCHEMA_VERSION,
            })
        ));
    }
}
