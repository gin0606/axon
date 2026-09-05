mod common;

use common::{TestDir, TestRepo, assert_failure, assert_success, stderr, stdout};
use rusqlite::{Connection, types::Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

fn fixture(root: &Path, version: i64) -> PathBuf {
    fs::create_dir_all(root.join(".axon")).unwrap();
    let path = root.join(".axon/axon.db");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(if version == 9 {
        include_str!("../src/db/schema_v9.sql")
    } else {
        include_str!("../src/db/schema_v10.sql")
    })
    .unwrap();
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
UPDATE entities SET progress='in_progress', claimed_actor='previous actor', claimed_worktree='/synthetic/worktree', claimed_at='2026-02-01T00:00:00Z' WHERE id IN ('accepted','group');
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
    if version == 10 {
        conn.execute_batch("UPDATE entities SET resurface_kind='command',resurface_command='exit 1' WHERE id='group'").unwrap();
    }
    path
}

type Snapshot = BTreeMap<String, Vec<Vec<Value>>>;
fn snapshot(path: &Path, version: i64) -> Snapshot {
    let conn = Connection::open(path).unwrap();
    let tables: Vec<String> = conn.prepare("SELECT name FROM sqlite_schema WHERE type='table' AND name NOT GLOB 'sqlite_*' ORDER BY name").unwrap().query_map([], |r| r.get(0)).unwrap().collect::<Result<_,_>>().unwrap();
    tables
        .into_iter()
        .map(|table| {
            let columns: Vec<String> = conn
                .prepare(&format!("PRAGMA table_info({table})"))
                .unwrap()
                .query_map([], |r| r.get(1))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            let columns: Vec<_> = columns
                .into_iter()
                .filter(|c| !(version == 9 && c == "resurface_command"))
                .collect();
            let names = columns.join(",");
            let rows = conn
                .prepare(&format!("SELECT {names} FROM {table} ORDER BY {names}"))
                .unwrap()
                .query_map([], |r| {
                    (0..columns.len())
                        .map(|i| r.get(i))
                        .collect::<rusqlite::Result<Vec<Value>>>()
                })
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            (table, rows)
        })
        .collect()
}
fn backups(root: &Path) -> Vec<PathBuf> {
    fs::read_dir(root.join(".axon/migration-backups"))
        .map(|d| d.map(|p| p.unwrap().path()).collect())
        .unwrap_or_default()
}
fn version(path: &Path) -> i64 {
    Connection::open(path)
        .unwrap()
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap()
}
fn migrated(output: &std::process::Output, from: i64) {
    assert!(
        stderr(output).contains(&format!("Migrated database v{from} -> v11; backup:")),
        "{}",
        stderr(output)
    );
    assert!(!stdout(output).contains("Migrated database"));
}

#[test]
fn first_reads_preserve_all_tables_and_repeated_output() {
    for from in [9, 10] {
        for args in [
            vec!["list"],
            vec!["show", "accepted"],
            vec!["claims"],
            vec!["log", "accepted"],
            vec!["note", "list", "accepted"],
            vec!["revision", "show", "accepted", "2"],
            vec!["export", "accepted"],
        ] {
            let repo = TestRepo::new();
            let path = fixture(repo.root(), from);
            let before = snapshot(&path, from);
            let first = repo.axon(&args);
            assert_success(&first);
            migrated(&first, from);
            assert_eq!(version(&path), 11);
            assert_eq!(snapshot(&path, from), before);
            let backup = backups(repo.root());
            assert_eq!(backup.len(), 1);
            assert_eq!(version(&backup[0]), from);
            assert_eq!(snapshot(&backup[0], from), before);
            let again = repo.axon(&args);
            assert_success(&again);
            assert_eq!(first.stdout, again.stdout);
            assert!(again.stderr.is_empty());
            assert_eq!(backups(repo.root()), backup);
        }
    }
}

#[test]
fn writes_and_import_paths_continue_after_migration() {
    for from in [9, 10] {
        for args in [
            vec!["capture", "new task"],
            vec!["note", "add", "accepted", "-m", "new note"],
        ] {
            let repo = TestRepo::new();
            let path = fixture(repo.root(), from);
            let before = snapshot(&path, from);
            let output = repo.axon(&args);
            assert_success(&output);
            migrated(&output, from);
            assert_ne!(snapshot(&path, from), before);
            assert_eq!(snapshot(&backups(repo.root())[0], from), before);
        }
        for command in ["prepare", "check", "apply"] {
            let repo = TestRepo::new();
            let path = fixture(repo.root(), from);
            let before = snapshot(&path, from);
            let file = repo.root().join("plan.yaml");
            fs::write(&file, "schema: axon-plan/v2\nissues: []\ngroups: []\nrelations:\n  editable:\n    parents: []\n    dependencies: []\n  readonly:\n    parents: []\n    dependencies: []\nreferences:\n  entities: []\n").unwrap();
            let output = repo.axon(&["import", command, file.to_str().unwrap()]);
            assert_success(&output);
            migrated(&output, from);
            assert_eq!(version(&path), 11);
            assert_eq!(snapshot(&path, from), before);
        }
    }
}

#[test]
fn root_resolution_migrates_shared_database_once() {
    let repo = TestRepo::new();
    let path = fixture(repo.root(), 9);
    let worktree = repo.add_worktree();
    let output = repo.axon_in(&worktree, &["list"]);
    assert_success(&output);
    migrated(&output, 9);
    assert_eq!(version(&path), 11);
    assert!(!worktree.join(".axon").exists());
    assert!(repo.axon(&["list"]).stderr.is_empty());
    assert_eq!(backups(repo.root()).len(), 1);
    let dir = TestDir::new("nongit-migration");
    let path = fixture(dir.path(), 10);
    let nested = dir.path().join("one/two");
    fs::create_dir_all(&nested).unwrap();
    let output = dir.axon_in(&nested, &["list"]);
    assert_success(&output);
    migrated(&output, 10);
    assert_eq!(version(&path), 11);
    assert_eq!(backups(dir.path()).len(), 1);
}

#[test]
fn utilities_and_init_do_not_migrate() {
    let repo = TestRepo::new();
    let path = fixture(repo.root(), 9);
    let before = fs::read(&path).unwrap();
    for args in [
        vec!["--help"],
        vec!["docs"],
        vec!["--version"],
        vec!["completion", "bash"],
    ] {
        let output = repo.axon(&args);
        assert_success(&output);
        assert!(output.stderr.is_empty());
        assert_eq!(fs::read(&path).unwrap(), before);
    }
    assert_failure(&repo.axon(&["init"]));
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(backups(repo.root()).is_empty());
}

#[test]
fn failures_stop_original_operation_and_committed_migration_is_distinct() {
    for corrupt in [false, true] {
        let repo = TestRepo::new();
        let path = fixture(repo.root(), 9);
        if corrupt {
            Connection::open(&path)
                .unwrap()
                .execute_batch("CREATE TABLE unexpected (id)")
                .unwrap();
        } else {
            fs::write(repo.root().join(".axon/migration-backups"), "obstruction").unwrap();
        }
        let before = fs::read(&path).unwrap();
        let output = repo.axon(&["capture", "must not exist"]);
        assert_failure(&output);
        assert!(output.stdout.is_empty());
        assert!(stderr(&output).contains("requested operation did not run"));
        assert!(stderr(&output).contains(if corrupt {
            "unknown structure"
        } else {
            "saving backup"
        }));
        assert_eq!(fs::read(&path).unwrap(), before);
    }
    let repo = TestRepo::new();
    let path = fixture(repo.root(), 9);
    let before = snapshot(&path, 9);
    let output = repo.axon(&["show", "missing"]);
    assert_failure(&output);
    migrated(&output, 9);
    assert!(stderr(&output).contains("does not exist"));
    assert!(!stderr(&output).contains("Migration was not applied"));
    assert_eq!(version(&path), 11);
    assert_eq!(snapshot(&path, 9), before);
}

#[test]
fn lock_and_permission_failures_report_the_actual_cause() {
    let repo = TestRepo::new();
    let path = fixture(repo.root(), 9);
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch("BEGIN IMMEDIATE").unwrap();
    let output = repo.axon(&["capture", "must not exist"]);
    assert_failure(&output);
    assert!(stderr(&output).contains("acquiring migration lock"));
    assert!(stderr(&output).contains("other writers"));
    assert_eq!(version(&path), 9);
    conn.execute_batch("ROLLBACK").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = fs::metadata(&path).unwrap().permissions();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();
        let output = repo.axon(&["capture", "must not exist"]);
        fs::set_permissions(&path, permissions).unwrap();
        assert_failure(&output);
        assert!(stderr(&output).contains("permissions"));
        assert!(stderr(&output).contains("requested operation did not run"));
        assert_eq!(version(&path), 9);
    }
}
