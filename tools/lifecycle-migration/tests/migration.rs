use axon::{
    file,
    lifecycle::{Context, Current, EntityId, Kind, Lifecycle},
    location::Location,
    sqlite,
};
use axon_lifecycle_migration::{
    convert::{self, Rules},
    job::{self, Backend, Boundary, Config, Input, Target},
    legacy::Legacy,
    sql, util,
};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let p = fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!(
                "axon-migration-test-{}",
                axon::lifecycle::RecordId::generate()
            ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn root(&self, name: &str) -> PathBuf {
        let p = self.0.join(name);
        fs::create_dir_all(p.join(".axon")).unwrap();
        p
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn now() -> DateTime<Utc> {
    "2026-09-13T01:02:03Z".parse().unwrap()
}
fn rid(label: &str) -> String {
    let prefix = if label == "old-store" {
        "store"
    } else {
        label.split_once("-").unwrap().0
    };
    format!("{}-{}", prefix, &util::digest(label.as_bytes())[..32])
}
fn fixture(schema: u32) -> Vec<Value> {
    let mut rows = vec![
        json!({"type":"Header","format":1,"schema":schema,"metadata":{
            "prefix":{"Text":"t"},"store_id":{"Text":"old-store"},"arbitrary":{"Bytes":[0,255,23]},
            "fraction":{"Real":1.25f64.to_bits()},"integer":{"Integer":i64::MAX}
        }}),
    ];
    for (id, kind, progress, disposition, parent, condition) in [
        (
            "t-group",
            "Group",
            json!("NotStarted"),
            "Rejected",
            None,
            json!("Always"),
        ),
        (
            "t-child",
            "Issue",
            json!("NotStarted"),
            "Undecided",
            Some("t-group"),
            json!("Manual"),
        ),
        (
            "t-done",
            "Issue",
            json!("Ended"),
            "Accepted",
            Some("t-group"),
            json!("Always"),
        ),
        (
            "t-work",
            "Issue",
            json!({"InProgress":{"actor":"a","worktree":"/old/path","at":now()}}),
            "Accepted",
            None,
            json!("Always"),
        ),
        (
            "t-wait",
            "Issue",
            json!("NotStarted"),
            "Accepted",
            None,
            json!({"AfterEntity":"t-work"}),
        ),
        (
            "t-date",
            "Issue",
            json!("NotStarted"),
            "Undecided",
            None,
            json!({"AtDate":if schema==13 {"2026-09-12"} else {"2026-09-12T09:00:00+09:00"}}),
        ),
        (
            "t-ready",
            "Issue",
            json!("NotStarted"),
            "Undecided",
            None,
            json!({"AfterEntity":"t-done"}),
        ),
        (
            "t-cmd",
            "Issue",
            json!("NotStarted"),
            "Undecided",
            None,
            json!({"Command":"exit 1"}),
        ),
    ] {
        let rev = if disposition == "Undecided" {
            Value::Null
        } else {
            json!(format!("rev-{id}"))
        };
        let entity = json!({"id":id,"kind":kind,"title":format!("Title {id}"),"description":"Line one\n日本語\n\u{001b}[31m", "progress":progress,"disposition":disposition,"current_revision":rev,"resurface_condition":condition,"parent":parent,"created_at":now(),"updated_at":now()});
        let proof = json!({"progress":progress,"disposition":disposition,"resurface":condition,"current_revision":rev,"last_revision":rev});
        let deps = if id == "t-wait" {
            json!(["t-work"])
        } else {
            json!([])
        };
        rows.push(json!({"type":"Entity","entity":entity,"dependencies":deps,"lineage":{"head":format!("baseline-{id}"),"last_revision":rev}}));
        if !rev.is_null() {
            rows.push(json!({"type":"Revision","value":{"id":rev,"title":entity["title"],"description":entity["description"],"parent":parent,"dependencies":deps,"created_at":now(),"baseline":true},"link":{"owner":id,"stream":"Revision","parents":[],"origin":{"Migration":{"schema":12}},"result":null}}));
        }
        rows.push(json!({"type":"Baseline","id":format!("baseline-{id}"),"value":{"bundle":{"entity":entity,"dependencies":deps,"last_revision":rev},"source_schema":12},"link":{"owner":id,"stream":"State","parents":[],"origin":{"Migration":{"schema":12}},"result":proof}}));
    }
    for (id, parents) in [
        ("note-root", vec![]),
        ("note-left", vec!["note-root"]),
        ("note-right", vec!["note-root"]),
    ] {
        rows.push(json!({"type":"Note","value":{"id":id,"body":format!("Body {id}\nOriginal content"),"actor":"original","created_at":now()},"link":{"owner":"t-work","stream":"Note","parents":parents,"origin":"Operation","result":null}}));
    }
    fn ids(v: &mut Value) {
        match v {
            Value::String(s)
                if s == "old-store"
                    || ["rev-t-", "baseline-t-", "note-"]
                        .iter()
                        .any(|p| s.starts_with(p)) =>
            {
                *s = rid(s)
            }
            Value::Array(a) => a.iter_mut().for_each(ids),
            Value::Object(o) => o.values_mut().for_each(ids),
            _ => (),
        }
    }
    rows.iter_mut().for_each(ids);
    rows
}
fn bytes(rows: &[Value]) -> Vec<u8> {
    rows.iter()
        .map(|v| format!("{v}\n"))
        .collect::<String>()
        .into_bytes()
}
fn rules() -> Rules {
    Rules {
        reviewed_commands: BTreeMap::from([("t-cmd".into(), "exit 1".into())]),
        ..Rules::default()
    }
}
fn sql_fixture(path: &Path, schema: u32) -> Connection {
    let db = Connection::open(path).unwrap();
    db.execute_batch(sql::SCHEMA).unwrap();
    db.pragma_update(None, "user_version", schema).unwrap();
    db.execute_batch("BEGIN; PRAGMA defer_foreign_keys=ON")
        .unwrap();
    let rows = fixture(schema);
    for (key, value) in rows[0]["metadata"].as_object().unwrap() {
        let val = if let Some(v) = value.get("Text") {
            rusqlite::types::Value::Text(v.as_str().unwrap().into())
        } else if let Some(v) = value.get("Integer") {
            rusqlite::types::Value::Integer(v.as_i64().unwrap())
        } else if let Some(v) = value.get("Real") {
            rusqlite::types::Value::Real(f64::from_bits(v.as_u64().unwrap()))
        } else {
            rusqlite::types::Value::Blob(vec![0, 255, 23])
        };
        db.execute(
            "INSERT INTO meta(key,value) VALUES (?1,?2)",
            params![key, val],
        )
        .unwrap();
    }
    let text = |v: &Value| v.as_str().map(str::to_owned);
    for row in &rows[1..] {
        let value = &row["value"];
        match row["type"].as_str().unwrap() {
            "Entity" => {
                let e = &row["entity"];
                let c = &e["resurface_condition"];
                let (kind, date, reference, command) = if c == "Always" {
                    (None, None, None, None)
                } else if c == "Manual" {
                    (Some("manual"), None, None, None)
                } else if let Some(v) = c.get("AtDate") {
                    (Some("date"), text(v), None, None)
                } else if let Some(v) = c.get("AfterEntity") {
                    (Some("after_entity"), None, text(v), None)
                } else {
                    (Some("command"), None, None, text(&c["Command"]))
                };
                let claim = &e["progress"]["InProgress"];
                db.execute("INSERT INTO entities(id,kind,title,description,progress,claimed_actor,claimed_worktree,claimed_at,disposition,current_revision,resurface_kind,resurface_date,resurface_ref,resurface_command,parent_id,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",params![text(&e["id"]),e["kind"].as_str().unwrap().to_lowercase(),text(&e["title"]),text(&e["description"]),if e["progress"]=="NotStarted" {"not_started"} else if e["progress"]=="Ended" {"ended"} else {"in_progress"},text(&claim["actor"]),text(&claim["worktree"]),text(&claim["at"]),e["disposition"].as_str().unwrap().to_lowercase(),text(&e["current_revision"]),kind,date,reference,command,text(&e["parent"]),text(&e["created_at"]),text(&e["updated_at"])]).unwrap();
                db.execute(
                    "INSERT INTO history_lineage VALUES (?1,?2)",
                    params![text(&e["id"]), row["lineage"].to_string()],
                )
                .unwrap();
                for dep in row["dependencies"].as_array().unwrap() {
                    db.execute(
                        "INSERT INTO entity_deps VALUES (?1,?2)",
                        params![text(&e["id"]), text(dep)],
                    )
                    .unwrap();
                }
            }
            "Revision" => {
                db.execute("INSERT INTO declaration_revisions(revision,entity_id,title,description,parent_id,created_at,baseline,sequence) VALUES (?1,?2,?3,?4,?5,?6,?7,1)",params![text(&value["id"]),text(&row["link"]["owner"]),text(&value["title"]),text(&value["description"]),text(&value["parent"]),text(&value["created_at"]),1]).unwrap();
                for dep in value["dependencies"].as_array().unwrap() {
                    db.execute(
                        "INSERT INTO revision_dependencies VALUES (?1,?2,?3)",
                        params![text(&row["link"]["owner"]), text(&value["id"]), text(dep)],
                    )
                    .unwrap();
                }
            }
            "Baseline" => {
                db.execute(
                    "INSERT INTO history_baselines VALUES (?1,?2)",
                    params![text(&row["id"]), value.to_string()],
                )
                .unwrap();
            }
            "Note" => {
                db.execute("INSERT INTO entity_notes(record_id,entity_id,body,actor,at,note) VALUES (?1,?2,?3,?4,?5,(SELECT count(*)+1 FROM entity_notes))",params![text(&value["id"]),text(&row["link"]["owner"]),text(&value["body"]),text(&value["actor"]),text(&value["created_at"])]).unwrap();
            }
            _ => unreachable!(),
        }
        if row.get("link").is_some() {
            db.execute(
                "INSERT INTO causal_links VALUES (?1,?2)",
                params![
                    text(row.get("id").unwrap_or(&value["id"])),
                    row["link"].to_string()
                ],
            )
            .unwrap();
        }
    }
    db.execute_batch("COMMIT").unwrap();
    db
}
fn config(targets: Vec<Target>) -> Config {
    Config {
        version: 1,
        axon_binary: fs::canonicalize(std::env::current_exe().unwrap()).unwrap(),
        legacy_binary: None,
        targets,
    }
}
fn target(ws: &Workspace, name: &str, backend: Backend, schema: u32) -> Target {
    let root = ws.root(name);
    let source = root.join(if backend == Backend::File {
        ".axon/state.jsonl"
    } else {
        ".axon/axon.db"
    });
    let store = format!("store-{}", &util::digest(name.as_bytes())[..32]);
    if backend == Backend::File {
        let mut rows = fixture(schema);
        rows[0]["metadata"]["store_id"] = json!({"Text":store});
        fs::write(&source, bytes(&rows)).unwrap();
    } else {
        let db = sql_fixture(&source, schema);
        db.execute("UPDATE meta SET value=?1 WHERE key='store_id'", [store])
            .unwrap();
    }
    Target {
        name: name.into(),
        root,
        source,
        backend,
        input: Input::Legacy,
        rules: rules(),
    }
}

#[test]
fn conversion_preserves_entities_original_records_notes_and_conditions() {
    for schema in [13, 14] {
        let raw = bytes(&fixture(schema));
        let old = Legacy::parse(&raw).unwrap();
        let (snapshot, report) =
            convert::run(&old, &util::digest(&raw), "fixed", now(), &rules()).unwrap();
        let get = |id: &str| {
            snapshot
                .entity(&EntityId::try_from(id.to_owned()).unwrap())
                .unwrap()
        };
        assert_eq!(get("t-child").current.lifecycle, Lifecycle::Cancelled);
        assert_eq!(get("t-done").current.lifecycle, Lifecycle::Completed);
        assert_eq!(get("t-work").current.lifecycle, Lifecycle::InProgress);
        assert_eq!(get("t-ready").current.condition, None);
        assert_eq!(get("t-child").current.condition.as_deref(), Some("exit 1"));
        assert!(
            get("t-date")
                .current
                .condition
                .as_ref()
                .unwrap()
                .contains("1789171200000000000")
        );
        let notes = snapshot.notes(&get("t-work").id).unwrap();
        assert_eq!(notes.len(), 4);
        let archive = notes
            .iter()
            .find(|n| n.id.to_string() == report.entities["t-work"].archive)
            .unwrap();
        assert_eq!(
            archive
                .parents
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            {
                let mut ids = vec![rid("note-left"), rid("note-right")];
                ids.sort();
                ids
            }
        );
        for row in old.global.iter().chain(&old.owned["t-work"]) {
            assert!(archive.body.contains(row));
        }
        let (again, _) = convert::run(&old, &util::digest(&raw), "fixed", now(), &rules()).unwrap();
        assert_eq!(snapshot, again);
        assert!(
            convert::run(&old, &util::digest(&raw), "fixed", now(), &Rules::default()).is_err()
        );
    }
}
#[test]
fn all_four_formats_round_trip_apply_and_restore() {
    for backend in [Backend::File, Backend::Sqlite] {
        for schema in [13, 14] {
            let ws = Workspace::new();
            let t = target(&ws, "root", backend, schema);
            let original = fs::read(&t.source).unwrap();
            let original_sql = if backend == Backend::Sqlite {
                Some(sql::dump(&t.source).unwrap().digest().unwrap())
            } else {
                None
            };
            let job = ws.0.join("job");
            let cfg = config(vec![t.clone()]);
            job::prepare(&cfg, &job).unwrap();
            assert_eq!(fs::read(&t.source).unwrap(), original);
            let candidate = fs::read(job.join("root/candidate.jsonl")).unwrap();
            job::prepare(&cfg, &job).unwrap();
            assert_eq!(
                fs::read(job.join("root/candidate.jsonl")).unwrap(),
                candidate
            );
            assert!(job::execute(&job, false, false).is_err());
            job::execute(&job, false, true).unwrap();
            job::execute(&job, false, true).unwrap();
            let (prefix, snapshot) = if backend == Backend::File {
                file::decode(&fs::read(&t.source).unwrap()).unwrap()
            } else {
                sqlite::Store::open(&t.source).unwrap().read().unwrap()
            };
            assert_eq!(file::encode(&prefix, &snapshot).unwrap(), candidate);
            job::execute(&job, true, true).unwrap();
            job::execute(&job, true, true).unwrap();
            if let Some(expected) = original_sql {
                assert_eq!(sql::dump(&t.source).unwrap().digest().unwrap(), expected);
            } else {
                assert_eq!(fs::read(&t.source).unwrap(), original);
            }
        }
    }
}
#[test]
fn wal_backup_keeps_committed_rows_and_cutover_requires_connections_closed() {
    let ws = Workspace::new();
    let t = target(&ws, "root", Backend::Sqlite, 13);
    let db = Connection::open(&t.source).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; INSERT INTO meta VALUES ('wal-only','kept');").unwrap();
    assert!(
        PathBuf::from(format!("{}-wal", t.source.display()))
            .metadata()
            .unwrap()
            .len()
            > 0
    );
    let job = ws.0.join("job");
    job::prepare(&config(vec![t.clone()]), &job).unwrap();
    assert!(
        fs::read_to_string(job.join("root/candidate.jsonl"))
            .unwrap()
            .contains("wal-only")
    );
    db.execute_batch("BEGIN; SELECT * FROM meta;").unwrap();
    assert!(job::execute(&job, false, true).is_err());
    db.execute_batch("ROLLBACK").unwrap();
    drop(db);
    job::execute(&job, false, true).unwrap();
    assert!(!PathBuf::from(format!("{}-wal", t.source.display())).exists());
    job::execute(&job, true, true).unwrap();
    assert!(
        sql::dump(&t.source)
            .unwrap()
            .legacy()
            .unwrap()
            .global
            .iter()
            .any(|r| r.contains("wal-only"))
    );
}
#[test]
fn crash_boundaries_resume_both_directions_without_duplicate_records() {
    for backend in [Backend::File, Backend::Sqlite] {
        for boundary in [
            Boundary::AfterIntent,
            Boundary::BeforeReplace,
            Boundary::AfterReplace,
        ] {
            let ws = Workspace::new();
            let t = target(&ws, "root", backend, 14);
            let job = ws.0.join("job");
            job::prepare(&config(vec![t]), &job).unwrap();
            for restore in [false, true] {
                let error = job::execute_with(&job, restore, true, |_, at| {
                    if at == boundary {
                        Err("injected failure".into())
                    } else {
                        Ok(())
                    }
                })
                .unwrap_err();
                assert!(error.to_string().contains("injected failure"));
                job::execute(&job, restore, true).unwrap();
                job::execute(&job, restore, true).unwrap();
            }
        }
    }
}
#[test]
fn drift_tampering_and_duplicate_roots_are_rejected() {
    let ws = Workspace::new();
    let t = target(&ws, "root", Backend::File, 14);
    let cfg = config(vec![t.clone()]);
    let job = ws.0.join("job");
    assert!(job::prepare(&config(vec![t.clone(), t.clone()]), &job).is_err());
    job::prepare(&cfg, &job).unwrap();
    let before = fs::read(&t.source).unwrap();
    fs::write(&t.source, [before.as_slice(), b"\n"].concat()).unwrap();
    assert!(job::execute(&job, false, true).is_err());
    fs::write(&t.source, &before).unwrap();
    let manifest = fs::read(job.join("root/prepared.json")).unwrap();
    fs::write(job.join("root/prepared.json"), b"{}").unwrap();
    assert!(job::execute(&job, false, true).is_err());
    fs::write(job.join("root/prepared.json"), manifest).unwrap();
    let candidate = fs::read(job.join("root/candidate")).unwrap();
    fs::write(job.join("root/candidate"), b"bad").unwrap();
    assert!(job::execute(&job, false, true).is_err());
    fs::write(job.join("root/candidate"), candidate).unwrap();
    job::execute(&job, false, true).unwrap();
    let mut store = Location::discover(&t.root, false).unwrap().open().unwrap();
    store
        .update(|_, s| {
            s.create(
                EntityId::generate("t"),
                Kind::Issue,
                Current {
                    title: "new write".into(),
                    description: String::new(),
                    lifecycle: Lifecycle::Undecided,
                    condition: None,
                    parent: None,
                    dependencies: Default::default(),
                },
                Context {
                    at: now(),
                    recorder: None,
                },
            )?;
            Ok(())
        })
        .unwrap();
    let changed = fs::read(&t.source).unwrap();
    assert!(
        job::execute(&job, true, true)
            .unwrap_err()
            .to_string()
            .contains("retain new writes")
    );
    assert_eq!(fs::read(&t.source).unwrap(), changed);
}
#[test]
fn partial_job_stops_and_current_input_is_preserved_exactly() {
    let ws = Workspace::new();
    let a = target(&ws, "a", Backend::File, 14);
    let b = target(&ws, "b", Backend::Sqlite, 13);
    let untouched = fs::read(&b.source).unwrap();
    let job = ws.0.join("job");
    job::prepare(&config(vec![a.clone(), b.clone()]), &job).unwrap();
    assert!(
        job::execute_with(&job, false, true, |name, at| {
            if name == "a" && at == Boundary::AfterReplace {
                Err("stopped".into())
            } else {
                Ok(())
            }
        })
        .is_err()
    );
    assert_eq!(fs::read(&b.source).unwrap(), untouched);
    job::execute(&job, false, true).unwrap();
    let saved = fs::read(&a.source).unwrap();
    let current = Target {
        input: Input::Current,
        rules: Rules::default(),
        ..a
    };
    let job2 = ws.0.join("job2");
    job::prepare(&config(vec![current.clone()]), &job2).unwrap();
    job::execute(&job2, false, true).unwrap();
    job::execute(&job2, true, true).unwrap();
    assert_eq!(fs::read(&current.source).unwrap(), saved);
}
#[test]
fn malformed_sources_are_rejected_without_silently_dropping_fields() {
    let raw = bytes(&fixture(14));
    let text = String::from_utf8(raw).unwrap();
    assert!(
        Legacy::parse(
            text.replace("\"schema\":14", "\"schema\":14,\"schema\":14")
                .as_bytes()
        )
        .is_err()
    );
    let mut rows = fixture(14);
    rows[1]["entity"]["unknown"] = json!("keep me");
    assert!(Legacy::parse(&bytes(&rows)).is_err());
    let mut rows = fixture(14);
    rows.last_mut().unwrap()["link"]["parents"] = json!(["missing"]);
    assert!(Legacy::parse(&bytes(&rows)).is_err());
    let mut rows = fixture(14);
    rows[1]["dependencies"] = json!(["t-done", "t-done"]);
    assert!(Legacy::parse(&bytes(&rows)).is_err());
}
#[test]
fn generated_after_entity_condition_handles_terminal_pending_and_error() {
    use std::os::unix::fs::PermissionsExt;
    let ws = Workspace::new();
    let source = Legacy::parse(&bytes(&fixture(14))).unwrap();
    let (snapshot, _) = convert::run(&source, "digest", "seed", now(), &rules()).unwrap();
    let condition = snapshot
        .entity(&"t-wait".to_owned().try_into().unwrap())
        .unwrap()
        .current
        .condition
        .clone()
        .unwrap();
    for (line, code) in [
        ("t-work  Issue  InProgress  title", 1),
        ("t-work  Issue  Completed  title", 0),
        ("t-work  Issue  Cancelled  title", 0),
        ("unexpected output", 2),
        ("t-work  Issue  UnknownState  title", 2),
    ] {
        fs::write(
            ws.0.join("axon"),
            format!("#!/bin/sh\nprintf '%s\\n' {}\n", util::quote(line)),
        )
        .unwrap();
        fs::set_permissions(ws.0.join("axon"), fs::Permissions::from_mode(0o755)).unwrap();
        let status = Command::new("sh")
            .args(["-c", &condition])
            .env(
                "PATH",
                format!("{}:{}", ws.0.display(), std::env::var("PATH").unwrap()),
            )
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(code));
    }
}

#[test]
fn abrupt_worker() {
    let Some(path) = std::env::var_os("AXON_MIGRATION_TEST_JOB") else {
        return;
    };
    job::execute_with(&PathBuf::from(path), false, true, |_, at| {
        if at == Boundary::AfterCheckpoint {
            std::process::exit(91);
        }
        Ok(())
    })
    .unwrap();
    panic!("worker did not stop at checkpoint");
}
#[test]
fn process_exit_after_checkpoint_can_resume() {
    let ws = Workspace::new();
    let t = target(&ws, "root", Backend::Sqlite, 13);
    let job = ws.0.join("job");
    job::prepare(&config(vec![t]), &job).unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "abrupt_worker"])
        .env("AXON_MIGRATION_TEST_JOB", &job)
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(91),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    job::execute(&job, false, true).unwrap();
    job::execute(&job, true, true).unwrap();
}
#[test]
fn legacy_branch_duplicates_and_unmerged_git_index_are_rejected() {
    let ws = Workspace::new();
    let a = target(&ws, "a", Backend::File, 14);
    let b = target(&ws, "b", Backend::File, 14);
    fs::copy(&a.source, &b.source).unwrap();
    let job = ws.0.join("job");
    assert!(
        job::prepare(&config(vec![a.clone(), b]), &job)
            .unwrap_err()
            .to_string()
            .contains("duplicate legacy store")
    );
    assert!(!job.join("checked.json").exists());
    assert!(!job.join("b/candidate").exists());
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .current_dir(&a.root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    };
    git(&["init", "-q"]);
    let oid = git(&["hash-object", "-w", ".axon/state.jsonl"]);
    use std::io::Write;
    let mut child = Command::new("git")
        .current_dir(&a.root)
        .args(["update-index", "--index-info"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(
            format!(
                "100644 {} 1\t.axon/state.jsonl\n100644 {} 2\t.axon/state.jsonl\n",
                oid.trim(),
                oid.trim()
            )
            .as_bytes(),
        )
        .unwrap();
    assert!(child.wait().unwrap().success());
    assert!(
        job::prepare(&config(vec![a]), &ws.0.join("job2"))
            .unwrap_err()
            .to_string()
            .contains("unmerged")
    );
}
#[test]
fn incomplete_prepare_reuses_fixed_source_and_rejects_source_tampering() {
    let ws = Workspace::new();
    let t = target(&ws, "root", Backend::File, 14);
    let cfg = config(vec![t]);
    let job = ws.0.join("job");
    job::prepare(&cfg, &job).unwrap();
    let candidate = fs::read(job.join("root/candidate")).unwrap();
    let plan = fs::read(job.join("plan.json")).unwrap();
    fs::remove_file(job.join("checked.json")).unwrap();
    fs::remove_file(job.join("root/prepared.json")).unwrap();
    job::prepare(&cfg, &job).unwrap();
    assert_eq!(fs::read(job.join("root/candidate")).unwrap(), candidate);
    assert_eq!(fs::read(job.join("plan.json")).unwrap(), plan);
    fs::remove_file(job.join("checked.json")).unwrap();
    fs::remove_file(job.join("root/prepared.json")).unwrap();
    let source = fs::read(job.join("root/source")).unwrap();
    fs::write(job.join("root/source"), [source.as_slice(), b"\n"].concat()).unwrap();
    assert!(
        job::prepare(&cfg, &job)
            .unwrap_err()
            .to_string()
            .contains("fixed source")
    );
}

#[test]
fn restoring_an_unpublished_apply_closes_the_job() {
    let ws = Workspace::new();
    let t = target(&ws, "root", Backend::File, 14);
    let job = ws.0.join("job");
    job::prepare(&config(vec![t]), &job).unwrap();
    assert!(
        job::execute_with(&job, false, true, |_, at| if at == Boundary::AfterIntent {
            Err("interrupted".into())
        } else {
            Ok(())
        })
        .is_err()
    );
    job::execute(&job, true, true).unwrap();
    assert!(
        job::execute(&job, false, true)
            .unwrap_err()
            .to_string()
            .contains("entered restore")
    );
}
#[test]
fn sqlite_restore_preserves_new_notes_and_unknown_schema_is_rejected() {
    let ws = Workspace::new();
    let t = target(&ws, "root", Backend::Sqlite, 14);
    let cfg = config(vec![t.clone()]);
    let job = ws.0.join("job");
    job::prepare(&cfg, &job).unwrap();
    job::execute(&job, false, true).unwrap();
    let mut store = sqlite::Store::open(&t.source).unwrap();
    store
        .update(|_, s| {
            s.create(
                EntityId::generate("t"),
                Kind::Issue,
                Current {
                    title: "new SQLite record".into(),
                    description: String::new(),
                    lifecycle: Lifecycle::Undecided,
                    condition: None,
                    parent: None,
                    dependencies: Default::default(),
                },
                Context {
                    at: now(),
                    recorder: None,
                },
            )?;
            Ok(())
        })
        .unwrap();
    let saved = store.read().unwrap();
    drop(store);
    assert!(job::execute(&job, true, true).is_err());
    assert_eq!(
        sqlite::Store::open(&t.source).unwrap().read().unwrap(),
        saved
    );
    let unknown = ws.0.join("unknown.db");
    let db = sql_fixture(&unknown, 14);
    db.execute_batch("CREATE TABLE extension(value TEXT)")
        .unwrap();
    drop(db);
    assert!(sql::dump(&unknown).is_err());
}

#[test]
fn historical_merges_decisions_and_progress_are_archived_with_every_branch() {
    let mut rows = fixture(13);
    let base = rows
        .iter()
        .find(|r| r["type"] == "Baseline" && r["link"]["owner"] == "t-work")
        .unwrap()
        .clone();
    let mut branch = base.clone();
    branch["id"] = json!(rid("baseline-other"));
    rows.push(branch.clone());
    let merge = rid("merge-work");
    rows.push(json!({"type":"Merge","id":merge,"value":{"inputs":[
        {"identity":"a".repeat(64),"heads":[base["id"]],"candidate":base["value"]["bundle"]},
        {"identity":"b".repeat(64),"heads":[branch["id"]],"candidate":branch["value"]["bundle"]}
    ],"selected":"b".repeat(64),"result":base["value"]["bundle"]},"link":{"owner":"t-work","stream":"State","parents":[base["id"],branch["id"]],"origin":"Operation","result":base["link"]["result"]}}));
    rows.iter_mut()
        .find(|r| r["type"] == "Entity" && r["entity"]["id"] == "t-work")
        .unwrap()["lineage"]["head"] = json!(merge);
    rows.push(json!({"type":"Decision","value":{"id":rid("decision-old"),"field":"resurface_condition","old_value":"date:2026-09-12","new_value":null,"revision":null,"actor":"original","reason":"Original decision","at":now()},"link":{"owner":"t-work","stream":"State","parents":[],"origin":{"Migration":{"schema":12}},"result":null}}));
    rows.push(json!({"type":"Progress","value":{"id":rid("progress-old"),"kind":"Release","actor":"original","reason":"Original release","at":now()},"link":{"owner":"t-work","stream":"State","parents":[],"origin":{"Migration":{"schema":12}},"result":null}}));
    let source = Legacy::parse(&bytes(&rows)).unwrap();
    let (snapshot, report) = convert::run(&source, "digest", "seed", now(), &rules()).unwrap();
    let notes = snapshot
        .notes(&"t-work".to_owned().try_into().unwrap())
        .unwrap();
    let archive = notes
        .iter()
        .find(|n| n.id.to_string() == report.entities["t-work"].archive)
        .unwrap();
    for row in &source.owned["t-work"] {
        assert!(archive.body.contains(row));
    }
    assert!(archive.body.contains("date:2026-09-12"));
    rows.iter_mut().find(|r| r["type"] == "Merge").unwrap()["value"]["inputs"][0]["heads"] =
        json!([rid("baseline-missing")]);
    assert!(Legacy::parse(&bytes(&rows)).is_err());
}
