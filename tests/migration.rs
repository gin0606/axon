mod common;

use common::{TestRepo, assert_failure, assert_success, stderr, stdout};
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
    } else if version == 10 {
        include_str!("../src/db/schema_v10.sql")
    } else {
        include_str!("../src/db/schema_v11.sql")
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
fn version(path: &Path) -> i64 {
    Connection::open(path)
        .unwrap()
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap()
}
fn migrate(repo: &TestRepo, source: &Path, output: &Path) -> std::process::Output {
    repo.axon(&[
        "migrate",
        "--source",
        source.to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
    ])
}

#[test]
fn manual_conversion_is_deterministic_and_preserves_all_values() {
    let repo = TestRepo::new();
    let source = fixture(repo.root(), 11);
    let before = snapshot(&source, 11);
    let original = fs::read(&source).unwrap();
    let a = repo.root().join("converted-a");
    let b = repo.root().join("converted-b");
    assert_success(&migrate(&repo, &source, &a));
    assert_success(&migrate(&repo, &source, &b));
    assert_eq!(fs::read(&source).unwrap(), original);
    assert_eq!(version(&source), 11);
    assert_eq!(snapshot(&a.join("source-v11.db"), 11), before);
    assert_eq!(
        snapshot(&a.join("axon.db"), 12),
        snapshot(&b.join("axon.db"), 12)
    );
    assert_eq!(
        fs::read(a.join("manifest.yaml")).unwrap(),
        fs::read(b.join("manifest.yaml")).unwrap()
    );
    let c = Connection::open(a.join("axon.db")).unwrap();
    // Independent field projection through the persisted mapping (including nullable references).
    let old = Connection::open(&source).unwrap();
    for table in before.keys() {
        let cols: Vec<String> = old
            .prepare(&format!("PRAGMA table_info({table})"))
            .unwrap()
            .query_map([], |r| r.get(1))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let cols_sql = cols.iter().map(|col| {
            if (table == "entities" && col == "current_revision") || (["declaration_revisions", "revision_dependencies", "entity_events"].contains(&table.as_str()) && col == "revision") {
                format!("(SELECT sequence FROM declaration_revisions r WHERE r.revision=t.{col}) AS {col}")
            } else { format!("t.{col}") }
        }).collect::<Vec<_>>().join(",");
        let filter = if table == "meta" {
            " WHERE key <> 'store_id'"
        } else {
            ""
        };
        let rows: Vec<Vec<Value>> = c
            .prepare(&format!(
                "SELECT {cols_sql} FROM {table} t{filter} ORDER BY {}",
                cols.join(",")
            ))
            .unwrap()
            .query_map([], |r| {
                (0..cols.len())
                    .map(|i| r.get(i))
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(rows, before[table], "{table}");
    }
    assert_failure(&migrate(&repo, &source, &a));
}

#[test]
fn old_open_never_migrates_and_unknown_inputs_are_unchanged() {
    for schema in [9, 10, 11, 999] {
        let repo = TestRepo::new();
        let source = fixture(repo.root(), schema);
        let before = fs::read(&source).unwrap();
        let result = repo.axon(&["list"]);
        assert_failure(&result);
        assert!(stderr(&result).contains("unsupported axon schema"));
        assert_eq!(fs::read(&source).unwrap(), before);
        if schema != 11 {
            assert_failure(&migrate(&repo, &source, &repo.root().join("out")));
        }
        assert_eq!(fs::read(&source).unwrap(), before);
    }
    let repo = TestRepo::new();
    let source = fixture(repo.root(), 11);
    Connection::open(&source)
        .unwrap()
        .execute_batch("CREATE TABLE unknown(data TEXT)")
        .unwrap();
    let before = fs::read(&source).unwrap();
    assert_failure(&migrate(&repo, &source, &repo.root().join("out")));
    assert_eq!(fs::read(&source).unwrap(), before);
}

#[test]
fn wal_and_failed_conversion_preserve_source_and_do_not_publish_completion() {
    let repo = TestRepo::new();
    let source = fixture(repo.root(), 11);
    let conn = Connection::open(&source).unwrap();
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; UPDATE entity_notes SET body='committed in WAL' WHERE note=1").unwrap();
    let bytes = fs::read(&source).unwrap();
    let wal = fs::read(source.with_extension("db-wal")).unwrap();
    let out = repo.root().join("out");
    assert_success(&migrate(&repo, &source, &out));
    assert_eq!(fs::read(&source).unwrap(), bytes);
    assert_eq!(fs::read(source.with_extension("db-wal")).unwrap(), wal);
    assert_eq!(
        snapshot(&out.join("source-v11.db"), 11),
        snapshot(&source, 11)
    );
    conn.execute("UPDATE entity_notes SET at='invalid time'", [])
        .unwrap();
    let before = snapshot(&source, 11);
    let failed = repo.root().join("failed");
    assert_failure(&migrate(&repo, &source, &failed));
    assert!(!failed.join("manifest.yaml").exists());
    assert_eq!(snapshot(&source, 11), before);
}

#[test]
fn converted_database_supports_normal_commands_without_remapping_ids() {
    let repo = TestRepo::new();
    let source = fixture(repo.root(), 11);
    let out = repo.root().join("out");
    assert_success(&migrate(&repo, &source, &out));
    fs::rename(&source, repo.root().join("old.db")).unwrap();
    fs::copy(out.join("axon.db"), &source).unwrap();
    for args in [
        &["list"][..],
        &["show", "accepted"],
        &["log", "accepted"],
        &["revision", "list", "accepted"],
        &["note", "list", "accepted"],
        &["export", "accepted"],
    ] {
        assert_success(&repo.axon(args));
    }
    let added = repo.axon(&["note", "add", "accepted", "-m", "new note"]);
    assert_success(&added);
    let id = stdout(&added)
        .split_whitespace()
        .nth(2)
        .unwrap()
        .to_string();
    assert!(id.starts_with("note-"));
    assert_success(&repo.axon(&["note", "show", "accepted", &id]));
    assert_success(&repo.axon(&["note", "show", "accepted", &id[5..13]]));
    assert_failure(&repo.axon(&["note", "show", "accepted", "1"]));
    let before = snapshot(&source, 12);
    assert_failure(&migrate(&repo, &source, &repo.root().join("twice")));
    assert_eq!(snapshot(&source, 12), before);
}

#[test]
fn physical_layout_does_not_define_migrated_identity() {
    let repo = TestRepo::new();
    let source = fixture(repo.root(), 11);
    let a = repo.root().join("a");
    assert_success(&migrate(&repo, &source, &a));
    let conn = Connection::open(&source).unwrap();
    conn.execute_batch("PRAGMA page_size=8192; VACUUM").unwrap();
    drop(conn);
    let b = repo.root().join("b");
    assert_success(&migrate(&repo, &source, &b));
    assert_ne!(
        fs::read(a.join("source-v11.db")).unwrap(),
        fs::read(b.join("source-v11.db")).unwrap()
    );
    assert_eq!(
        snapshot(&a.join("axon.db"), 12),
        snapshot(&b.join("axon.db"), 12)
    );
}

#[test]
fn ambiguous_ids_and_duplicate_record_identity_are_rejected() {
    let repo = TestRepo::new();
    repo.init("t");
    let id = repo.plan("task");
    assert_success(&repo.axon(&["note", "add", &id, "-m", "same"]));
    assert_success(&repo.axon(&["note", "add", &id, "-m", "same"]));
    repo.execute_batch("UPDATE entity_notes SET record_id=CASE note WHEN 1 THEN 'note-abcd0000000000000000000000000001' ELSE 'note-abcd0000000000000000000000000002' END");
    let result = repo.axon(&["note", "show", &id, "abcd"]);
    assert_failure(&result);
    assert!(stderr(&result).contains("ambiguous"));
    assert_success(&repo.axon(&["note", "show", &id, "note-abcd0000000000000000000000000001"]));
    let conn = Connection::open(repo.root().join(".axon/axon.db")).unwrap();
    assert!(conn.execute("UPDATE entity_notes SET record_id='note-abcd0000000000000000000000000001' WHERE note=2",[]).is_err());
}
