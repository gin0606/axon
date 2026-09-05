pub mod migration;

use crate::derived::{Evaluation, EvaluationError, View};
use crate::domain::*;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error(transparent)]
    Migration(#[from] migration::Failure),
    #[error("{0}")]
    Evaluation(#[from] EvaluationError),
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
    #[error("Note {number} does not exist for {id}")]
    NoSuchNote { id: String, number: i64 },
    #[error("Declaration Revision {number} does not exist for {id}")]
    NoSuchRevision { id: String, number: i64 },
    #[error("the plan declaration of {0} is fixed by its Disposition")]
    DeclarationFixed(String),
    #[error("invalid axon schema: {0}")]
    InvalidSchema(String),
    #[error("unsupported axon schema version {found}; expected {expected}")]
    UnsupportedSchema { found: i64, expected: i64 },
}

pub type Result<T> = std::result::Result<T, DbError>;

const SCHEMA_VERSION: i64 = 11;

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

const FRESH_SCHEMA: &str = r#"
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE entities (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL CHECK (kind IN ('issue','group')),
  title TEXT NOT NULL,
  description TEXT,
  progress TEXT NOT NULL CHECK (progress IN ('not_started','in_progress','ended')),
  disposition TEXT NOT NULL CHECK (disposition IN ('undecided','accepted','rejected')),
  current_revision INTEGER,
  resurface_kind TEXT CHECK (resurface_kind IN ('date','after_entity','manual','command')),
  resurface_date TEXT,
  resurface_ref TEXT,
  resurface_command TEXT,
  parent_id TEXT REFERENCES entities(id),
  claimed_actor TEXT,
  claimed_worktree TEXT,
  claimed_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  CHECK (id <> parent_id),
  CHECK ((disposition = 'undecided' AND current_revision IS NULL) OR
         (disposition IN ('accepted','rejected') AND current_revision IS NOT NULL)),
  CHECK ((progress = 'in_progress' AND claimed_actor IS NOT NULL AND
          claimed_worktree IS NOT NULL AND claimed_at IS NOT NULL) OR
         (progress <> 'in_progress' AND claimed_actor IS NULL AND
          claimed_worktree IS NULL AND claimed_at IS NULL)),
  CHECK ((resurface_kind IS NULL AND resurface_date IS NULL AND resurface_ref IS NULL AND resurface_command IS NULL) OR
         (resurface_kind IS NOT NULL AND ((resurface_kind = 'date' AND resurface_date IS NOT NULL AND resurface_ref IS NULL AND resurface_command IS NULL) OR
         (resurface_kind = 'after_entity' AND resurface_ref IS NOT NULL AND resurface_date IS NULL AND resurface_command IS NULL) OR
         (resurface_kind = 'manual' AND resurface_command IS NULL AND resurface_date IS NULL AND resurface_ref IS NULL) OR
         (resurface_kind = 'command' AND resurface_command IS NOT NULL AND resurface_date IS NULL AND resurface_ref IS NULL)))),
  FOREIGN KEY (id,current_revision)
    REFERENCES declaration_revisions(entity_id,revision) DEFERRABLE INITIALLY DEFERRED
);

CREATE TABLE entity_deps (
  entity_id TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
  depends_on_id TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
  PRIMARY KEY (entity_id, depends_on_id), CHECK (entity_id <> depends_on_id)
);

CREATE TABLE declaration_revisions (
  entity_id TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
  revision INTEGER NOT NULL CHECK (revision > 0),
  title TEXT NOT NULL,
  description TEXT,
  parent_id TEXT REFERENCES entities(id),
  created_at TEXT NOT NULL,
  baseline INTEGER NOT NULL DEFAULT 0 CHECK (baseline IN (0,1)),
  PRIMARY KEY (entity_id, revision)
);

CREATE TABLE revision_dependencies (
  entity_id TEXT NOT NULL,
  revision INTEGER NOT NULL,
  depends_on_id TEXT NOT NULL REFERENCES entities(id),
  PRIMARY KEY (entity_id, revision, depends_on_id),
  FOREIGN KEY (entity_id,revision)
    REFERENCES declaration_revisions(entity_id,revision) ON DELETE CASCADE
);

CREATE TABLE entity_events (
  id INTEGER PRIMARY KEY,
  entity_id TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
  field TEXT NOT NULL CHECK (field IN ('disposition','resurface_condition')),
  old_value TEXT,
  new_value TEXT,
  revision INTEGER,
  actor TEXT NOT NULL,
  reason TEXT,
  at TEXT NOT NULL,
  CHECK ((field = 'disposition' AND new_value = 'undecided' AND revision IS NULL) OR
         (field = 'disposition' AND new_value IN ('accepted','rejected') AND revision IS NOT NULL) OR
         (field = 'resurface_condition' AND revision IS NULL)),
  FOREIGN KEY (entity_id,revision)
    REFERENCES declaration_revisions(entity_id,revision)
);
CREATE INDEX idx_entity_events_entity ON entity_events (entity_id, id);

CREATE TABLE entity_progress_events (
  id INTEGER PRIMARY KEY,
  entity_id TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
  kind TEXT NOT NULL CHECK (kind IN ('start','done','release')),
  actor TEXT NOT NULL,
  reason TEXT,
  at TEXT NOT NULL
);
CREATE INDEX idx_entity_progress_events_entity ON entity_progress_events (entity_id, id);

CREATE TABLE entity_notes (
  entity_id TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
  note INTEGER NOT NULL CHECK (note > 0),
  body TEXT NOT NULL CHECK (length(trim(body)) > 0),
  actor TEXT NOT NULL,
  at TEXT NOT NULL,
  PRIMARY KEY (entity_id,note)
);
"#;

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

#[derive(Clone, Copy)]
enum ResolveFor {
    Init,
    Open,
}

fn management_root(resolve_for: ResolveFor) -> Result<PathBuf> {
    let current_dir = std::env::current_dir()?;
    if let Some(root) = git_management_root(&current_dir)? {
        return Ok(root);
    }
    if let Some(root) = current_dir
        .ancestors()
        .find(|root| database_path(root).is_file())
    {
        return match resolve_for {
            ResolveFor::Init => Err(DbError::AlreadyInitialized(root.to_path_buf())),
            ResolveFor::Open => Ok(root.to_path_buf()),
        };
    }
    match resolve_for {
        ResolveFor::Init => Ok(current_dir),
        ResolveFor::Open => Err(DbError::NotInitialized),
    }
}

fn git_management_root(current_dir: &Path) -> Result<Option<PathBuf>> {
    let output = Command::new("git")
        .current_dir(current_dir)
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .output();
    let output = match output {
        Ok(output) => output,
        Err(_) if !has_git_marker(current_dir) => return Ok(None),
        Err(error) => return Err(DbError::GitRoot(error.to_string())),
    };
    if !output.status.success() {
        if has_git_marker(current_dir) {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let detail = if stderr.is_empty() {
                output.status.to_string()
            } else {
                stderr
            };
            return Err(DbError::GitRoot(detail));
        }
        return Ok(None);
    }
    let git_dir = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    git_dir
        .parent()
        .map(Path::to_path_buf)
        .map(Some)
        .ok_or_else(|| DbError::GitRoot("Git returned a common directory without a parent".into()))
}

fn has_git_marker(current_dir: &Path) -> bool {
    current_dir
        .ancestors()
        .any(|directory| directory.join(".git").exists())
}

fn database_path(root: &Path) -> PathBuf {
    root.join(".axon").join("axon.db")
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
}

pub struct RevisionSnapshot {
    pub entity: Entity,
    pub revisions: Vec<DeclarationRevision>,
}

impl RevisionSnapshot {
    pub fn revision(&self, number: i64) -> Result<&DeclarationRevision> {
        self.revisions
            .iter()
            .find(|revision| revision.number == number)
            .ok_or_else(|| DbError::NoSuchRevision {
                id: self.entity.id.to_string(),
                number,
            })
    }
}

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

    pub fn init(prefix: Option<&str>) -> Result<(PathBuf, String)> {
        let root = management_root(ResolveFor::Init)?;
        let path = database_path(&root);
        if path.exists() {
            return Err(DbError::AlreadyInitialized(root));
        }
        let prefix = prefix.map(str::to_owned).unwrap_or_else(|| {
            root.file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "axon".to_string())
        });
        std::fs::create_dir_all(path.parent().expect("database path has a parent"))?;
        let mut conn = Connection::open(&path)?;
        initialize_schema(&mut conn)?;
        conn.execute(
            "INSERT INTO meta (key, value) VALUES ('prefix', ?1)",
            params![prefix],
        )?;
        Ok((path, prefix))
    }

    pub fn open(report: impl FnOnce(&migration::Outcome)) -> Result<Self> {
        let root = management_root(ResolveFor::Open)?;
        let path = database_path(&root);
        if !path.exists() {
            return Err(DbError::NotInitialized);
        }
        let (conn, outcome) = migration::open(&path)?;
        report(&outcome);
        let mut store = Self::from_conn(conn)?;
        let output = Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .output();
        let command_root = match output {
            Ok(output) if output.status.success() => {
                PathBuf::from(String::from_utf8_lossy(&output.stdout).trim())
            }
            _ => root,
        };
        store.evaluation = Rc::new(Evaluation::new(command_root));
        Ok(store)
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
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(DbError::from)?;
        let before = read_snapshot(&tx, self.evaluation.clone()).map_err(E::from)?;
        let (value, desired) = build(before.clone())?;
        apply_import_snapshot(&tx, &before, &desired).map_err(E::from)?;
        let after = read_snapshot(&tx, self.evaluation.clone()).map_err(E::from)?;
        tx.commit().map_err(DbError::from)?;
        Ok((value, after))
    }

    pub fn resolve_id(&self, input: &str) -> Result<EntityId> {
        resolve_id(&self.conn, input)
    }

    pub fn get(&self, id: &EntityId) -> Result<Entity> {
        read_entity(&self.conn, id)
    }

    pub fn notes(&self, id: &EntityId) -> Result<Vec<Note>> {
        read_entity(&self.conn, id)?;
        read_notes(&self.conn, id)
    }

    pub fn note(&self, id: &EntityId, number: i64) -> Result<Note> {
        read_entity(&self.conn, id)?;
        read_note(&self.conn, id, number)
    }

    pub fn add_note(&mut self, id: &EntityId, body: &str, actor: &str) -> Result<Note> {
        if body.trim().is_empty() {
            return Err(DbError::EmptyNote);
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        read_entity(&tx, id)?;
        let number = tx.query_row(
            "SELECT coalesce(max(note),0) + 1 FROM entity_notes WHERE entity_id=?1",
            params![id.as_str()],
            |row| row.get::<_, i64>(0),
        )?;
        let created_at = Utc::now();
        tx.execute(
            "INSERT INTO entity_notes (entity_id,note,body,actor,at)
             VALUES (?1,?2,?3,?4,?5)",
            params![id.as_str(), number, body, actor, created_at.to_rfc3339()],
        )?;
        tx.commit()?;
        Ok(Note {
            number,
            body: body.to_string(),
            actor: actor.to_string(),
            created_at,
        })
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

    pub fn insert(&mut self, entity: &Entity) -> Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        validate_parent_target(&tx, entity.parent.as_ref())?;
        if let Some(parent) = &entity.parent {
            let current = read_view(&tx, self.evaluation.clone())?;
            let target = current.get(parent).expect("validated parent exists");
            if matches!(target.progress, Progress::Ended)
                || current
                    .ancestors(parent)
                    .into_iter()
                    .any(|ancestor| matches!(ancestor.progress, Progress::Ended))
            {
                return Err(DbError::Containment(format!(
                    "cannot add {} below an Ended group",
                    entity.id
                )));
            }
        }
        write_entity(&tx, entity)?;
        let view = read_view(&tx, self.evaluation.clone())?;
        validate_structure(&view)?;
        validate_relations(&view)?;
        tx.commit()?;
        Ok(())
    }

    pub fn add_dep(&mut self, source: &EntityId, target: &EntityId) -> Result<ApplyOutcome> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let source_entity = read_entity(&tx, source)?;
        read_entity(&tx, target)?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM entity_deps WHERE entity_id=?1 AND depends_on_id=?2)",
            params![source.as_str(), target.as_str()],
            |row| row.get(0),
        )?;
        if exists {
            tx.commit()?;
            return Ok(ApplyOutcome::Unchanged);
        }
        if source_entity.kind == EntityKind::Group
            && matches!(source_entity.progress, Progress::Ended)
        {
            return Err(DbError::Containment(format!(
                "Ended group {source} cannot change its dependencies"
            )));
        }
        if source_entity.disposition != Disposition::Undecided {
            return Err(DbError::DeclarationFixed(source.to_string()));
        }
        write_dependency(&tx, source, target, true)?;
        validate_relations(&read_view(&tx, self.evaluation.clone())?)?;
        tx.commit()?;
        Ok(ApplyOutcome::Changed)
    }

    pub fn remove_dep(&mut self, source: &EntityId, target: &EntityId) -> Result<ApplyOutcome> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let source_entity = read_entity(&tx, source)?;
        read_entity(&tx, target)?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM entity_deps WHERE entity_id=?1 AND depends_on_id=?2)",
            params![source.as_str(), target.as_str()],
            |row| row.get(0),
        )?;
        if !exists {
            tx.commit()?;
            return Ok(ApplyOutcome::Unchanged);
        }
        if source_entity.kind == EntityKind::Group
            && matches!(source_entity.progress, Progress::Ended)
        {
            return Err(DbError::Containment(format!(
                "Ended group {source} cannot change its dependencies"
            )));
        }
        if source_entity.disposition != Disposition::Undecided {
            return Err(DbError::DeclarationFixed(source.to_string()));
        }
        write_dependency(&tx, source, target, false)?;
        tx.commit()?;
        Ok(ApplyOutcome::Changed)
    }

    pub fn apply(&mut self, id: &EntityId, change: Change, ctx: &Ctx) -> Result<ApplyOutcome> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let before = read_entity(&tx, id)?;
        if let Some((field, current)) = unchanged_transition(&before, &change) {
            return Err(DbError::Unchanged {
                id: id.to_string(),
                field,
                current,
            });
        }
        if unchanged_setting(&before, &change) {
            tx.commit()?;
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
            return Err(DbError::DeclarationFixed(id.to_string()));
        }

        let now = Utc::now();
        let view = read_view(&tx, self.evaluation.clone())?;
        match &change {
            Change::Start(claim) => {
                if !view.is_ready(&before)? {
                    return Err(DbError::CannotStart {
                        id: id.to_string(),
                        fact: not_ready_fact(&view, &before)?,
                    });
                }
                tx.execute(
                    "UPDATE entities SET progress='in_progress', claimed_actor=?2,
                     claimed_worktree=?3, claimed_at=?4, updated_at=?5 WHERE id=?1",
                    params![
                        id.as_str(),
                        claim.actor,
                        claim.worktree,
                        claim.at.to_rfc3339(),
                        now.to_rfc3339()
                    ],
                )?;
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
                    return Err(DbError::CannotProgress {
                        id: id.to_string(),
                        action: "ended",
                        fact: format!("non-terminal descendants: {remaining}"),
                    });
                }
                clear_claim_and_set_progress(&tx, id, "ended", &now)?;
            }
            Change::Release => {
                if !matches!(before.progress, Progress::InProgress(_)) {
                    return Err(cannot_progress(id, "released", &before));
                }
                if before.kind == EntityKind::Group {
                    let active = view.in_progress_descendants(id);
                    if !active.is_empty() {
                        return Err(DbError::CannotProgress {
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
                clear_claim_and_set_progress(&tx, id, "not_started", &now)?;
            }
            Change::Decide(disposition) => {
                let current_revision = if *disposition == Disposition::Undecided {
                    None
                } else {
                    Some(ensure_current_revision(&tx, id, &now, false)?)
                };
                tx.execute(
                    "UPDATE entities SET disposition=?2,current_revision=?3,updated_at=?4
                     WHERE id=?1",
                    params![
                        id.as_str(),
                        disposition.as_db(),
                        current_revision,
                        now.to_rfc3339()
                    ],
                )?;
                let updated = read_view(&tx, self.evaluation.clone())?;
                if updated
                    .ancestors(id)
                    .into_iter()
                    .any(|ancestor| matches!(ancestor.progress, Progress::Ended))
                    && !updated
                        .get(id)
                        .expect("updated entity exists")
                        .is_terminal()
                {
                    return Err(DbError::Containment(format!(
                        "{id} must remain terminal below an Ended group"
                    )));
                }
            }
            Change::SetResurfaceCondition(condition) => {
                if let ResurfaceCondition::AfterEntity(target) = condition {
                    read_entity(&tx, target)?;
                }
                tx.execute(
                    "UPDATE entities SET resurface_kind=?2, resurface_date=?3,
                     resurface_ref=?4, updated_at=?5, resurface_command=?6 WHERE id=?1",
                    params![
                        id.as_str(),
                        condition.kind_db(),
                        resurface_date(condition),
                        resurface_ref(condition),
                        now.to_rfc3339(),
                        resurface_command(condition)
                    ],
                )?;
                if matches!(condition, ResurfaceCondition::AfterEntity(_)) {
                    validate_relations(&read_view(&tx, self.evaluation.clone())?)?;
                }
            }
            Change::SetTitle(title) => {
                write_entity_setting(&tx, id, &Change::SetTitle(title.clone()), &now)?;
            }
            Change::SetDescription(description) => {
                write_entity_setting(&tx, id, &Change::SetDescription(description.clone()), &now)?;
            }
            Change::SetParent(parent) => {
                validate_parent_target(&tx, parent.as_ref())?;
                if before.kind == EntityKind::Group && matches!(before.progress, Progress::Ended) {
                    return Err(DbError::Containment(format!(
                        "Ended group {id} cannot move"
                    )));
                }
                if view
                    .ancestors(id)
                    .into_iter()
                    .any(|ancestor| matches!(ancestor.progress, Progress::Ended))
                {
                    return Err(DbError::Containment(format!(
                        "{id} cannot move within an Ended group"
                    )));
                }
                if let Some(parent) = parent {
                    let target = read_entity(&tx, parent)?;
                    if matches!(target.progress, Progress::Ended)
                        || view
                            .ancestors(parent)
                            .into_iter()
                            .any(|ancestor| matches!(ancestor.progress, Progress::Ended))
                    {
                        return Err(DbError::Containment(format!(
                            "cannot add {id} below an Ended group"
                        )));
                    }
                }
                write_entity_setting(&tx, id, &Change::SetParent(parent.clone()), &now)?;
                if parent.is_some() {
                    let updated = read_view(&tx, self.evaluation.clone())?;
                    validate_structure(&updated)?;
                    validate_relations(&updated)?;
                }
            }
        }

        if let Some(kind) = progress_event_kind(&change) {
            tx.execute(
                "INSERT INTO entity_progress_events (entity_id,kind,actor,reason,at)
                 VALUES (?1,?2,?3,?4,?5)",
                params![
                    id.as_str(),
                    kind.as_db(),
                    ctx.actor,
                    (kind == ProgressEventKind::Release)
                        .then_some(ctx.reason.as_deref())
                        .flatten(),
                    now.to_rfc3339()
                ],
            )?;
        }
        if let Some((field, old_value, new_value)) = decision_event(&before, &change) {
            let revision = match change {
                Change::Decide(_) => read_entity(&tx, id)?.current_revision,
                _ => None,
            };
            tx.execute(
                "INSERT INTO entity_events
                 (entity_id,field,old_value,new_value,revision,actor,reason,at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                params![
                    id.as_str(),
                    field,
                    old_value,
                    new_value,
                    revision,
                    ctx.actor,
                    ctx.reason,
                    now.to_rfc3339()
                ],
            )?;
        }
        tx.commit()?;
        Ok(ApplyOutcome::Changed)
    }

    pub fn events(&self, id: &EntityId) -> Result<Vec<Event>> {
        read_events(&self.conn, id)
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

fn cannot_progress(id: &EntityId, action: &'static str, entity: &Entity) -> DbError {
    DbError::CannotProgress {
        id: id.to_string(),
        action,
        fact: format!("Progress is {}", entity.progress.label()),
    }
}

fn clear_claim_and_set_progress(
    tx: &Transaction<'_>,
    id: &EntityId,
    progress: &str,
    at: &DateTime<Utc>,
) -> Result<()> {
    tx.execute(
        "UPDATE entities SET progress=?2, claimed_actor=NULL, claimed_worktree=NULL,
         claimed_at=NULL, updated_at=?3 WHERE id=?1",
        params![id.as_str(), progress, at.to_rfc3339()],
    )?;
    Ok(())
}

fn validate_parent_target(conn: &Connection, parent: Option<&EntityId>) -> Result<()> {
    if let Some(parent) = parent {
        let entity = read_entity(conn, parent)?;
        if entity.kind != EntityKind::Group {
            return Err(DbError::NotGroup(parent.to_string()));
        }
    }
    Ok(())
}

fn validate_structure(view: &View) -> Result<()> {
    for entity in view.iter() {
        if let Some(parent) = &entity.parent {
            let Some(parent) = view.get(parent) else {
                return Err(DbError::Containment(format!(
                    "parent {} of {} does not exist",
                    parent, entity.id
                )));
            };
            if parent.kind != EntityKind::Group {
                return Err(DbError::NotGroup(parent.id.to_string()));
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
            return Err(DbError::Containment(format!(
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
            return Err(DbError::Cycle {
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
    if let Some(revision) = entity.current_revision {
        write_revision(conn, entity, revision, &[], false, &entity.created_at)?;
    }
    Ok(())
}

fn write_revision(
    conn: &Connection,
    entity: &Entity,
    revision: i64,
    dependencies: &[EntityId],
    baseline: bool,
    created_at: &DateTime<Utc>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO declaration_revisions
         (entity_id,revision,title,description,parent_id,created_at,baseline)
         VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![
            entity.id.as_str(),
            revision,
            entity.title,
            entity.description,
            entity.parent.as_ref().map(EntityId::as_str),
            created_at.to_rfc3339(),
            baseline,
        ],
    )?;
    for dependency in dependencies {
        conn.execute(
            "INSERT INTO revision_dependencies (entity_id,revision,depends_on_id)
             VALUES (?1,?2,?3)",
            params![entity.id.as_str(), revision, dependency.as_str()],
        )?;
    }
    Ok(())
}

fn ensure_current_revision(
    conn: &Connection,
    id: &EntityId,
    created_at: &DateTime<Utc>,
    baseline: bool,
) -> Result<i64> {
    let entity = read_entity(conn, id)?;
    let dependencies = read_deps(conn)?
        .into_iter()
        .filter_map(|(source, target)| (source == *id).then_some(target))
        .collect::<Vec<_>>();
    let latest = conn.query_row(
        "SELECT max(revision) FROM declaration_revisions WHERE entity_id=?1",
        params![id.as_str()],
        |row| row.get::<_, Option<i64>>(0),
    )?;
    if let Some(number) = latest {
        let revision = read_revision(conn, id, number)?;
        if revision.title == entity.title
            && revision.description == entity.description
            && revision.parent == entity.parent
            && revision.dependencies == dependencies
        {
            return Ok(number);
        }
    }
    let number = latest.unwrap_or(0) + 1;
    write_revision(conn, &entity, number, &dependencies, baseline, created_at)?;
    Ok(number)
}

fn read_snapshot(conn: &Connection, evaluation: Rc<Evaluation>) -> Result<StoreSnapshot> {
    Ok(StoreSnapshot {
        evaluation,
        entities: read_all(conn)?,
        dependencies: read_deps(conn)?,
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
        return Err(DbError::InvalidImport(
            "the final snapshot contains duplicate Entity IDs".to_string(),
        ));
    }
    if let Some(missing) = before
        .entities
        .iter()
        .find(|entity| !desired_ids.contains(&entity.id))
    {
        return Err(DbError::InvalidImport(format!(
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
                return Err(DbError::InvalidImport(format!(
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
            return Err(DbError::InvalidImport(format!(
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
            return Err(DbError::DeclarationFixed(after.id.to_string()));
        }
        if current.kind == EntityKind::Group
            && matches!(current.progress, Progress::Ended)
            && (current.parent != after.parent
                || direct_dependency_ids(&before_view, &current.id)
                    != direct_dependency_ids(&desired_view, &after.id))
        {
            return Err(DbError::Containment(format!(
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
            return Err(DbError::Containment(format!(
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
            return Err(DbError::Containment(format!(
                "cannot add {} below an Ended group",
                after.id
            )));
        }
    }
    validate_structure(&desired_view)?;
    validate_relations(&desired_view)?;
    Ok(())
}

fn apply_import_snapshot(
    conn: &Connection,
    before: &StoreSnapshot,
    desired: &StoreSnapshot,
) -> Result<()> {
    validate_import_snapshot(before, desired)?;
    let before_view = before.view();
    let now = Utc::now();
    let new_ids = desired
        .entities
        .iter()
        .filter(|entity| before_view.get(&entity.id).is_none())
        .map(|entity| entity.id.clone())
        .collect::<Vec<_>>();
    for after in &desired.entities {
        if before_view.get(&after.id).is_none() {
            let mut entity = after.clone();
            entity.parent = None;
            entity.disposition = Disposition::Undecided;
            entity.current_revision = None;
            entity.created_at = now;
            entity.updated_at = now;
            write_entity(conn, &entity)?;
        }
    }
    for after in &desired.entities {
        let current = before_view.get(&after.id);
        if current.is_none_or(|current| current.title != after.title) {
            write_entity_setting(
                conn,
                &after.id,
                &Change::SetTitle(after.title.clone()),
                &now,
            )?;
        }
        if current.is_none_or(|current| current.description != after.description) {
            write_entity_setting(
                conn,
                &after.id,
                &Change::SetDescription(after.description.clone()),
                &now,
            )?;
        }
        if current.is_none_or(|current| current.parent != after.parent) {
            write_entity_setting(
                conn,
                &after.id,
                &Change::SetParent(after.parent.clone()),
                &now,
            )?;
        }
    }
    if before.dependencies != desired.dependencies {
        let before = before.dependencies.iter().cloned().collect::<HashSet<_>>();
        let desired = desired.dependencies.iter().cloned().collect::<HashSet<_>>();
        for (source, target) in before.difference(&desired) {
            write_dependency(conn, source, target, false)?;
        }
        for (source, target) in desired.difference(&before) {
            write_dependency(conn, source, target, true)?;
        }
    }
    for id in new_ids {
        let revision = ensure_current_revision(conn, &id, &now, false)?;
        conn.execute(
            "UPDATE entities SET disposition='accepted',current_revision=?2 WHERE id=?1",
            params![id.as_str(), revision],
        )?;
    }
    Ok(())
}

fn write_entity_setting(
    conn: &Connection,
    id: &EntityId,
    change: &Change,
    now: &DateTime<Utc>,
) -> Result<()> {
    match change {
        Change::SetTitle(title) => {
            conn.execute(
                "UPDATE entities SET title=?2, updated_at=?3 WHERE id=?1",
                params![id.as_str(), title, now.to_rfc3339()],
            )?;
        }
        Change::SetDescription(description) => {
            conn.execute(
                "UPDATE entities SET description=?2, updated_at=?3 WHERE id=?1",
                params![id.as_str(), description, now.to_rfc3339()],
            )?;
        }
        Change::SetParent(parent) => {
            conn.execute(
                "UPDATE entities SET parent_id=?2, updated_at=?3 WHERE id=?1",
                params![
                    id.as_str(),
                    parent.as_ref().map(EntityId::as_str),
                    now.to_rfc3339()
                ],
            )?;
        }
        _ => {
            return Err(DbError::InvalidImport(
                "a transition was passed to the setting writer".to_string(),
            ));
        }
    }
    Ok(())
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
    let mut statement = conn
        .prepare("SELECT note,body,actor,at FROM entity_notes WHERE entity_id=?1 ORDER BY note")?;
    let rows = statement.query_map(params![id.as_str()], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;
    rows.map(|row| {
        let (number, body, actor, at) = row?;
        Ok(Note {
            number,
            body,
            actor,
            created_at: parse_time(&at)?,
        })
    })
    .collect()
}

fn read_note(conn: &Connection, id: &EntityId, number: i64) -> Result<Note> {
    let row = conn
        .query_row(
            "SELECT body,actor,at FROM entity_notes WHERE entity_id=?1 AND note=?2",
            params![id.as_str(), number],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| DbError::NoSuchNote {
            id: id.to_string(),
            number,
        })?;
    Ok(Note {
        number,
        body: row.0,
        actor: row.1,
        created_at: parse_time(&row.2)?,
    })
}

fn read_revision_dependencies(
    conn: &Connection,
    id: &EntityId,
    revision: i64,
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

fn read_revision(conn: &Connection, id: &EntityId, number: i64) -> Result<DeclarationRevision> {
    let row = conn
        .query_row(
            "SELECT title,description,parent_id,created_at,baseline
             FROM declaration_revisions WHERE entity_id=?1 AND revision=?2",
            params![id.as_str(), number],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, bool>(4)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| DbError::NoSuchRevision {
            id: id.to_string(),
            number,
        })?;
    Ok(DeclarationRevision {
        number,
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
        "SELECT revision FROM declaration_revisions WHERE entity_id=?1 ORDER BY revision",
    )?;
    let rows = statement.query_map(params![id.as_str()], |row| row.get::<_, i64>(0))?;
    let numbers = rows.collect::<std::result::Result<Vec<_>, _>>()?;
    numbers
        .into_iter()
        .map(|number| read_revision(conn, id, number))
        .collect()
}

fn read_revision_snapshot(conn: &Connection, id: &EntityId) -> Result<RevisionSnapshot> {
    Ok(RevisionSnapshot {
        entity: read_entity(conn, id)?,
        revisions: read_revisions(conn, id)?,
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
    current_revision: Option<i64>,
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
pub struct Event {
    pub field: String,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
    pub revision: Option<i64>,
    pub actor: String,
    pub reason: Option<String>,
    pub at: DateTime<Utc>,
}

fn read_events(conn: &Connection, id: &EntityId) -> Result<Vec<Event>> {
    let mut statement = conn.prepare(
        "SELECT field,old_value,new_value,revision,actor,reason,at
         FROM entity_events WHERE entity_id=?1 ORDER BY id",
    )?;
    let rows = statement.query_map(params![id.as_str()], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<i64>>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, String>(6)?,
        ))
    })?;
    rows.map(|row| {
        let (field, old_value, new_value, revision, actor, reason, at) = row?;
        Ok(Event {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressEventKind {
    Start,
    Done,
    Release,
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

fn progress_event_kind(change: &Change) -> Option<ProgressEventKind> {
    match change {
        Change::Start(_) => Some(ProgressEventKind::Start),
        Change::Done => Some(ProgressEventKind::Done),
        Change::Release => Some(ProgressEventKind::Release),
        _ => None,
    }
}

#[derive(Debug, Clone)]
pub struct ProgressEvent {
    pub kind: ProgressEventKind,
    pub actor: String,
    pub reason: Option<String>,
    pub at: DateTime<Utc>,
}

fn read_progress_events(conn: &Connection, id: &EntityId) -> Result<Vec<ProgressEvent>> {
    let mut statement = conn.prepare(
        "SELECT kind,actor,reason,at FROM entity_progress_events
         WHERE entity_id=?1 ORDER BY id",
    )?;
    let rows = statement.query_map(params![id.as_str()], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;
    rows.map(|row| {
        let (kind, actor, reason, at) = row?;
        Ok(ProgressEvent {
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
    let decision_events = read_events(conn, &id)?;
    let progress_events = read_progress_events(conn, &id)?;
    let revisions = read_revisions(conn, &id)?;
    let notes = read_notes(conn, &id)?;
    let counts = RecordCounts {
        notes: notes.len(),
        revisions: revisions.len(),
        decisions: decision_events.len(),
        progressions: progress_events.len(),
    };
    Ok(ShowSnapshot {
        view: read_view(conn, evaluation)?,
        decision_events,
        progress_events,
        revisions,
        notes,
        counts,
        id,
    })
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
            current_revision: Some(1),
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
            matches!(result, Err(DbError::Sqlite(ref error)) if error.to_string().contains("injected import failure"))
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
        assert_eq!(accepted.current_revision, Some(1));
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
        assert_eq!(store.get(&id("i")).unwrap().current_revision, Some(2));
        assert_eq!(store.revisions(&id("i")).unwrap().len(), 2);
        assert_eq!(
            store.events(&id("i")).unwrap().last().unwrap().revision,
            Some(2)
        );

        store
            .apply(&id("i"), Change::Decide(Disposition::Undecided), &ctx())
            .unwrap();
        store
            .apply(&id("i"), Change::Decide(Disposition::Rejected), &ctx())
            .unwrap();
        assert_eq!(store.get(&id("i")).unwrap().current_revision, Some(2));
        assert_eq!(store.revisions(&id("i")).unwrap().len(), 2);
    }

    #[test]
    fn notes_are_append_only_numbered_and_reject_blank_bodies() {
        let mut store = Store::in_memory("t").unwrap();
        store.insert(&entity("i", EntityKind::Issue, None)).unwrap();
        let first = store.add_note(&id("i"), "first", "alice").unwrap();
        let second = store.add_note(&id("i"), "first", "alice").unwrap();
        assert_eq!((first.number, second.number), (1, 2));
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
                    candidate.current_revision =
                        (disposition != Disposition::Undecided).then_some(1);
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
        let mut numbers = threads
            .into_iter()
            .map(|thread| thread.join().unwrap().number)
            .collect::<Vec<_>>();
        numbers.sort();
        assert_eq!(numbers, (1..=8).collect::<Vec<_>>());

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
                        table_name: "declaration_revisions",
                        ..
                    }
                ) && !attempted_from_hook.swap(true, Ordering::SeqCst)
                    && let Err(error) = writer.conn.execute_batch(
                        "BEGIN IMMEDIATE;
                         UPDATE entities SET disposition='undecided',current_revision=NULL
                           WHERE id='i';
                         UPDATE entities SET title='changed' WHERE id='i';
                         INSERT INTO declaration_revisions
                           (entity_id,revision,title,created_at,baseline)
                           VALUES ('i',2,'changed','2026-09-03T00:00:00Z',0);
                         UPDATE entities SET disposition='accepted',current_revision=2
                           WHERE id='i';
                         COMMIT;",
                    )
                {
                    *error_from_hook.lock().unwrap() = Some(error.to_string());
                }
                Authorization::Allow
            }))
            .unwrap();

        let snapshot = reader.revision_snapshot(&id("i")).unwrap();
        assert!(write_attempted.load(Ordering::SeqCst));
        assert_eq!(write_error.lock().unwrap().as_deref(), None);
        assert_eq!(snapshot.entity.current_revision, Some(1));
        assert_eq!(snapshot.revisions.len(), 1);

        let fresh = reader.revision_snapshot(&id("i")).unwrap();
        assert_eq!(fresh.entity.current_revision, Some(2));
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
