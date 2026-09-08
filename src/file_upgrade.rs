//! File schema updates run only on ordinary store opening, under the writer lock.
use crate::{
    codec,
    db::{DbError, Result},
    derived::Evaluation,
    storage,
};
use std::{fs, path::Path, rc::Rc};

pub(crate) struct Step {
    pub from: (u32, u32),
    pub to: (u32, u32),
    pub apply: fn(&[u8]) -> Result<Vec<u8>>,
}
const STEPS: &[Step] = &[Step {
    from: (1, 13),
    to: (1, 14),
    apply: codec::upgrade_v13,
}];

pub(crate) fn open(
    path: &Path,
    root: &Path,
    is_git: bool,
    evaluation: Rc<Evaluation>,
) -> Result<()> {
    run(
        path,
        root,
        is_git,
        (codec::FORMAT, codec::SCHEMA),
        STEPS,
        |bytes| {
            codec::decode(bytes, evaluation.clone())?;
            Ok(())
        },
        |_| Ok(()),
    )
}
fn version(bytes: &[u8]) -> Result<(u32, u32)> {
    let value = bytes
        .split(|b| *b == b'\n')
        .filter(|l| !l.iter().all(u8::is_ascii_whitespace))
        .map(serde_json::from_slice::<serde_json::Value>)
        .find_map(|v| match v {
            Ok(v) if v["type"] == "Header" => Some(Ok(v)),
            Err(e) => Some(Err(e)),
            _ => None,
        })
        .ok_or_else(|| DbError::Storage("missing snapshot Header".into()))?
        .map_err(|e| DbError::Storage(e.to_string()))?;
    let number = |key| {
        value[key]
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| DbError::Storage("invalid snapshot version".into()))
    };
    if value["type"] != "Header" {
        return Err(DbError::Storage("snapshot must start with Header".into()));
    }
    Ok((number("format")?, number("schema")?))
}
fn run(
    path: &Path,
    root: &Path,
    is_git: bool,
    target: (u32, u32),
    steps: &[Step],
    validate: impl Fn(&[u8]) -> Result<()>,
    mut checkpoint: impl FnMut(&str) -> Result<()>,
) -> Result<()> {
    let initial = fs::read(path)?;
    if version(&initial)? == target {
        return validate(&initial);
    }
    let _lock = storage::lock(&root.join(".axon/write.lock"))?;
    storage::check_index(root, is_git)?;
    if storage::discover(root, is_git)?.0 != storage::Backend::File {
        return Err(DbError::Storage(
            "backend changed before schema update".into(),
        ));
    }
    let before = fs::read(path)?;
    let from = version(&before)?;
    if from == target {
        return validate(&before);
    }
    let mut bytes = before.clone();
    let mut current = from;
    let mut seen = std::collections::BTreeSet::new();
    while current != target {
        if !seen.insert(current) {
            return Err(DbError::Storage("cyclic file schema update path".into()));
        }
        let step = steps.iter().find(|s| s.from == current).ok_or_else(|| {
            DbError::Storage(format!(
                "unsupported snapshot format/schema {current:?}; current {target:?}"
            ))
        })?;
        bytes = (step.apply)(&bytes)?;
        if version(&bytes)? != step.to {
            return Err(DbError::Storage(
                "file schema update produced wrong version".into(),
            ));
        }
        current = step.to;
    }
    validate(&bytes)?;
    let directory = path.parent().unwrap().join("migration-backups");
    fs::create_dir_all(&directory)?;
    let backup = directory.join(format!(
        "file-{}-{}-{:032x}.jsonl",
        from.0,
        from.1,
        rand::random::<u128>()
    ));
    let mut replaced = false;
    let result = (|| -> Result<()> {
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&backup)?;
        file.write_all(&before)?;
        file.sync_all()?;
        storage::sync_dir(&directory)?;
        storage::sync_dir(directory.parent().unwrap())?;
        checkpoint("backed-up")?;
        let temp = storage::temporary(path, &bytes)?;
        checkpoint("before-replace")?;
        storage::check_index(root, is_git)?;
        if fs::read(path)? != before || storage::discover(root, is_git)?.0 != storage::Backend::File
        {
            return Err(DbError::Storage(
                "input changed before schema update".into(),
            ));
        }
        fs::rename(&temp, path)?;
        replaced = true;
        checkpoint("after-replace")?;
        storage::sync_dir(path.parent().unwrap())?;
        Ok(())
    })();
    result.map_err(|e| {
        DbError::Storage(format!(
            "{} schema update: {e}\n{}; backup: {}",
            path.display(),
            if replaced {
                "Result unknown"
            } else {
                "Not applied"
            },
            backup.display()
        ))
    })?;
    eprintln!(
        "Schema updated: {} {:?} -> {:?}; backup: {}",
        path.display(),
        from,
        target,
        backup.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (std::path::PathBuf, std::path::PathBuf) {
        let root =
            std::env::temp_dir().join(format!("axon-file-upgrade-{:032x}", rand::random::<u128>()));
        fs::create_dir_all(root.join(".axon")).unwrap();
        let path = root.join(".axon/state.jsonl");
        fs::write(
            &path,
            b"{\"type\":\"Header\",\"format\":1,\"schema\":1}\nkept\n",
        )
        .unwrap();
        (root, path)
    }
    fn steps() -> [Step; 1] {
        [Step {
            from: (1, 1),
            to: (1, 2),
            apply: |bytes| {
                Ok(String::from_utf8(bytes.to_vec())
                    .unwrap()
                    .replace("\"schema\":1", "\"schema\":2")
                    .into_bytes())
            },
        }]
    }
    #[test]
    fn preserves_backup_noop_and_publication_failure_boundaries() {
        for point in ["", "backed-up", "before-replace", "after-replace"] {
            let (root, path) = fixture();
            let before = fs::read(&path).unwrap();
            let result = run(
                &path,
                &root,
                false,
                (1, 2),
                &steps(),
                |bytes| {
                    assert!(bytes.ends_with(b"kept\n"));
                    Ok(())
                },
                |p| {
                    if p == point {
                        Err(DbError::Storage("fault".into()))
                    } else {
                        Ok(())
                    }
                },
            );
            let after = fs::read(&path).unwrap();
            let backups: Vec<_> = fs::read_dir(root.join(".axon/migration-backups"))
                .unwrap()
                .map(|p| p.unwrap().path())
                .collect();
            assert_eq!(fs::read(&backups[0]).unwrap(), before);
            if point.is_empty() || point == "after-replace" {
                assert_eq!(version(&after).unwrap(), (1, 2));
                run(
                    &path,
                    &root,
                    false,
                    (1, 2),
                    &steps(),
                    |_| Ok(()),
                    |_| panic!("no update"),
                )
                .unwrap();
                assert_eq!(
                    fs::read_dir(root.join(".axon/migration-backups"))
                        .unwrap()
                        .count(),
                    1
                );
            } else {
                assert_eq!(after, before);
            }
            if point.is_empty() {
                result.unwrap();
            } else {
                assert!(
                    result
                        .unwrap_err()
                        .to_string()
                        .contains(if point == "after-replace" {
                            "Result unknown"
                        } else {
                            "Not applied"
                        })
                );
            }
            fs::remove_dir_all(root).unwrap();
        }
    }
    #[test]
    fn concurrent_updates_publish_once() {
        let (root, path) = fixture();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let threads: Vec<_> = (0..2)
            .map(|_| {
                let root = root.clone();
                let path = path.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    run(
                        &path,
                        &root,
                        false,
                        (1, 2),
                        &steps(),
                        |_| Ok(()),
                        |_| Ok(()),
                    )
                    .unwrap();
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        assert_eq!(version(&fs::read(&path).unwrap()).unwrap(), (1, 2));
        assert_eq!(
            fs::read_dir(root.join(".axon/migration-backups"))
                .unwrap()
                .count(),
            1
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    #[ignore = "child process invoked by crash recovery test"]
    fn crash_child() {
        let root =
            std::path::PathBuf::from(std::env::var_os("AXON_FILE_UPGRADE_TEST_ROOT").unwrap());
        run(
            &root.join(".axon/state.jsonl"),
            &root,
            false,
            (1, 2),
            &steps(),
            |_| Ok(()),
            |point| {
                if point == "before-replace" {
                    std::process::exit(72);
                }
                Ok(())
            },
        )
        .unwrap();
    }
    #[test]
    fn process_exit_preserves_original_and_releases_lock() {
        let (root, path) = fixture();
        let before = fs::read(&path).unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "file_upgrade::tests::crash_child", "--ignored"])
            .env("AXON_FILE_UPGRADE_TEST_ROOT", &root)
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(72));
        assert_eq!(fs::read(&path).unwrap(), before);
        run(
            &path,
            &root,
            false,
            (1, 2),
            &steps(),
            |_| Ok(()),
            |_| Ok(()),
        )
        .unwrap();
        assert_eq!(version(&fs::read(&path).unwrap()).unwrap(), (1, 2));
        fs::remove_dir_all(root).unwrap();
    }
}
