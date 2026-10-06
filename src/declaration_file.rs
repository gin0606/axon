//! Declaration publication shares the store's atomic replacement boundary.
use crate::error::{Error, Result, invalid};
use crate::file::Progress;
use crate::lifecycle::record::Entry;
use std::path::Path;

pub fn rewrite(path: &Path, before: &[u8], bytes: &[u8]) -> Result<()> {
    let path = std::path::absolute(path)?;
    crate::file::publish(&path, before, bytes, || Ok(()))
}

#[derive(Debug)]
pub struct ApplyOutcome {
    pub changed: bool,
    pub new_ids: Vec<(String, String)>,
}

/// Read and validate under the storage lock, publish the records, then rewrite the file from
/// the record set they leave behind.
pub fn apply(
    store: &mut crate::file::Store,
    path: &Path,
    context: crate::lifecycle::Context,
) -> Result<ApplyOutcome> {
    apply_with(store, path, context, &mut |_| Ok(()), rewrite)
}

fn apply_with(
    store: &mut crate::file::Store,
    path: &Path,
    context: crate::lifecycle::Context,
    progress: &mut dyn FnMut(Progress) -> Result<()>,
    publish: impl FnOnce(&Path, &[u8], &[u8]) -> Result<()>,
) -> Result<ApplyOutcome> {
    let (before, rewritten, outcome) = store
        .update_with(
            |_, records, _| {
                let before = crate::file::read_regular(path)?;
                let input = std::str::from_utf8(&before)
                    .map_err(|e| invalid(format!("Declaration schema: {e}")))?;
                let mut declaration = crate::declaration::parse_unvalidated(input)
                    .map_err(|e| invalid(e.to_string()))?;
                let checked = declaration
                    .check(input, records, context)
                    .map_err(|e| invalid(e.to_string()))?;
                let changed = !checked.records.is_empty();
                let new_ids = declaration.assigned_new_ids();
                declaration
                    .refresh_applied(&checked.after_view)
                    .map_err(|e| invalid(e.to_string()))?;
                let rewritten = declaration
                    .serialize(&checked.after_view)
                    .map_err(|e| invalid(e.to_string()))?;
                let entries = checked.records.into_iter().map(Entry::Record).collect();
                Ok((
                    entries,
                    (before, rewritten, ApplyOutcome { changed, new_ids }),
                ))
            },
            progress,
        )
        .map_err(|error| {
            if matches!(&error, Error::PublicationUnknown(_)) {
                invalid(format!(
                    "Result unknown: storage save: {error}; declaration unchanged; retry the same file with axon import apply"
                ))
            } else {
                invalid(format!(
                    "Not applied: storage unchanged; declaration unchanged: {error}"
                ))
            }
        })?;
    publish(path, &before, rewritten.as_bytes()).map_err(|error| {
        let boundary = if matches!(&error, Error::PublicationUnknown(_)) {
            "Result unknown: declaration publication"
        } else {
            "Not applied: declaration unchanged"
        };
        let storage = if outcome.changed {
            "storage applied"
        } else {
            "storage unchanged (no-op)"
        };
        invalid(format!(
            "Applied: {storage}; {boundary}: {error}; retry the same file with axon import apply"
        ))
    })?;
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::invalid;
    use crate::lifecycle::record::Store;

    fn records(store: &crate::file::Store) -> Store {
        store.read().unwrap().1
    }

    #[test]
    fn apply_rewrite_failures_preserve_storage_and_allow_retry() {
        for failure in ["write", "drift", "sync", "typed", "misleading"] {
            let root =
                std::env::temp_dir().join(format!("axon-apply-{:032x}", rand::random::<u128>()));
            std::fs::create_dir(&root).unwrap();
            let location = crate::location::Location::discover(&root, true).unwrap();
            location.init("demo").unwrap();
            let mut store = location.open().unwrap();
            let before = records(&store);
            let mut declaration = crate::declaration::example();
            declaration.prepare(&before, "demo").unwrap();
            let input = declaration.serialize(&before.view().unwrap()).unwrap();
            let path = root.join("plan.yaml");
            std::fs::write(&path, &input).unwrap();
            let context = || crate::lifecycle::Context {
                at: chrono::Utc::now(),
                recorder: None,
            };
            let error = super::apply_with(
                &mut store,
                &path,
                context(),
                &mut |_| Ok(()),
                |path, before, bytes| {
                    let result = match failure {
                        "typed" => Err(super::Error::PublicationUnknown(
                            "changed diagnostic".into(),
                        )),
                        "misleading" => Err(invalid("result unknown after publication")),
                        "write" => Err(invalid("injected temporary write failure")),
                        "drift" => crate::file::publish_with(
                            path,
                            before,
                            bytes,
                            || {
                                std::fs::write(path, b"editor content")?;
                                Ok(())
                            },
                            || Ok(()),
                        ),
                        _ => crate::file::publish_with(
                            path,
                            before,
                            bytes,
                            || Ok(()),
                            || Err(invalid("injected sync failure")),
                        ),
                    };
                    assert_eq!(
                        matches!(&result, Err(super::Error::PublicationUnknown(_))),
                        matches!(failure, "sync" | "typed"),
                        "{failure}"
                    );
                    result
                },
            )
            .unwrap_err()
            .to_string();
            assert!(
                error.contains("Applied: storage applied"),
                "{failure}: {error}"
            );
            let applied = records(&store);
            assert_eq!(applied.view().unwrap().known().count(), 3, "{failure}");
            if matches!(failure, "sync" | "typed") {
                assert!(
                    error.contains("Result unknown: declaration publication"),
                    "{failure}: {error}"
                );
            } else {
                assert!(
                    error.contains("Not applied: declaration unchanged"),
                    "{failure}: {error}"
                );
                assert_eq!(
                    std::fs::read_to_string(&path).unwrap(),
                    if failure == "drift" {
                        "editor content"
                    } else {
                        &input
                    }
                );
            }
            if failure == "drift" {
                std::fs::write(&path, &input).unwrap();
            }
            assert!(
                !super::apply(&mut store, &path, context()).unwrap().changed,
                "{failure}"
            );
            assert_eq!(records(&store), applied, "{failure}");
            let output = std::fs::read_to_string(&path).unwrap();
            let d = crate::declaration::parse(&output).unwrap();
            assert!(
                d.records().all(|r| r.base.is_some() && r.key.is_some()),
                "{failure}"
            );
            // The rewritten file matches the store with refreshed bases: nothing to publish.
            let checked = d.check(&output, &applied, context()).unwrap();
            assert!(
                checked.records.is_empty() && !checked.already_applied,
                "{failure}"
            );
            drop(store);
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn no_op_rewrite_failure_preserves_storage_and_canonical_input() {
        let root = std::env::temp_dir().join(format!("axon-apply-{:032x}", rand::random::<u128>()));
        std::fs::create_dir(&root).unwrap();
        let location = crate::location::Location::discover(&root, true).unwrap();
        location.init("demo").unwrap();
        let mut store = location.open().unwrap();
        let before = records(&store);
        let mut declaration = crate::declaration::example();
        declaration.prepare(&before, "demo").unwrap();
        let input = declaration.serialize(&before.view().unwrap()).unwrap();
        let path = root.join("plan.yaml");
        std::fs::write(&path, &input).unwrap();
        let context = || crate::lifecycle::Context {
            at: chrono::Utc::now(),
            recorder: None,
        };
        super::apply(&mut store, &path, context()).unwrap();
        let applied = records(&store);
        // A retry that writes nothing and then fails to rewrite says so.
        let canonical = std::fs::read(&path).unwrap();
        let error = super::apply_with(&mut store, &path, context(), &mut |_| Ok(()), |_, _, _| {
            Err(invalid("injected rewrite failure"))
        })
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("Applied: storage unchanged (no-op); Not applied"),
            "{error}"
        );
        assert_eq!(records(&store), applied);
        assert_eq!(std::fs::read(&path).unwrap(), canonical);
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod process_tests {
    use super::*;
    use crate::lifecycle::record::Store;
    use std::{
        fs,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };

    fn context() -> crate::lifecycle::Context {
        crate::lifecycle::Context {
            at: chrono::DateTime::from_timestamp(1000, 0).unwrap(),
            recorder: None,
        }
    }
    fn records(location: &crate::location::Location) -> Store {
        location.open().unwrap().read().unwrap().1
    }

    #[test]
    fn apply_child() {
        let Some(root) = std::env::var_os("AXON_DECLARATION_TEST_ROOT") else {
            return;
        };
        let root = std::path::PathBuf::from(root);
        let stop = std::env::var("AXON_DECLARATION_TEST_STOP").unwrap();
        let barrier = |point: &str| {
            if point == stop {
                fs::write(root.join("ready"), point).unwrap();
                loop {
                    std::thread::sleep(Duration::from_secs(1));
                }
            }
        };
        let location = crate::location::Location::discover(&root, false).unwrap();
        let mut store = location.open().unwrap();
        apply_with(
            &mut store,
            &root.join("plan.yaml"),
            context(),
            &mut |progress| {
                match progress {
                    Progress::BeforePublish => barrier("before-storage"),
                    Progress::Renamed(0) => barrier("mid-storage"),
                    Progress::Renamed(_) => {}
                }
                Ok(())
            },
            |path, before, bytes| {
                fs::write(root.join("expected.yaml"), bytes)?;
                barrier("after-storage");
                rewrite(path, before, bytes)?;
                barrier("after-rewrite");
                Ok(())
            },
        )
        .unwrap();
        panic!("requested barrier was not reached");
    }

    #[test]
    fn killed_apply_leaves_whole_records_and_a_retry_completes_them() {
        for stop in [
            "before-storage",
            "mid-storage",
            "after-storage",
            "after-rewrite",
        ] {
            let root = std::env::temp_dir()
                .join(format!("axon-apply-kill-{:032x}", rand::random::<u128>()));
            fs::create_dir(&root).unwrap();
            let location = crate::location::Location::discover(&root, true).unwrap();
            location.init("demo").unwrap();
            let before = records(&location);
            let mut declaration = crate::declaration::example();
            declaration.prepare(&before, "demo").unwrap();
            let input = declaration.serialize(&before.view().unwrap()).unwrap();
            let path = root.join("plan.yaml");
            fs::write(&path, &input).unwrap();
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "declaration_file::process_tests::apply_child",
                    "--nocapture",
                ])
                .env("AXON_DECLARATION_TEST_ROOT", &root)
                .env("AXON_DECLARATION_TEST_STOP", stop)
                .env("LLVM_PROFILE_FILE", "/dev/null")
                .stdout(Stdio::null())
                .spawn()
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            while !root.join("ready").exists() && Instant::now() < deadline {
                if let Some(status) = child.try_wait().unwrap() {
                    panic!("apply child exited before {stop}: {status}");
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let ready = root.join("ready").exists();
            child.kill().unwrap();
            assert!(!child.wait().unwrap().success());
            assert!(ready, "apply child did not reach {stop}");
            // Whatever was published is whole records: the store reads without corruption
            // and every temporary file left behind is ignored.
            let saved = records(&location);
            let published = saved.view().unwrap().known().count();
            match stop {
                "before-storage" => assert_eq!(published, 0),
                "mid-storage" => assert_eq!(published, 1),
                _ => assert_eq!(published, 3),
            }
            let output = fs::read_to_string(&path).unwrap();
            if stop == "after-rewrite" {
                assert_eq!(
                    output,
                    fs::read_to_string(root.join("expected.yaml")).unwrap()
                );
            } else {
                assert_eq!(output, input);
            }
            let mut store = location.open().unwrap();
            let outcome = apply(&mut store, &path, context()).unwrap();
            assert_eq!(outcome.changed, published < 3);
            let applied = records(&location);
            assert_eq!(applied.view().unwrap().known().count(), 3);
            // The retry adds only the records that were missing.
            assert_eq!(applied.len(), 3);
            let output = fs::read_to_string(&path).unwrap();
            let declaration = crate::declaration::parse(&output).unwrap();
            assert!(
                declaration
                    .records()
                    .all(|r| r.base.is_some() && r.key.is_some())
            );
            let checked = declaration.check(&output, &applied, context()).unwrap();
            assert!(checked.records.is_empty() && !checked.already_applied);
            assert!(!apply(&mut store, &path, context()).unwrap().changed);
            assert_eq!(records(&location), applied);
            drop(store);
            fs::remove_dir_all(root).unwrap();
        }
    }
}

#[cfg(test)]
mod relationship_tests;
