//! Declaration publication shares the file backend's atomic replacement boundary.
use crate::error::{Error, Result, invalid};
use std::path::Path;

pub fn rewrite(path: &Path, before: &[u8], bytes: &[u8]) -> Result<()> {
    let path = std::path::absolute(path)?;
    crate::file::publish(&path, Some(before), bytes, || Ok(()))
}

#[derive(Debug)]
pub struct ApplyOutcome {
    pub changed: bool,
    pub new_ids: Vec<(String, String)>,
}

/// Read and validate under the storage lock, then rewrite from the committed snapshot.
pub fn apply(
    store: &mut crate::location::Store,
    path: &Path,
    context: crate::lifecycle::Context,
) -> Result<ApplyOutcome> {
    apply_with(store, path, context, |_| Ok(()), rewrite)
}

fn apply_with(
    store: &mut crate::location::Store,
    path: &Path,
    context: crate::lifecycle::Context,
    before_publish: impl FnOnce(&crate::lifecycle::Snapshot) -> Result<()>,
    publish: impl FnOnce(&Path, &[u8], &[u8]) -> Result<()>,
) -> Result<ApplyOutcome> {
    let (before, rewritten, outcome) = store
        .update_with(
            |_, snapshot| {
                let before = crate::file::read_regular(path)?;
                let input = std::str::from_utf8(&before)
                    .map_err(|e| invalid(format!("Declaration schema: {e}")))?;
                let mut declaration =
                    crate::declaration::parse(input).map_err(|e| invalid(e.to_string()))?;
                let checked = declaration
                    .check(input, snapshot, context)
                    .map_err(|e| invalid(e.to_string()))?;
                let changed = *snapshot != checked.snapshot;
                let new_ids = declaration.assigned_new_ids();
                declaration
                    .refresh_applied(&checked.snapshot)
                    .map_err(|e| invalid(e.to_string()))?;
                let rewritten = declaration
                    .serialize(&checked.snapshot)
                    .map_err(|e| invalid(e.to_string()))?;
                *snapshot = checked.snapshot;
                Ok((before, rewritten, ApplyOutcome { changed, new_ids }))
            },
            before_publish,
        )
        .map_err(|error| {
            if matches!(&error, Error::Commit(_) | Error::PublicationUnknown(_)) {
                invalid(format!(
                    "Result unknown: storage save: {error}; declaration unchanged"
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
        invalid(format!("Applied: storage applied; {boundary}: {error}; retry the same file with axon import apply"))
    })?;
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::invalid;

    #[test]
    fn apply_rewrite_failures_preserve_storage_and_allow_retry() {
        for file_backend in [false, true] {
            for failure in ["write", "drift", "sync", "typed", "misleading"] {
                let root = std::env::temp_dir()
                    .join(format!("axon-apply-{:032x}", rand::random::<u128>()));
                std::fs::create_dir(&root).unwrap();
                let location = crate::location::Location::discover(&root, true).unwrap();
                location.init_backend("demo", file_backend).unwrap();
                let mut store = location.open().unwrap();
                let before = store.read().unwrap().1;
                let mut declaration = crate::declaration::example();
                declaration.prepare(&before, "demo").unwrap();
                let input = declaration.serialize(&before).unwrap();
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
                    |_| Ok(()),
                    |path, before, bytes| {
                        let result = match failure {
                            "typed" => Err(super::Error::PublicationUnknown(
                                "changed diagnostic".into(),
                            )),
                            "misleading" => Err(invalid("result unknown after publication")),
                            "write" => Err(invalid("injected temporary write failure")),
                            "drift" => crate::file::publish_with(
                                path,
                                Some(before),
                                bytes,
                                || {
                                    std::fs::write(path, b"editor content")?;
                                    Ok(())
                                },
                                || Ok(()),
                            ),
                            _ => crate::file::publish_with(
                                path,
                                Some(before),
                                bytes,
                                || Ok(()),
                                || Err(invalid("injected sync failure")),
                            ),
                        };
                        assert_eq!(
                            matches!(&result, Err(super::Error::PublicationUnknown(_))),
                            matches!(failure, "sync" | "typed")
                        );
                        result
                    },
                )
                .unwrap_err()
                .to_string();
                assert!(error.contains("Applied: storage applied"), "{error}");
                let applied = store.read().unwrap().1;
                assert_eq!(applied.entities().count(), 3);
                if matches!(failure, "sync" | "typed") {
                    assert!(
                        error.contains("Result unknown: declaration publication"),
                        "{error}"
                    );
                } else {
                    assert!(
                        error.contains("Not applied: declaration unchanged"),
                        "{error}"
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
                assert!(!super::apply(&mut store, &path, context()).unwrap().changed);
                assert_eq!(store.read().unwrap().1, applied);
                let output = std::fs::read_to_string(&path).unwrap();
                let d = crate::declaration::parse(&output).unwrap();
                assert!(d.records().all(|r| r.base.is_some() && r.key.is_some()));
                assert_eq!(
                    d.check(&output, &applied, context()).unwrap().snapshot,
                    applied
                );
                drop(store);
                std::fs::remove_dir_all(root).unwrap();
            }
        }
    }
    #[test]
    fn declaration_publication_distinguishes_drift_and_sync_failure() {
        let directory =
            std::env::temp_dir().join(format!("axon-declaration-{:032x}", rand::random::<u128>()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("declaration.yaml");
        std::fs::write(&path, b"original").unwrap();
        let error = crate::file::publish_with(
            &path,
            Some(b"original"),
            b"prepared",
            || {
                std::fs::write(&path, b"editor")?;
                Ok(())
            },
            || Ok(()),
        )
        .unwrap_err();
        assert!(matches!(&error, super::Error::Invalid(_)));
        let error = error.to_string();
        assert!(
            error.contains("not applied")
                && error.contains("destination changed")
                && error.contains("retained"),
            "{error}"
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"editor");
        let error = crate::file::publish_with(
            &path,
            Some(b"editor"),
            b"prepared",
            || Ok(()),
            || Err(std::io::Error::other("injected sync failure").into()),
        )
        .unwrap_err();
        assert!(matches!(&error, super::Error::PublicationUnknown(_)));
        let error = error.to_string();
        assert!(
            error.contains("result unknown after publication"),
            "{error}"
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"prepared");
        std::fs::remove_dir_all(directory).unwrap();
    }
}

#[cfg(test)]
mod process_tests {
    use super::*;
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
            |snapshot| {
                fs::write(
                    root.join("expected.json"),
                    crate::lifecycle::encode(snapshot)?,
                )?;
                barrier("before-storage");
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
    fn killed_apply_preserves_complete_snapshots_and_retries() {
        for file_backend in [false, true] {
            for stop in ["before-storage", "after-storage", "after-rewrite"] {
                let root = std::env::temp_dir()
                    .join(format!("axon-apply-kill-{:032x}", rand::random::<u128>()));
                fs::create_dir(&root).unwrap();
                let location = crate::location::Location::discover(&root, true).unwrap();
                location.init_backend("demo", file_backend).unwrap();
                let before = location.open().unwrap().read().unwrap().1;
                let mut declaration = crate::declaration::example();
                declaration.prepare(&before, "demo").unwrap();
                let input = declaration.serialize(&before).unwrap();
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
                let expected =
                    crate::lifecycle::decode(&fs::read(root.join("expected.json")).unwrap())
                        .unwrap();
                let mut store = location.open().unwrap();
                let saved = store.read().unwrap().1;
                assert_eq!(
                    saved,
                    if stop == "before-storage" {
                        before
                    } else {
                        expected
                    }
                );
                let output = fs::read_to_string(&path).unwrap();
                if stop == "after-rewrite" {
                    assert_eq!(
                        output,
                        fs::read_to_string(root.join("expected.yaml")).unwrap()
                    );
                } else {
                    assert_eq!(output, input);
                }
                let outcome = apply(&mut store, &path, context()).unwrap();
                assert_eq!(outcome.changed, stop == "before-storage");
                let applied = store.read().unwrap().1;
                assert_eq!(applied.entities().count(), 3);
                if stop != "before-storage" {
                    assert_eq!(applied, saved);
                }
                let output = fs::read_to_string(&path).unwrap();
                let declaration = crate::declaration::parse(&output).unwrap();
                assert!(
                    declaration
                        .records()
                        .all(|r| r.base.is_some() && r.key.is_some())
                );
                assert_eq!(
                    declaration
                        .check(&output, &applied, context())
                        .unwrap()
                        .snapshot,
                    applied
                );
                assert!(!apply(&mut store, &path, context()).unwrap().changed);
                assert_eq!(store.read().unwrap().1, applied);
                drop(store);
                fs::remove_dir_all(root).unwrap();
            }
        }
    }
}
