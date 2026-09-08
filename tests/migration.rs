mod common;
use common::{TestRepo, assert_failure, assert_success, stderr};
use rusqlite::Connection;
use std::{fs, path::Path};

#[test]
fn ordinary_open_migrates_v13_dates_and_preserves_sqlite_history() {
    let repo = TestRepo::new();
    repo.init("legacy");
    let issue = repo.plan("AtDate(2026-01-02)");
    assert_success(&repo.axon(&["note", "add", &issue, "-m", "kept note"]));
    assert_success(&repo.axon(&[
        "when",
        "at",
        &issue,
        "2099-01-02T00:00:00Z",
        "-r",
        "kept reason",
    ]));

    let path = repo.root().join(".axon/axon.db");
    let conn = Connection::open(&path).unwrap();
    let counts = |conn: &Connection| {
        [
            "entity_notes",
            "declaration_revisions",
            "entity_events",
            "causal_links",
        ]
        .map(|table| {
            conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap()
        })
    };
    let before_counts = counts(&conn);
    conn.execute_batch(
        "UPDATE entities SET resurface_date='2099-01-02' WHERE resurface_kind='date';
         UPDATE entity_events SET new_value='AtDate(2099-01-02)'
           WHERE new_value='AtDate(2099-01-02T00:00:00Z)';
         UPDATE entity_events SET old_value='AtDate(2099-01-02)'
           WHERE old_value='AtDate(2099-01-02T00:00:00Z)';
         UPDATE causal_links SET payload=replace(payload,'2099-01-02T00:00:00Z','2099-01-02');
         UPDATE history_baselines SET payload=replace(payload,'2099-01-02T00:00:00Z','2099-01-02');
         UPDATE history_merges SET payload=replace(payload,'2099-01-02T00:00:00Z','2099-01-02');
         PRAGMA user_version=13;",
    )
    .unwrap();
    drop(conn);

    let result = repo.axon(&["show", &issue, "--skip-command-evaluation"]);
    assert_success(&result);
    assert!(stderr(&result).contains("v13 -> v14"));
    let conn = Connection::open(&path).unwrap();
    assert_eq!(
        conn.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        14
    );
    assert_eq!(counts(&conn), before_counts);
    assert_eq!(
        conn.query_row("SELECT title FROM entities WHERE id=?1", [&issue], |row| {
            row.get::<_, String>(0)
        })
        .unwrap(),
        "AtDate(2026-01-02)"
    );
    assert_eq!(
        conn.query_row(
            "SELECT resurface_date FROM entities WHERE id=?1",
            [&issue],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        "2099-01-02T00:00:00Z"
    );
    assert_eq!(
        conn.query_row(
            "SELECT new_value FROM entity_events WHERE entity_id=?1 AND field='resurface_condition'",
            [&issue],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        "AtDate(2099-01-02T00:00:00Z)"
    );
    assert!(
        conn.query_row(
            "SELECT payload FROM causal_links WHERE payload LIKE '%' || ?1 || '%' ORDER BY rowid DESC LIMIT 1",
            [&issue],
            |row| row.get::<_, String>(0),
        )
        .unwrap()
        .contains("2099-01-02T00:00:00Z")
    );
    assert_eq!(
        fs::read_dir(repo.root().join(".axon/migration-backups"))
            .unwrap()
            .count(),
        1
    );
}

fn convert(repo: &TestRepo, source: &Path, output: &Path, backend: &str) -> std::process::Output {
    repo.axon(&[
        "migrate",
        "--source",
        source.to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
        "--backend",
        backend,
    ])
}
#[test]
fn current_backend_conversion_preserves_state_and_source() {
    let repo = TestRepo::new();
    repo.init("conversion");
    let issue = repo.plan("kept declaration");
    assert_success(&repo.axon(&["note", "add", &issue, "-m", "kept note"]));
    assert_success(&repo.axon(&["start", &issue]));
    let source = repo.root().join(".axon/axon.db");
    let before = fs::read(&source).unwrap();
    let mut snapshots = Vec::new();
    for backend in ["sqlite", "file"] {
        let output = repo.root().join(backend);
        assert_success(&convert(&repo, &source, &output, backend));
        assert_eq!(fs::read(&source).unwrap(), before);
        let snapshot = fs::read(output.join("snapshot.jsonl")).unwrap();
        let text = String::from_utf8(snapshot.clone()).unwrap();
        assert!(text.contains("kept note"));
        assert!(text.contains("kept declaration"));
        assert!(text.contains("InProgress"));
        assert_success(&repo.axon(&[
            "storage",
            "check",
            output.join("snapshot.jsonl").to_str().unwrap(),
        ]));
        let manifest: serde_json::Value =
            serde_saphyr::from_str(&fs::read_to_string(output.join("manifest.yaml")).unwrap())
                .unwrap();
        assert_eq!(manifest["source_schema"], 14);
        assert_eq!(manifest["target_schema"], 14);
        assert_eq!(manifest["row_counts"], manifest["target_row_counts"]);
        assert_eq!(manifest["mappings"], serde_json::json!([]));
        assert!(!output.join("staging").exists());
        assert_failure(&convert(&repo, &source, &output, backend));
        if backend == "file" {
            assert_eq!(fs::read(output.join("state.jsonl")).unwrap(), snapshot);
        } else {
            let again = repo.root().join("again");
            assert_success(&convert(&repo, &output.join("axon.db"), &again, "file"));
            assert_eq!(fs::read(again.join("snapshot.jsonl")).unwrap(), snapshot);
        }
        snapshots.push(snapshot);
    }
    assert_eq!(snapshots[0], snapshots[1]);
}
#[test]
fn incompatible_schema_is_rejected_without_output_or_source_update() {
    for schema in [9, 10, 11, 12, 13, 999] {
        let repo = TestRepo::new();
        repo.init("old");
        let source = repo.root().join(".axon/axon.db");
        Connection::open(&source)
            .unwrap()
            .pragma_update(None, "user_version", schema)
            .unwrap();
        let before = fs::read(&source).unwrap();
        for backend in ["sqlite", "file"] {
            let output = repo.root().join(backend);
            let result = convert(&repo, &source, &output, backend);
            assert_failure(&result);
            assert!(stderr(&result).contains("backend conversion requires current schema"));
            assert!(!output.exists());
            assert_eq!(fs::read(&source).unwrap(), before);
        }
    }
}
#[test]
fn unknown_structure_is_rejected_without_output() {
    let repo = TestRepo::new();
    repo.init("unknown");
    let source = repo.root().join(".axon/axon.db");
    Connection::open(&source)
        .unwrap()
        .execute_batch("CREATE TABLE extra(value TEXT)")
        .unwrap();
    let before = fs::read(&source).unwrap();
    let output = repo.root().join("out");
    assert_failure(&convert(&repo, &source, &output, "file"));
    assert!(!output.exists());
    assert_eq!(fs::read(source).unwrap(), before);
}

#[test]
fn ambiguous_ids_and_duplicate_record_identity_are_rejected() {
    let repo = TestRepo::new();
    repo.init("t");
    let id = repo.plan("task");
    assert_success(&repo.axon(&["note", "add", &id, "-m", "same"]));
    assert_success(&repo.axon(&["note", "add", &id, "-m", "same"]));
    for (number, new) in [
        (1, "note-abcd0000000000000000000000000001"),
        (2, "note-abcd0000000000000000000000000002"),
    ] {
        let old = repo.note_id(&id, number);
        let conn = Connection::open(repo.root().join(".axon/axon.db")).unwrap();
        conn.execute(
            "UPDATE causal_links SET record_id=?2 WHERE record_id=?1",
            [&old, new],
        )
        .unwrap();
        conn.execute(
            "UPDATE causal_links SET payload=replace(payload,?1,?2)",
            [&old, new],
        )
        .unwrap();
    }
    repo.execute_batch("UPDATE entity_notes SET record_id=CASE note WHEN 1 THEN 'note-abcd0000000000000000000000000001' ELSE 'note-abcd0000000000000000000000000002' END");
    let result = repo.axon(&["note", "show", &id, "abcd"]);
    assert_failure(&result);
    assert!(stderr(&result).contains("ambiguous"));
    assert_success(&repo.axon(&["note", "show", &id, "note-abcd0000000000000000000000000001"]));
    let conn = Connection::open(repo.root().join(".axon/axon.db")).unwrap();
    assert!(conn.execute("UPDATE entity_notes SET record_id='note-abcd0000000000000000000000000001' WHERE note=2",[]).is_err());
}

#[test]
fn current_wal_and_changed_source_keep_existing_identity_and_causal_payloads() {
    let repo = TestRepo::new();
    repo.init("current");
    let id = repo.plan("history");
    let source = repo.root().join(".axon/axon.db");
    let first = repo.root().join("first");
    assert_success(&convert(&repo, &source, &first, "file"));
    assert_success(&repo.axon(&["note", "add", &id, "-m", "additional record"]));
    let conn = Connection::open(&source).unwrap();
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; INSERT INTO meta VALUES ('wal-extra','retained');").unwrap();
    let bytes = fs::read(&source).unwrap();
    let wal = fs::read(source.with_extension("db-wal")).unwrap();
    let second = repo.root().join("second");
    assert_success(&convert(&repo, &source, &second, "file"));
    assert_eq!(fs::read(&source).unwrap(), bytes);
    assert_eq!(fs::read(source.with_extension("db-wal")).unwrap(), wal);
    let records = |dir: &Path| {
        fs::read_to_string(dir.join("state.jsonl"))
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
            .collect::<Vec<_>>()
    };
    let old = records(&first);
    let new = records(&second);
    for record in old
        .iter()
        .filter(|r| !["Entity", "Header"].contains(&r["type"].as_str().unwrap_or("")))
    {
        assert!(new.contains(record), "original record missing: {record}");
    }
    let config: serde_json::Value =
        serde_saphyr::from_str(&fs::read_to_string(first.join("manifest.yaml")).unwrap()).unwrap();
    let later: serde_json::Value =
        serde_saphyr::from_str(&fs::read_to_string(second.join("manifest.yaml")).unwrap()).unwrap();
    assert_eq!(config["store_id"], later["store_id"]);
}
