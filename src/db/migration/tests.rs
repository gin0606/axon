use super::*;
use rusqlite::types::Value;
use std::{
    collections::BTreeMap,
    process::{Command, Stdio},
    sync::{Arc, Barrier},
    time::{Duration, Instant},
};

struct Fixture {
    root: PathBuf,
    database: PathBuf,
}
impl Fixture {
    fn new(version: i64, wal: bool) -> Self {
        Self::with_schema(version, wal, schema(version).unwrap())
    }
    fn with_schema(version: i64, wal: bool, ddl: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "axon-migration-test-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir_all(root.join(".axon")).unwrap();
        let database = root.join(".axon/axon.db");
        let conn = Connection::open(&database).unwrap();
        conn.execute_batch(ddl).unwrap();
        conn.pragma_update(None, "user_version", version).unwrap();
        conn.execute_batch(r#"
BEGIN;
INSERT INTO meta VALUES ('prefix','fixture'), ('custom','metadata 日本語');
INSERT INTO entities (id,kind,title,description,progress,disposition,current_revision,created_at,updated_at) VALUES
 ('group','group','Group','group description','not_started','accepted',1,'2026-01-01T00:00:00Z','2026-02-01T00:00:00Z'),
 ('accepted','issue','Accepted','line one
line two','not_started','accepted',2,'2026-01-01T00:00:00Z','2026-02-01T00:00:00Z'),
 ('rejected','issue','Rejected',NULL,'ended','rejected',1,'2026-01-01T00:00:00Z','2026-02-01T00:00:00Z'),
 ('draft','issue','Draft','','not_started','undecided',NULL,'2026-01-01T00:00:00Z','2026-02-01T00:00:00Z');
UPDATE entities SET parent_id='group' WHERE id='accepted';
UPDATE entities SET progress='in_progress', claimed_actor='previous actor', claimed_worktree='/synthetic/worktree', claimed_at='2026-02-01T00:00:00Z' WHERE id='accepted';
UPDATE entities SET resurface_kind='date',resurface_date='2026-12-31' WHERE id='draft';
UPDATE entities SET resurface_kind='after_entity',resurface_ref='rejected' WHERE id='accepted';
INSERT INTO declaration_revisions SELECT id,1,title,description,parent_id,created_at,1 FROM entities WHERE current_revision IS NOT NULL;
UPDATE declaration_revisions SET title='Earlier title',parent_id=NULL WHERE entity_id='accepted';
INSERT INTO declaration_revisions SELECT id,2,title,description,parent_id,updated_at,0 FROM entities WHERE id='accepted';
INSERT INTO entity_deps VALUES ('accepted','rejected');
INSERT INTO revision_dependencies VALUES ('accepted',2,'rejected');
INSERT INTO entity_notes VALUES ('accepted',1,'first note','note actor','2026-01-01T00:00:00Z'),('accepted',2,'second note','different actor','2025-12-01T00:00:00Z');
INSERT INTO entity_events VALUES (4,'accepted','disposition','undecided','accepted',2,'decision actor','decision reason','2026-02-01T00:00:00Z'),(7,'draft','resurface_condition',NULL,'AtDate(2026-12-31)',NULL,'condition actor',NULL,'2026-01-01T00:00:00Z');
INSERT INTO entity_progress_events VALUES (3,'accepted','start','start actor',NULL,'2026-02-01T00:00:00Z'),(8,'rejected','done','legacy actor','preserved old reason','2026-01-01T00:00:00Z');
COMMIT;
"#).unwrap();
        if version >= 10 {
            conn.execute_batch("UPDATE entities SET resurface_kind='command',resurface_command='printf command; exit 1' WHERE id='group'").unwrap();
        }
        if wal {
            conn.pragma_update(None, "journal_mode", "WAL").unwrap();
        }
        integrity(&conn).unwrap();
        Self { root, database }
    }
    fn backups(&self) -> Vec<PathBuf> {
        let directory = self.root.join(".axon/migration-backups");
        if !directory.is_dir() {
            return vec![];
        }
        fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

type Snapshot = BTreeMap<String, Vec<Vec<Value>>>;
fn snapshot(conn: &Connection, source_version: i64) -> Snapshot {
    let reference = Connection::open_in_memory().unwrap();
    reference
        .execute_batch(schema(source_version).unwrap())
        .unwrap();
    catalog(&reference)
        .unwrap()
        .into_iter()
        .filter(|row| row.0 == "table")
        .map(|(_, table, _, _)| {
            let columns: Vec<String> = reference
                .prepare(&format!("PRAGMA table_info({table})"))
                .unwrap()
                .query_map([], |row| row.get(1))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap();
            let names = columns.join(",");
            let rows = conn
                .prepare(&format!("SELECT {names} FROM {table} ORDER BY {names}"))
                .unwrap()
                .query_map([], |row| {
                    (0..columns.len())
                        .map(|i| row.get(i))
                        .collect::<rusqlite::Result<Vec<Value>>>()
                })
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap();
            (table, rows)
        })
        .collect()
}

#[test]
fn supported_paths_preserve_every_value_and_backup_including_wal() {
    for from in [9, 10] {
        for wal in [false, true] {
            let fixture = Fixture::new(from, wal);
            let writer = Connection::open(&fixture.database).unwrap();
            writer.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
            writer
                .execute(
                    "UPDATE meta SET value='committed WAL value' WHERE key='custom'",
                    [],
                )
                .unwrap();
            let before = snapshot(&writer, from);
            let (conn, outcome) = open(&fixture.database).unwrap();
            let Outcome::Migrated {
                from: actual,
                to,
                backup,
                ..
            } = outcome
            else {
                panic!("expected migration")
            };
            assert_eq!((actual, to), (from, SCHEMA_VERSION));
            assert_eq!(snapshot(&conn, from), before);
            assert_eq!(snapshot(&Connection::open(&backup).unwrap(), from), before);
            assert_eq!(version(&Connection::open(backup).unwrap()).unwrap(), from);
            integrity(&conn).unwrap();
            let entities = super::super::read_all(&conn).unwrap();
            for entity in entities {
                super::super::read_notes(&conn, &entity.id).unwrap();
                super::super::read_revisions(&conn, &entity.id).unwrap();
                super::super::read_events(&conn, &entity.id).unwrap();
                super::super::read_progress_events(&conn, &entity.id).unwrap();
            }
            if from == 9 {
                assert_eq!(
                    conn.query_row(
                        "SELECT count(*) FROM entities WHERE resurface_command IS NOT NULL",
                        [],
                        |r| r.get::<_, i64>(0)
                    )
                    .unwrap(),
                    0
                );
            }
            conn.execute("UPDATE entities SET resurface_kind='manual',resurface_command=NULL WHERE id='group'",[]).unwrap();
            assert!(matches!(
                open(&fixture.database).unwrap().1,
                Outcome::Current { .. }
            ));
            assert_eq!(fixture.backups().len(), 1);
        }
    }
}

#[test]
fn current_schema_is_noop_including_manual_migration_ddl() {
    for quoted in [false, true] {
        let ddl = if quoted {
            FRESH_SCHEMA.replace("CREATE TABLE entities", "CREATE TABLE \"entities\"")
        } else {
            FRESH_SCHEMA.to_owned()
        };
        let fixture = Fixture::with_schema(11, false, &ddl);
        let bytes = fs::read(&fixture.database).unwrap();
        assert!(matches!(
            open(&fixture.database).unwrap().1,
            Outcome::Current { .. }
        ));
        assert_eq!(fs::read(&fixture.database).unwrap(), bytes);
        assert!(fixture.backups().is_empty());
    }
}

#[test]
fn unknown_versions_and_structures_and_corruption_are_unchanged() {
    for change in [
        "PRAGMA user_version=8",
        "PRAGMA user_version=12",
        "PRAGMA user_version=0",
        "CREATE TABLE unexpected (id INTEGER)",
        "CREATE TRIGGER unexpected AFTER UPDATE ON entities BEGIN SELECT 1; END",
        "DROP INDEX idx_entity_events_entity",
        "PRAGMA ignore_check_constraints=ON; UPDATE entities SET kind='unknown'",
        "PRAGMA foreign_keys=OFF; INSERT INTO entity_deps VALUES ('missing','draft')",
    ] {
        let fixture = Fixture::new(9, false);
        Connection::open(&fixture.database)
            .unwrap()
            .execute_batch(change)
            .unwrap();
        let bytes = fs::read(&fixture.database).unwrap();
        let failure = open(&fixture.database).unwrap_err();
        assert_eq!(failure.applied, ApplicationState::NotApplied, "{change}");
        assert_eq!(fs::read(&fixture.database).unwrap(), bytes, "{change}");
        assert!(fixture.backups().is_empty(), "{change}");
    }
}

#[test]
fn backup_failure_never_changes_database() {
    let fixture = Fixture::new(9, false);
    fs::write(fixture.root.join(".axon/migration-backups"), "occupied").unwrap();
    let before = fs::read(&fixture.database).unwrap();
    let failure = open(&fixture.database).unwrap_err();
    assert_eq!(failure.applied, ApplicationState::NotApplied);
    assert_eq!(fs::read(&fixture.database).unwrap(), before);
}

#[test]
fn sql_errors_rollback_all_steps_and_retries_keep_backups() {
    for phase in [Phase::BackedUp, Phase::Step, Phase::BeforeCommit] {
        let fixture = Fixture::new(9, false);
        let before = snapshot(&Connection::open(&fixture.database).unwrap(), 9);
        let error = open_with_hook(&fixture.database, |at| {
            if at == phase {
                Err(invalid("injected error"))
            } else {
                Ok(())
            }
        })
        .unwrap_err();
        assert_eq!(error.applied, ApplicationState::NotApplied);
        let conn = Connection::open(&fixture.database).unwrap();
        assert_eq!(version(&conn).unwrap(), 9);
        assert_eq!(snapshot(&conn, 9), before);
        integrity(&conn).unwrap();
        let first = error.backup.unwrap();
        let first_bytes = fs::read(&first).unwrap();
        open(&fixture.database).unwrap();
        assert_eq!(fs::read(first).unwrap(), first_bytes);
        assert_eq!(fixture.backups().len(), 2);
    }
}

#[test]
fn writer_between_observation_and_lock_is_included_in_backup() {
    for wal in [false, true] {
        let fixture = Fixture::new(9, wal);
        let writer = Connection::open(&fixture.database).unwrap();
        let (conn, outcome) = open_with_hook(&fixture.database, |phase| {
            if phase == Phase::Observed {
                writer.execute(
                    "UPDATE meta SET value='new generation' WHERE key='custom'",
                    [],
                )?;
            }
            if phase == Phase::BackedUp {
                writer.busy_timeout(Duration::ZERO)?;
                assert!(
                    writer
                        .execute("UPDATE meta SET value='must wait' WHERE key='custom'", [])
                        .is_err()
                );
            }
            Ok(())
        })
        .unwrap();
        let Outcome::Migrated { backup, .. } = outcome else {
            panic!()
        };
        assert_eq!(
            snapshot(&conn, 9),
            snapshot(&Connection::open(backup).unwrap(), 9)
        );
    }
}

#[test]
fn concurrent_migrators_recheck_version_after_waiting() {
    for wal in [false, true] {
        let fixture = Fixture::new(9, wal);
        let gate = Arc::new(Barrier::new(2));
        let workers: Vec<_> = (0..2)
            .map(|_| {
                let gate = gate.clone();
                let path = fixture.database.clone();
                std::thread::spawn(move || {
                    open_with_hook(&path, |phase| {
                        if phase == Phase::Observed {
                            gate.wait();
                        }
                        Ok(())
                    })
                    .unwrap()
                    .1
                })
            })
            .collect();
        let outcomes: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
        assert_eq!(
            outcomes
                .iter()
                .filter(|o| matches!(o, Outcome::Migrated { .. }))
                .count(),
            1
        );
        assert_eq!(fixture.backups().len(), 1);
    }
}

fn child(fixture: &Fixture, mode: &str, marker: &Path) -> std::process::Child {
    Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "db::migration::tests::child_process",
            "--ignored",
            "--nocapture",
        ])
        .env("AXON_MIGRATION_TEST_DB", &fixture.database)
        .env("AXON_MIGRATION_TEST_MODE", mode)
        .env("AXON_MIGRATION_TEST_MARKER", marker)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

#[test]
#[ignore = "subprocess helper"]
fn child_process() {
    let database = PathBuf::from(std::env::var_os("AXON_MIGRATION_TEST_DB").unwrap());
    let mode = std::env::var("AXON_MIGRATION_TEST_MODE").unwrap();
    let marker = PathBuf::from(std::env::var_os("AXON_MIGRATION_TEST_MARKER").unwrap());
    let (_, outcome) = open_with_hook(&database, |phase| {
        if format!("{phase:?}") == mode {
            fs::write(&marker, "reached")?;
            loop {
                std::thread::park();
            }
        }
        Ok(())
    })
    .unwrap();
    fs::write(marker, format!("{outcome:?}")).unwrap();
}

#[test]
fn killed_process_recovers_old_database_and_preserves_backup() {
    for wal in [false, true] {
        for phase in ["BackedUp", "Step", "BeforeCommit"] {
            let fixture = Fixture::new(9, wal);
            let before = snapshot(&Connection::open(&fixture.database).unwrap(), 9);
            let marker = fixture.root.join("marker");
            let mut process = child(&fixture, phase, &marker);
            let deadline = Instant::now() + Duration::from_secs(10);
            while !marker.exists() {
                assert!(
                    process.try_wait().unwrap().is_none(),
                    "child exited before checkpoint"
                );
                if Instant::now() >= deadline {
                    process.kill().unwrap();
                    panic!("child checkpoint timeout");
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            process.kill().unwrap();
            assert!(!process.wait().unwrap().success());
            let conn = Connection::open(&fixture.database).unwrap();
            assert_eq!(version(&conn).unwrap(), 9);
            assert_eq!(snapshot(&conn, 9), before);
            integrity(&conn).unwrap();
            let backups = fixture.backups();
            assert_eq!(backups.len(), 1);
            assert_eq!(snapshot(&Connection::open(&backups[0]).unwrap(), 9), before);
            open(&fixture.database).unwrap();
            assert_eq!(fixture.backups().len(), 2);
        }
    }
}

#[test]
fn parallel_processes_apply_once() {
    let fixture = Fixture::new(10, true);
    let first = child(&fixture, "run", &fixture.root.join("first"));
    let second = child(&fixture, "run", &fixture.root.join("second"));
    for process in [first, second] {
        let output = process.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert_eq!(fixture.backups().len(), 1);
    assert_eq!(
        version(&Connection::open(&fixture.database).unwrap()).unwrap(),
        11
    );
}

#[test]
fn every_supported_schema_has_a_contiguous_path() {
    assert_eq!(SCHEMAS.len() as i64, SCHEMA_VERSION - OLDEST_VERSION + 1);
    for version in OLDEST_VERSION..SCHEMA_VERSION {
        let fixture = Fixture::new(version, false);
        assert!(open(&fixture.database).is_ok());
    }
}

#[test]
fn distinct_sql_tokens_are_not_equivalent_schema() {
    let ddl = schema(9)
        .unwrap()
        .replace("title TEXT NOT NULL", "title TEXTNOTNULL");
    let fixture = Fixture::with_schema(9, false, &ddl);
    let before = fs::read(&fixture.database).unwrap();
    assert!(open(&fixture.database).is_err());
    assert_eq!(fs::read(&fixture.database).unwrap(), before);
    assert!(fixture.backups().is_empty());
    assert_ne!(
        normalize("CHECK (body = 'a b')"),
        normalize("CHECK (body = 'ab')")
    );
    assert_ne!(
        normalize("CHECK (body = 'a'' b')"),
        normalize("CHECK (body = 'a''b')")
    );
}

#[test]
fn legacy_check_loophole_causes_sql_error_without_partial_migration() {
    let fixture = Fixture::new(9, false);
    let conn = Connection::open(&fixture.database).unwrap();
    conn.execute(
        "UPDATE entities SET resurface_kind=NULL WHERE id='draft'",
        [],
    )
    .unwrap();
    integrity(&conn).unwrap();
    let before = snapshot(&conn, 9);
    let failure = open(&fixture.database).unwrap_err();
    assert!(matches!(*failure.source, DbError::Sqlite(_)));
    assert_eq!(failure.applied, ApplicationState::NotApplied);
    assert_eq!(version(&conn).unwrap(), 9);
    assert_eq!(snapshot(&conn, 9), before);
    assert_eq!(
        snapshot(&Connection::open(failure.backup.unwrap()).unwrap(), 9),
        before
    );
}

#[test]
fn bare_relative_database_path_migrates() {
    let fixture = Fixture::new(9, false);
    let marker = fixture.root.join("relative");
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "db::migration::tests::child_process",
            "--ignored",
        ])
        .current_dir(fixture.database.parent().unwrap())
        .env("AXON_MIGRATION_TEST_DB", "axon.db")
        .env("AXON_MIGRATION_TEST_MODE", "run")
        .env("AXON_MIGRATION_TEST_MARKER", &marker)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(fs::read_to_string(marker).unwrap().contains("Migrated"));
    assert_eq!(fixture.backups().len(), 1);
}
