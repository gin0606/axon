//! Schema updates for ordinary commands. Backend conversion never calls this entry point.
use super::migration::{ApplicationState, Failure, Outcome};
use super::{DbError, Result};
use rusqlite::{Connection, OpenFlags};
use std::{fs, path::Path};

pub(super) struct Step {
    pub from: i64,
    pub schema: &'static str,
    pub apply: fn(&Connection) -> Result<()>,
}

// Retired formats have no production update path. Add a step with the next schema change.
const STEPS: &[Step] = &[Step {
    from: 13,
    schema: super::FRESH_SCHEMA,
    apply: upgrade_v13,
}];

fn upgrade_v13(conn: &Connection) -> Result<()> {
    conn.execute(
        "UPDATE entities SET resurface_date=resurface_date || 'T00:00:00Z' WHERE resurface_kind='date'",
        [],
    )?;
    migrate_event_labels(conn)?;
    migrate_json_payloads(conn)?;
    Ok(())
}

fn migrate_event_labels(conn: &Connection) -> Result<()> {
    for column in ["old_value", "new_value"] {
        let sql = format!(
            "UPDATE entity_events SET {column}=substr({column},1,17) || 'T00:00:00Z)' WHERE {column} GLOB 'AtDate(????-??-??)'"
        );
        conn.execute(&sql, [])?;
    }
    Ok(())
}

fn migrate_json_payloads(conn: &Connection) -> Result<()> {
    let tables = [
        (
            "causal_links",
            "record_id",
            crate::codec::upgrade_link_v13 as fn(&mut serde_json::Value) -> crate::db::Result<bool>,
        ),
        (
            "history_baselines",
            "record_id",
            crate::codec::upgrade_baseline_v13,
        ),
        (
            "history_merges",
            "record_id",
            crate::codec::upgrade_merge_v13,
        ),
    ];
    for (table, key, upgrade) in tables {
        let sql = format!("SELECT {key},payload FROM {table}");
        let mut statement = conn.prepare(&sql)?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(statement);
        for (id, payload) in rows {
            let mut value: serde_json::Value = serde_json::from_str(&payload)
                .map_err(|error| DbError::InvalidSchema(error.to_string()))?;
            if upgrade(&mut value)? {
                let payload = serde_json::to_string(&value)
                    .map_err(|error| DbError::InvalidSchema(error.to_string()))?;
                let sql = format!("UPDATE {table} SET payload=?2 WHERE {key}=?1");
                conn.execute(&sql, rusqlite::params![id, payload])?;
            }
        }
    }
    Ok(())
}

pub(super) fn open(path: &Path) -> std::result::Result<(Connection, Outcome), Failure> {
    run(
        path,
        super::SCHEMA_VERSION,
        super::FRESH_SCHEMA,
        STEPS,
        |_| Ok(()),
    )
}

