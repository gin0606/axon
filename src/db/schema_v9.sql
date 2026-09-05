CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE entities (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL CHECK (kind IN ('issue','group')),
  title TEXT NOT NULL,
  description TEXT,
  progress TEXT NOT NULL CHECK (progress IN ('not_started','in_progress','ended')),
  disposition TEXT NOT NULL CHECK (disposition IN ('undecided','accepted','rejected')),
  current_revision INTEGER,
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
  CHECK ((disposition = 'undecided' AND current_revision IS NULL) OR
         (disposition IN ('accepted','rejected') AND current_revision IS NOT NULL)),
  CHECK ((progress = 'in_progress' AND claimed_actor IS NOT NULL AND
          claimed_worktree IS NOT NULL AND claimed_at IS NOT NULL) OR
         (progress <> 'in_progress' AND claimed_actor IS NULL AND
          claimed_worktree IS NULL AND claimed_at IS NULL)),
  CHECK ((resurface_kind IS NULL AND resurface_date IS NULL AND resurface_ref IS NULL) OR
         (resurface_kind = 'date' AND resurface_date IS NOT NULL AND resurface_ref IS NULL) OR
         (resurface_kind = 'after_entity' AND resurface_ref IS NOT NULL AND resurface_date IS NULL)),
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
