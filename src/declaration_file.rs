//! Declaration publication shares the file backend's atomic replacement boundary.
use crate::sqlite::Result;
use std::path::Path;

pub fn rewrite(path: &Path, before: &[u8], bytes: &[u8]) -> Result<()> {
    let path = std::path::absolute(path)?;
    crate::file::publish(&path, Some(before), bytes, || Ok(()))
}

/// Read and validate under the storage lock, then rewrite from the committed snapshot.
pub fn apply(
    store: &mut crate::location::Store,
    path: &Path,
    context: crate::lifecycle::Context,
) -> Result<bool> {
    apply_with(store, path, context, rewrite)
}

fn apply_with(
    store: &mut crate::location::Store,
    path: &Path,
    context: crate::lifecycle::Context,
    publish: impl FnOnce(&Path, &[u8], &[u8]) -> Result<()>,
) -> Result<bool> {
    let (before, rewritten, changed) = store
        .update(|_, snapshot| {
            let before = crate::file::read_regular(path)?;
            let input = std::str::from_utf8(&before)
                .map_err(|e| crate::sqlite::invalid(format!("Declaration schema: {e}")))?;
            let mut declaration = crate::declaration::parse(input)
                .map_err(|e| crate::sqlite::invalid(e.to_string()))?;
            let checked = declaration
                .check(input, snapshot, context)
                .map_err(|e| crate::sqlite::invalid(e.to_string()))?;
            let changed = *snapshot != checked.snapshot;
            declaration
                .refresh_applied(&checked.snapshot)
                .map_err(|e| crate::sqlite::invalid(e.to_string()))?;
            let rewritten = declaration
                .serialize(&checked.snapshot)
                .map_err(|e| crate::sqlite::invalid(e.to_string()))?;
            *snapshot = checked.snapshot;
            Ok((before, rewritten, changed))
        })
        .map_err(|error| {
            if matches!(&error, crate::sqlite::Error::Commit(_))
                || error
                    .to_string()
                    .contains("result unknown after publication")
            {
                crate::sqlite::invalid(format!(
                    "Result unknown: storage save: {error}; declaration unchanged"
                ))
            } else {
                crate::sqlite::invalid(format!(
                    "Not applied: storage unchanged; declaration unchanged: {error}"
                ))
            }
        })?;
    publish(path, &before, rewritten.as_bytes()).map_err(|error| {
        let boundary = if error.to_string().contains("result unknown after publication") {
            "Result unknown: declaration publication"
        } else {
            "Not applied: declaration unchanged"
        };
        crate::sqlite::invalid(format!("Applied: storage applied; {boundary}: {error}; retry the same file with axon import apply"))
    })?;
    Ok(changed)
}

#[cfg(test)]
mod tests {
    #[test]
    fn apply_rewrite_failures_preserve_storage_and_allow_retry() {
        for file_backend in [false, true] {
            for failure in ["write", "drift", "sync"] {
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
                let error =
                    super::apply_with(&mut store, &path, context(), |path, before, bytes| {
                        match failure {
                            "write" => {
                                Err(crate::sqlite::invalid("injected temporary write failure"))
                            }
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
                                || Err(crate::sqlite::invalid("injected sync failure")),
                            ),
                        }
                    })
                    .unwrap_err()
                    .to_string();
                assert!(error.contains("Applied: storage applied"), "{error}");
                let applied = store.read().unwrap().1;
                assert_eq!(applied.entities().count(), 3);
                if failure == "sync" {
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
                assert!(!super::apply(&mut store, &path, context()).unwrap());
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
        .unwrap_err()
        .to_string();
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
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("result unknown after publication"),
            "{error}"
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"prepared");
        std::fs::remove_dir_all(directory).unwrap();
    }
}