fn run(
    path: &Path,
    target: i64,
    target_schema: &str,
    steps: &[Step],
    mut checkpoint: impl FnMut(&str) -> Result<()>,
) -> std::result::Result<(Connection, Outcome), Failure> {
    let mut failure = Failure {
        kind: super::migration::Kind::SchemaUpdate,
        database: path.into(),
        stage: "checking schema",
        from: None,
        to: target,
        backup: None,
        applied: ApplicationState::NotApplied,
        source: Box::new(DbError::InvalidSchema("schema update not run".into())),
    };
    let result = (|| -> Result<_> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        super::configure(&conn)?;
        let version = |conn: &Connection| {
            conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
        };
        let validate = |conn: &Connection, found: i64| -> Result<()> {
            let ddl = if found == target {
                target_schema
            } else {
                steps
                    .iter()
                    .find(|s| s.from == found)
                    .map(|s| s.schema)
                    .ok_or(DbError::UnsupportedSchema {
                        found,
                        expected: target,
                    })?
            };
            super::migration::validate_ddl(conn, found, ddl)?;
            if found == super::SCHEMA_VERSION && ddl == super::FRESH_SCHEMA {
                super::migration::validate_record_ids(conn)?;
                super::read_state(
                    conn,
                    std::rc::Rc::new(crate::derived::Evaluation::new(
                        path.parent().unwrap_or(Path::new(".")).into(),
                    )),
                )?;
            }
            Ok(())
        };
        conn.execute_batch("BEGIN")?;
        let initial = version(&conn)?;
        failure.from = Some(initial);
        validate(&conn, initial)?;
        conn.execute_batch("COMMIT")?;
        if initial == target {
            return Ok((
                conn,
                Outcome::Current {
                    database: path.into(),
                    version: initial,
                },
            ));
        }
        // Reject unsupported gaps before taking a backup or changing any schema.
        for from in initial..target {
            if !steps.iter().any(|s| s.from == from) {
                return Err(DbError::UnsupportedSchema {
                    found: initial,
                    expected: target,
                });
            }
        }
        if initial > target {
            return Err(DbError::UnsupportedSchema {
                found: initial,
                expected: target,
            });
        }
        failure.stage = "locking schema update";
        conn.pragma_update(None, "foreign_keys", false)?;
        conn.execute_batch("BEGIN IMMEDIATE")?;
        let found = version(&conn)?;
        validate(&conn, found)?;
        if found == target {
            conn.execute_batch("COMMIT")?;
            conn.pragma_update(None, "foreign_keys", true)?;
            return Ok((
                conn,
                Outcome::Current {
                    database: path.into(),
                    version: found,
                },
            ));
        }
        if found != initial {
            return Err(DbError::InvalidSchema(
                "schema changed while waiting for update lock".into(),
            ));
        }
        checkpoint("locked")?;
        failure.stage = "backing up schema update";
        let directory = path
            .parent()
            .unwrap_or(Path::new("."))
            .join("migration-backups");
        fs::create_dir_all(&directory)?;
        let backup = directory.join(format!(
            "v{found}-to-v{target}-{:032x}.db",
            rand::random::<u128>()
        ));
        failure.backup = Some(backup.clone());
        // A second connection sees committed WAL pages while the writer lock excludes changes.
        let source = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        super::migration::copy_database(&source, &backup)?;
        fs::File::open(&directory)?.sync_all()?;
        fs::File::open(directory.parent().unwrap())?.sync_all()?;
        checkpoint("backed-up")?;
        failure.stage = "updating schema";
        for from in found..target {
            let step = steps
                .iter()
                .find(|s| s.from == from)
                .ok_or(DbError::UnsupportedSchema {
                    found: from,
                    expected: target,
                })?;
            (step.apply)(&conn)?;
            conn.pragma_update(None, "user_version", from + 1)?;
            validate(&conn, from + 1)?;
            checkpoint("step")?;
        }
        super::migration::integrity(&conn)?;
        checkpoint("before-commit")?;
        failure.stage = "committing schema update";
        failure.applied = ApplicationState::Unknown;
        conn.execute_batch("COMMIT")?;
        failure.applied = ApplicationState::Applied;
        conn.pragma_update(None, "foreign_keys", true)?;
        Ok((
            conn,
            Outcome::Migrated {
                database: path.into(),
                from: found,
                to: target,
                backup,
            },
        ))
    })();
    result.map_err(|source| {
        *failure.source = source;
        failure
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    const OLD: &str = "CREATE TABLE data(value TEXT NOT NULL);";
    const NEW: &str = "CREATE TABLE data(value TEXT NOT NULL); CREATE TABLE added(value TEXT);";
    fn fixture() -> (std::path::PathBuf, std::path::PathBuf) {
        let root =
            std::env::temp_dir().join(format!("axon-upgrade-{:032x}", rand::random::<u128>()));
        fs::create_dir(&root).unwrap();
        let path = root.join("axon.db");
        let c = Connection::open(&path).unwrap();
        c.execute_batch(OLD).unwrap();
        c.execute("INSERT INTO data VALUES ('preserved')", [])
            .unwrap();
        c.pragma_update(None, "user_version", 1).unwrap();
        (root, path)
    }
    fn steps() -> [Step; 1] {
        [Step {
            from: 1,
            schema: OLD,
            apply: |c| {
                c.execute_batch("CREATE TABLE added(value TEXT);")?;
                Ok(())
            },
        }]
    }
    #[test]
    fn update_preserves_backup_and_current_is_noop() {
        let (root, path) = fixture();
        let (conn, outcome) = run(&path, 2, NEW, &steps(), |_| Ok(())).unwrap();
        let Outcome::Migrated { backup, .. } = outcome else {
            panic!()
        };
        assert_eq!(
            conn.query_row("SELECT value FROM data", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "preserved"
        );
        drop(conn);
        let saved = Connection::open(backup).unwrap();
        assert_eq!(
            saved
                .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        let (_, outcome) = run(&path, 2, NEW, &steps(), |_| panic!("no update")).unwrap();
        assert!(matches!(outcome, Outcome::Current { .. }));
        assert_eq!(
            fs::read_dir(root.join("migration-backups"))
                .unwrap()
                .count(),
            1
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn failures_rollback_and_preserve_backup() {
        for point in ["locked", "backed-up", "step", "before-commit"] {
            let (root, path) = fixture();
            let err = run(&path, 2, NEW, &steps(), |p| {
                if p == point {
                    Err(DbError::InvalidSchema("fault".into()))
                } else {
                    Ok(())
                }
            })
            .unwrap_err();
            assert_eq!(err.applied, ApplicationState::NotApplied);
            let c = Connection::open(&path).unwrap();
            assert_eq!(
                c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                    .unwrap(),
                1
            );
            assert_eq!(
                c.query_row("SELECT count(*) FROM data", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                1
            );
            if point != "locked" {
                assert!(err.backup.unwrap().is_file());
            }
            drop(c);
            run(&path, 2, NEW, &steps(), |_| Ok(())).unwrap();
            fs::remove_dir_all(root).unwrap();
        }
    }
    #[test]
    fn concurrent_open_updates_once_and_backup_includes_wal() {
        let (root, path) = fixture();
        let keeper = Connection::open(&path).unwrap();
        keeper.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; INSERT INTO data VALUES ('in WAL');").unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let path = path.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    let (conn, result) = run(&path, 2, NEW, &steps(), |_| Ok(())).unwrap();
                    drop(conn);
                    result
                })
            })
            .collect();
        let outcomes: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(
            outcomes
                .iter()
                .filter(|o| matches!(o, Outcome::Migrated { .. }))
                .count(),
            1
        );
        let backup = fs::read_dir(root.join("migration-backups"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let saved = Connection::open(backup).unwrap();
        assert_eq!(
            saved
                .query_row("SELECT count(*) FROM data", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            2
        );
        drop(saved);
        drop(keeper);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    #[ignore = "child process invoked by crash recovery test"]
    fn crash_child() {
        let path = std::env::var_os("AXON_UPGRADE_TEST_PATH").unwrap();
        run(Path::new(&path), 2, NEW, &steps(), |point| {
            if point == "step" {
                std::process::exit(71);
            }
            Ok(())
        })
        .unwrap();
    }
    #[test]
    fn process_exit_rolls_back_and_releases_update_lock() {
        let (root, path) = fixture();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "db::upgrade::tests::crash_child", "--ignored"])
            .env("AXON_UPGRADE_TEST_PATH", &path)
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(71));
        let conn = Connection::open(&path).unwrap();
        assert_eq!(
            conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        drop(conn);
        run(&path, 2, NEW, &steps(), |_| Ok(())).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
}
