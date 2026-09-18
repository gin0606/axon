//! Worktree-local snapshots, serialized under a stable OS lock.
use crate::{
    error::{Error, Result, invalid, validate_prefix},
    lifecycle::{self, Snapshot},
    location::Location,
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    format: String,
    prefix: String,
}
pub fn encode(prefix: &str, snapshot: &Snapshot) -> Result<Vec<u8>> {
    validate_prefix(prefix)?;
    let mut bytes = serde_json::to_vec(&Header {
        format: "axon-file/v1".into(),
        prefix: prefix.into(),
    })
    .map_err(|e| invalid(e.to_string()))?;
    bytes.push(b'\n');
    bytes.extend(lifecycle::encode(snapshot)?);
    Ok(bytes)
}
pub fn decode(bytes: &[u8]) -> Result<(String, Snapshot)> {
    let split = bytes
        .iter()
        .position(|b| *b == b'\n')
        .ok_or_else(|| invalid("missing file header"))?;
    let header: Header = serde_json::from_slice(&bytes[..split])
        .map_err(|e| invalid(format!("invalid file header: {e}")))?;
    if header.format != "axon-file/v1" {
        return Err(invalid(
            "unsupported file format; storage was not converted or modified",
        ));
    }
    validate_prefix(&header.prefix)?;
    Ok((header.prefix, lifecycle::decode(&bytes[split + 1..])?))
}
pub(crate) fn read_regular(path: &Path) -> Result<Vec<u8>> {
    if !fs::symlink_metadata(path)?.file_type().is_file() {
        return Err(invalid(format!("{} is not a regular file", path.display())));
    }
    Ok(fs::read(path)?)
}
pub(crate) fn optional_bytes(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::symlink_metadata(path) {
        Ok(_) => read_regular(path).map(Some),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
pub(crate) fn lock(path: &Path) -> Result<File> {
    if let Ok(meta) = fs::symlink_metadata(path)
        && !meta.file_type().is_file()
    {
        return Err(invalid("lock is not a regular file"));
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    file.lock()?;
    Ok(file)
}
pub(crate) fn temporary(path: &Path, bytes: &[u8]) -> Result<PathBuf> {
    let temp = path
        .parent()
        .ok_or_else(|| invalid("missing parent directory"))?
        .join(format!(".axon-{:032x}.tmp", rand::random::<u128>()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| {
            invalid(format!(
                "temporary write failed: {e}; retained {}",
                temp.display()
            ))
        })?;
    Ok(temp)
}
pub(crate) fn publish(
    path: &Path,
    before: Option<&[u8]>,
    bytes: &[u8],
    recheck: impl FnOnce() -> Result<()>,
) -> Result<()> {
    publish_with(path, before, bytes, recheck, || {
        File::open(path.parent().unwrap())?
            .sync_all()
            .map_err(Into::into)
    })
}
pub(crate) fn publish_with(
    path: &Path,
    before: Option<&[u8]>,
    bytes: &[u8],
    recheck: impl FnOnce() -> Result<()>,
    sync: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let temp = temporary(path, bytes).map_err(|e| {
        invalid(format!(
            "not applied: temporary write at {}: {e}",
            path.display()
        ))
    })?;
    let result = (|| -> Result<()> {
        recheck()?;
        if optional_bytes(path)?.as_deref() != before {
            return Err(invalid("destination changed"));
        }
        if before.is_some() {
            fs::rename(&temp, path)?;
        } else {
            fs::hard_link(&temp, path)?;
        }
        Ok(())
    })();
    result.map_err(|e| {
        invalid(format!(
            "not applied at {}: {e}; retained {}",
            path.display(),
            temp.display()
        ))
    })?;
    sync().map_err(|e| {
        Error::PublicationUnknown(format!(
            "result unknown after publication at {}: {e}; inspect state before retrying",
            path.display()
        ))
    })?;
    if before.is_none() {
        fs::remove_file(&temp).map_err(|e| {
            Error::PublicationUnknown(format!("result unknown after publication: {e}"))
        })?;
    }
    Ok(())
}
pub struct Store {
    location: Location,
}
impl Store {
    /// Discovery runs once per invocation; every guard reuses this location.
    pub(crate) fn at(location: Location) -> Result<Self> {
        let store = Self { location };
        store.backend()?;
        Ok(store)
    }
    fn root(&self) -> &Path {
        &self.location.root
    }
    fn backend(&self) -> Result<()> {
        if !fs::symlink_metadata(self.root().join(".axon"))?
            .file_type()
            .is_dir()
        {
            return Err(invalid(
                "file management directory is not a regular directory",
            ));
        }
        if !self.location.is_file()? {
            return Err(invalid("file backend changed"));
        }
        Ok(())
    }
    fn guard(&self) -> Result<()> {
        self.backend()?;
        self.location.check_index()
    }
    pub fn read(&self) -> Result<(String, Snapshot)> {
        self.guard()?;
        decode(&read_regular(&self.root().join(".axon/state.jsonl"))?)
    }
    pub fn update<T>(
        &mut self,
        change: impl FnOnce(&str, &mut Snapshot) -> Result<T>,
    ) -> Result<T> {
        self.update_with(change, |_| Ok(()))
    }
    pub(crate) fn update_with<T>(
        &mut self,
        change: impl FnOnce(&str, &mut Snapshot) -> Result<T>,
        before_publish: impl FnOnce(&crate::lifecycle::Snapshot) -> Result<()>,
    ) -> Result<T> {
        let _lock = lock(&self.root().join(".axon/state.lock"))?;
        self.guard()?;
        let path = self.root().join(".axon/state.jsonl");
        let bytes = read_regular(&path)?;
        let (prefix, mut snapshot) = decode(&bytes)?;
        let original = snapshot.clone();
        let result = change(&prefix, &mut snapshot)?;
        if snapshot.store() != original.store() {
            return Err(invalid("store identity cannot change"));
        }
        let next = encode(&prefix, &snapshot)?;
        if original == snapshot {
            return Ok(result);
        }
        publish(&path, Some(&bytes), &next, || {
            self.guard()?;
            before_publish(&snapshot)
        })?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn publication_classifies_failure_before_and_after_replace() {
        let root =
            std::env::temp_dir().join(format!("axon-publish-{:032x}", rand::random::<u128>()));
        fs::create_dir(&root).unwrap();
        let path = root.join("state");
        fs::write(&path, b"old").unwrap();
        let error = publish_with(
            &path,
            Some(b"old"),
            b"new",
            || Err(invalid("injected before rename")),
            || Ok(()),
        )
        .unwrap_err();
        assert!(matches!(&error, Error::Invalid(_)));
        assert!(error.to_string().contains("not applied"));
        assert_eq!(fs::read(&path).unwrap(), b"old");
        let error = publish_with(
            &path,
            Some(b"old"),
            b"new",
            || Ok(()),
            || Err(invalid("injected directory sync")),
        )
        .unwrap_err();
        assert!(matches!(&error, Error::PublicationUnknown(_)));
        assert!(error.to_string().contains("result unknown"));
        assert_eq!(fs::read(&path).unwrap(), b"new");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn publication_cleanup_failure_is_unknown() {
        let root =
            std::env::temp_dir().join(format!("axon-publish-{:032x}", rand::random::<u128>()));
        fs::create_dir(&root).unwrap();
        let path = root.join("state");
        let error = publish_with(
            &path,
            None,
            b"new",
            || Ok(()),
            || {
                for entry in fs::read_dir(&root)? {
                    let entry = entry?;
                    if entry.path().extension().is_some_and(|ext| ext == "tmp") {
                        fs::remove_file(entry.path())?;
                    }
                }
                Ok(())
            },
        )
        .unwrap_err();
        assert!(matches!(&error, Error::PublicationUnknown(_)));
        assert!(
            error
                .to_string()
                .starts_with("result unknown after publication:")
        );
        assert_eq!(fs::read(&path).unwrap(), b"new");
        fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod process_tests {
    use super::*;
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    #[test]
    fn lock_holder() {
        let Some(root) = std::env::var_os("AXON_FILE_TEST_LOCK_HOLDER") else {
            return;
        };
        let root = PathBuf::from(root);
        let _lock = lock(&root.join("state.lock")).unwrap();
        fs::write(root.join("ready"), b"ready").unwrap();
        loop {
            std::thread::sleep(Duration::from_secs(1));
        }
    }
    #[test]
    fn killed_process_releases_stable_lock() {
        let root = std::env::temp_dir().join(format!("axon-lock-{:032x}", rand::random::<u128>()));
        fs::create_dir(&root).unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "file::process_tests::lock_holder", "--nocapture"])
            .env("AXON_FILE_TEST_LOCK_HOLDER", &root)
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !root.join("ready").exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        if !root.join("ready").exists() {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("lock holder did not start");
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        let path = root.join("state.lock");
        let thread = std::thread::spawn(move || {
            let _lock = lock(&path).unwrap();
            sender.send(()).unwrap();
        });
        assert!(receiver.recv_timeout(Duration::from_millis(100)).is_err());
        child.kill().unwrap();
        child.wait().unwrap();
        receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        thread.join().unwrap();
        assert!(root.join("state.lock").is_file());
        fs::remove_dir_all(root).unwrap();
    }
}
