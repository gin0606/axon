//! The record directory: one immutable file per record, published under a stable OS lock.
use crate::{
    error::{Error, Result, invalid},
    lifecycle::record::{self, Entry, Header, RECORD_ID_LENGTH, RecordId},
    location::Location,
};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

/// The name of the header file, the mark of a store, inside `.axon/`.
pub const HEADER_FILE: &str = "header.json";
/// The record directory inside `.axon/`.
pub const RECORDS_DIRECTORY: &str = "records";
/// The lock every ordinary writer takes, inside `.axon/`.
pub const WRITE_LOCK: &str = "write.lock";
/// The suffix of a file that is being written; readers ignore such names.
pub const TEMPORARY_SUFFIX: &str = ".tmp";
/// The ignore file `axon init` writes beside the records: locks and temporary files only.
pub const GITIGNORE: &str = "*.lock\n*.tmp\n";
/// The attributes file `axon init` writes beside the records: it keeps Git from converting
/// line endings, which would change the bytes a record ID is the hash of. No merge attribute.
pub const GITATTRIBUTES: &str = "* -text\n";
/// The files `axon init` writes inside `.axon/` besides the header and the record directory,
/// by name and content. Discovery treats a file with this content, CRLF read as LF, as residue;
/// init rewrites such a file with CRLF to this content.
pub const STORE_FILES: [(&str, &str); 2] =
    [(".gitignore", GITIGNORE), (".gitattributes", GITATTRIBUTES)];
/// The one line a report adds when a record file is its record with CRLF line endings: one
/// wording for a store without the attributes file init writes, one for a store that has it.
pub const LINE_ENDING_HINT_WITHOUT_ATTRIBUTES: &str = "Hint: Git line-ending conversion is likely: .axon/.gitattributes with \"* -text\" is missing from the store. Do not stage the converted files; axon docs (Storage and recovery) gives the repair and the storage guide (docs/guide/storage.md, line-ending conversion) the details";
pub const LINE_ENDING_HINT_WITH_ATTRIBUTES: &str = "Hint: Git line-ending conversion is likely: the files were checked out before .axon/.gitattributes existed, or it lacks \"* -text\" or a setting overrides it. Do not stage the converted files; axon docs (Storage and recovery) gives the repair and the storage guide (docs/guide/storage.md, line-ending conversion) the details";

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
/// Writes bytes to `<path>.tmp` and syncs it; the caller renames it into place. Only for a
/// writer under the store's lock, for which a leftover file of that name is a stale one of
/// its own.
pub(crate) fn temporary(path: &Path, bytes: &[u8]) -> Result<PathBuf> {
    let mut name = path
        .file_name()
        .ok_or_else(|| invalid("missing file name"))?
        .to_os_string();
    name.push(TEMPORARY_SUFFIX);
    let temp = path.with_file_name(name);
    // Whatever is at that name is removed rather than opened: opening would follow a symlink
    // and truncate its target. Removing a link removes the link only.
    match fs::remove_file(&temp) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    fill(file, temp, bytes)
}
/// Writes bytes to a temporary file of its own beside `path`, for publication without a lock:
/// concurrent publications never share a temporary file.
fn temporary_unique(path: &Path, bytes: &[u8]) -> Result<PathBuf> {
    let temp = path
        .parent()
        .ok_or_else(|| invalid("missing parent directory"))?
        .join(format!(
            ".axon-{:032x}{TEMPORARY_SUFFIX}",
            rand::random::<u128>()
        ));
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    fill(file, temp, bytes)
}
fn fill(mut file: File, temp: PathBuf, bytes: &[u8]) -> Result<PathBuf> {
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
pub(crate) fn sync_directory(path: &Path) -> Result<()> {
    File::open(path)?.sync_all().map_err(Into::into)
}
/// Replaces a file whose current bytes are `before` through a temporary file and a rename,
/// then syncs the directory. Failure before the rename is `not applied`; failure after it is
/// `result unknown`.
pub(crate) fn publish(
    path: &Path,
    before: &[u8],
    bytes: &[u8],
    recheck: impl FnOnce() -> Result<()>,
) -> Result<()> {
    publish_with(path, before, bytes, recheck, || {
        sync_directory(path.parent().unwrap())
    })
}
pub(crate) fn publish_with(
    path: &Path,
    before: &[u8],
    bytes: &[u8],
    recheck: impl FnOnce() -> Result<()>,
    sync: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let temp = temporary_unique(path, bytes).map_err(|e| {
        invalid(format!(
            "not applied: temporary write at {}: {e}",
            path.display()
        ))
    })?;
    let result = (|| -> Result<()> {
        recheck()?;
        if optional_bytes(path)?.as_deref() != Some(before) {
            return Err(invalid("destination changed"));
        }
        fs::rename(&temp, path)?;
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
    })
}

/// One file under `.axon/records/` that is not a record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Corruption {
    /// Relative to `.axon/records/`.
    pub path: PathBuf,
    pub reason: String,
    /// The content is the record named by the file with CRLF line endings: what a Git
    /// line-ending conversion leaves behind.
    pub converted_line_endings: bool,
}
impl Corruption {
    /// The one line every report shows for this file: its path and the reason.
    pub fn line(&self) -> String {
        format!("{}: {}", self.path.display(), self.reason)
    }
}
/// Whether `bytes` are the record named `id` with every LF turned into CRLF.
fn is_converted_record(bytes: &[u8], id: &RecordId) -> bool {
    if !bytes.contains(&b'\r') {
        return false;
    }
    RecordId::of(&crlf_as_lf(bytes)) == *id
}
/// `bytes` with each CRLF read as LF, undoing a Git line-ending conversion. A lone CR stays.
pub(crate) fn crlf_as_lf(bytes: &[u8]) -> Vec<u8> {
    let mut restored = Vec::with_capacity(bytes.len());
    for (index, &byte) in bytes.iter().enumerate() {
        if byte == b'\r' && bytes.get(index + 1) == Some(&b'\n') {
            continue;
        }
        restored.push(byte);
    }
    restored
}
/// What one read of the store found: the header, every record, and the files that are not
/// records. A read with corruption is not usable for derivation.
#[derive(Debug)]
pub struct Loaded {
    pub header: Header,
    pub records: record::Store,
    /// The derived view, when the set is intact.
    pub view: Option<record::View>,
    pub corruption: Vec<Corruption>,
    /// Whether `.axon/.gitattributes` is known to be absent, checked only when a converted
    /// record was found: it decides the wording of the line-ending hint. A check that fails
    /// does not claim the file is missing.
    pub attributes_missing: bool,
}
impl Loaded {
    pub fn is_intact(&self) -> bool {
        self.corruption.is_empty()
    }
    /// The lines a report shows for the corrupt files, each behind `prefix`, followed by
    /// the line-ending hint when one of them is a converted record.
    pub fn corruption_lines(&self, prefix: &str) -> Vec<String> {
        let mut lines: Vec<_> = self
            .corruption
            .iter()
            .map(|c| format!("{prefix}{}", c.line()))
            .collect();
        if self.corruption.iter().any(|c| c.converted_line_endings) {
            lines.push(
                if self.attributes_missing {
                    LINE_ENDING_HINT_WITHOUT_ATTRIBUTES
                } else {
                    LINE_ENDING_HINT_WITH_ATTRIBUTES
                }
                .into(),
            );
        }
        lines
    }
    fn usable(self) -> Result<(Header, record::Store, record::View)> {
        if self.corruption.is_empty()
            && let Some(view) = self.view
        {
            return Ok((self.header, self.records, view));
        }
        Err(invalid(format!(
            "corrupt record files under .axon/records/; repair them before any operation:\n{}",
            self.corruption_lines("").join("\n")
        )))
    }
}
fn is_hex_name(name: &str, length: usize) -> bool {
    name.len() == length
        && name
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
/// Whether a file name is a temporary file's, which readers and discovery ignore.
pub(crate) fn is_temporary(name: &std::ffi::OsStr) -> bool {
    name.to_string_lossy().ends_with(TEMPORARY_SUFFIX)
}
/// The prefix of the message a corrupt header stops a read with; `axon storage check`
/// reports such a header by this mark.
pub const CORRUPT_HEADER: &str = "corrupt header ";

/// Where the store publishes: after every record file has been written to its temporary
/// name, and after each rename. Callers that stop a process at these points test what a
/// reader sees in between.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    BeforePublish,
    Renamed(usize),
}

pub struct Store {
    location: Location,
}
impl Store {
    /// Discovery runs once per invocation; every guard reuses this location.
    pub(crate) fn at(location: Location) -> Result<Self> {
        let store = Self { location };
        // As in `guard`, an unmerged index is reported before the damage; only a damaged
        // `.axon` pays for the extra look at the index.
        if let Err(damage) = store.store() {
            store.location.check_index()?;
            return Err(damage);
        }
        Ok(store)
    }
    fn root(&self) -> &Path {
        &self.location.root
    }
    fn directory(&self) -> PathBuf {
        self.root().join(".axon")
    }
    pub fn header_path(&self) -> PathBuf {
        self.directory().join(HEADER_FILE)
    }
    pub fn records_path(&self) -> PathBuf {
        self.directory().join(RECORDS_DIRECTORY)
    }
    fn store(&self) -> Result<()> {
        if !fs::symlink_metadata(self.directory())?.file_type().is_dir() {
            return Err(invalid("management directory is not a regular directory"));
        }
        // The record directory is read and written through as well, so a link there would
        // reach another store, outside its lock.
        if let Ok(meta) = fs::symlink_metadata(self.records_path())
            && !meta.file_type().is_dir()
        {
            return Err(invalid(format!(
                "{} is not a regular directory",
                self.records_path().display()
            )));
        }
        if !self.location.initialized()? {
            return Err(invalid("store disappeared"));
        }
        Ok(())
    }
    fn guard(&self) -> Result<()> {
        // An unmerged index explains a damaged `.axon` better than the damage does.
        self.location.check_index()?;
        self.store()
    }
    /// Reads the header and every record file, collecting what is not a record instead of
    /// stopping at the first; `read` requires an intact store.
    pub fn load(&self) -> Result<Loaded> {
        self.guard()?;
        let header_path = self.header_path();
        let header = read_regular(&header_path)
            .and_then(|bytes| record::decode_header(&bytes).map_err(Into::into))
            .map_err(|e| {
                invalid(format!(
                    "{CORRUPT_HEADER}{}: {e}; the store is not read until it is repaired",
                    header_path.display()
                ))
            })?;
        let mut records = record::Store::new();
        let mut corruption = Vec::new();
        let base = self.records_path();
        let mut corrupt = |path: &Path, reason: String, converted_line_endings: bool| {
            corruption.push(Corruption {
                path: path.strip_prefix(&base).unwrap_or(path).to_path_buf(),
                reason,
                converted_line_endings,
            });
        };
        let entries = match fs::read_dir(&base) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let view = Some(records.view()?);
                return Ok(Loaded {
                    header,
                    records,
                    view,
                    corruption,
                    attributes_missing: false,
                });
            }
            Err(e) => {
                return Err(invalid(format!(
                    "unreadable record directory {}: {e}; the store is not read until it is repaired",
                    base.display()
                )));
            }
        };
        let mut subdirectories = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| {
                invalid(format!(
                    "unreadable record directory {}: {e}; the store is not read until it is repaired",
                    base.display()
                ))
            })?;
            let path = entry.path();
            let name = entry.file_name();
            if is_temporary(&name) {
                continue;
            }
            match fs::symlink_metadata(&path) {
                Ok(meta) if meta.file_type().is_dir() => {
                    subdirectories.push((name.to_string_lossy().into_owned(), path));
                }
                Ok(_) => corrupt(&path, "not a record subdirectory".into(), false),
                Err(e) => corrupt(&path, format!("unreadable: {e}"), false),
            }
        }
        subdirectories.sort();
        // A file or directory that cannot be read is reported by path like any other
        // corruption, and the scan goes on to list the rest.
        for (subdirectory, path) in subdirectories {
            if !is_hex_name(&subdirectory, 2) {
                corrupt(&path, "not a record subdirectory".into(), false);
                continue;
            }
            let mut files: Vec<_> = match fs::read_dir(&path).and_then(Iterator::collect) {
                Ok(files) => files,
                Err(e) => {
                    corrupt(&path, format!("unreadable: {e}"), false);
                    continue;
                }
            };
            files.sort_by_key(|entry| entry.file_name());
            for file in files {
                let file_path = file.path();
                let name = file.file_name();
                if is_temporary(&name) {
                    continue;
                }
                let name = name.to_string_lossy().into_owned();
                let regular = fs::symlink_metadata(&file_path).map(|m| m.file_type().is_file());
                match regular {
                    Ok(true) => {}
                    Ok(false) => {
                        corrupt(&file_path, "not a regular file".into(), false);
                        continue;
                    }
                    Err(e) => {
                        corrupt(&file_path, format!("unreadable: {e}"), false);
                        continue;
                    }
                }
                if !is_hex_name(&name, RECORD_ID_LENGTH) {
                    corrupt(&file_path, "name is not a record ID".into(), false);
                    continue;
                }
                if !name.starts_with(&subdirectory) {
                    corrupt(&file_path, "subdirectory differs from the ID".into(), false);
                    continue;
                }
                let id = RecordId::try_from(name.as_str())?;
                let bytes = match fs::read(&file_path) {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        corrupt(&file_path, format!("unreadable: {e}"), false);
                        continue;
                    }
                };
                // One decode: the ID it computes is compared with the file name. A file
                // that is its record with CRLF line endings is reported as such: Git converted
                // it, and the reader does not restore it.
                match records.insert_bytes(&bytes) {
                    Ok(actual) if actual == id => {}
                    _ if is_converted_record(&bytes, &id) => corrupt(
                        &file_path,
                        "CRLF line endings; with LF the content hashes to the name".into(),
                        true,
                    ),
                    Ok(actual) => corrupt(
                        &file_path,
                        format!("content whose hash is {actual}, not the name"),
                        false,
                    ),
                    Err(e) => corrupt(&file_path, e.to_string(), false),
                }
            }
        }
        // Corruption that shows only across records (a record that does not continue its
        // parent) refuses derivation; it is then reported by file like every other.
        let view = if corruption.is_empty() {
            match records.view() {
                Ok(view) => Some(view),
                Err(error) => {
                    let problems = records.problems();
                    if problems.is_empty() {
                        return Err(invalid(format!(
                            "corrupt record set under {}: {error}; the store is not read until it is repaired",
                            base.display()
                        )));
                    }
                    for (id, error) in problems {
                        corruption.push(Corruption {
                            path: Path::new(id.subdirectory()).join(id.as_ref()),
                            reason: error.to_string(),
                            converted_line_endings: false,
                        });
                    }
                    None
                }
            }
        } else {
            None
        };
        let attributes_missing = corruption.iter().any(|c| c.converted_line_endings)
            && crate::location::present(&self.directory().join(".gitattributes")).is_ok_and(|p| !p);
        Ok(Loaded {
            header,
            records,
            view,
            corruption,
            attributes_missing,
        })
    }
    /// The header, the record set and its derived view of an intact store.
    pub fn read(&self) -> Result<(Header, record::Store, record::View)> {
        self.load()?.usable()
    }
    /// Runs `change` under the write lock against the current record set and publishes the
    /// records it returns, one file each. No record: nothing is written.
    pub fn update<T>(
        &mut self,
        change: impl FnOnce(&Header, &record::Store, &record::View) -> Result<(Vec<Entry>, T)>,
    ) -> Result<T> {
        self.update_with(change, &mut |_| Ok(()))
    }
    pub(crate) fn update_with<T>(
        &mut self,
        change: impl FnOnce(&Header, &record::Store, &record::View) -> Result<(Vec<Entry>, T)>,
        progress: &mut dyn FnMut(Progress) -> Result<()>,
    ) -> Result<T> {
        // A lock that cannot be taken is damage too, reported after an unmerged index.
        let _lock = match lock(&self.directory().join(WRITE_LOCK)) {
            Ok(lock) => lock,
            Err(damage) => {
                self.location.check_index()?;
                return Err(damage);
            }
        };
        let (header, records, view) = self.read()?;
        let (entries, result) = change(&header, &records, &view)?;
        if entries.is_empty() {
            return Ok(result);
        }
        self.publish_entries(&entries, progress)?;
        Ok(result)
    }
    /// Writes every record to its temporary file, then renames each into place in order and
    /// syncs its directory. A record whose file already exists is the same record and is
    /// skipped. Failure before the first rename is `not applied`; afterwards `result unknown`.
    /// Either way the temporary files that were not renamed are removed.
    fn publish_entries(
        &self,
        entries: &[Entry],
        progress: &mut dyn FnMut(Progress) -> Result<()>,
    ) -> Result<()> {
        let base = self.records_path();
        let mut planned = Vec::with_capacity(entries.len());
        // The record directory and `.axon/` are synced after the renames as well: a
        // subdirectory's entry (this publication's, or an earlier interrupted one's) is durable
        // only once its parent is.
        let mut created_records_directory = false;
        let prepared = (|| -> Result<()> {
            for entry in entries {
                let bytes = record::encode(entry)?;
                let id = RecordId::of(&bytes);
                let directory = base.join(id.subdirectory());
                // A directory created here is synced into its parent at once, so the entry
                // of a subdirectory is durable before any record renamed into it.
                if fs::symlink_metadata(&base).is_err() {
                    fs::create_dir(&base)?;
                    sync_directory(base.parent().unwrap())?;
                    created_records_directory = true;
                }
                // A link at the subdirectory would take the record outside the store; a read
                // rejects it as corruption, so a write does not go through it either.
                match fs::symlink_metadata(&directory) {
                    Ok(meta) if meta.file_type().is_dir() => {}
                    Ok(_) => {
                        return Err(invalid(format!(
                            "{} is not a regular directory",
                            directory.display()
                        )));
                    }
                    Err(_) => {
                        fs::create_dir(&directory)?;
                        sync_directory(&base)?;
                    }
                }
                let path = directory.join(id.as_ref());
                if fs::symlink_metadata(&path).is_ok() {
                    planned.push((path, None));
                    continue;
                }
                let temp = temporary(&path, &bytes)?;
                planned.push((path, Some(temp)));
            }
            self.guard()?;
            progress(Progress::BeforePublish)
        })();
        if let Err(e) = prepared {
            for (_, temp) in &planned {
                if let Some(temp) = temp {
                    let _ = fs::remove_file(temp);
                }
            }
            return Err(invalid(format!(
                "not applied: no record written under {}: {e}",
                base.display()
            )));
        }
        let mut renamed: Vec<&Path> = Vec::new();
        let last = planned.len() - 1;
        for (index, (path, temp)) in planned.iter().enumerate() {
            let published = (|| -> Result<()> {
                if let Some(temp) = temp {
                    fs::rename(temp, path)?;
                    renamed.push(path);
                }
                // Each rename is followed by the sync of its directory, so the publication
                // order survives a crash (a record whose parent or dependency is not yet
                // durable is not durable either). A file left by an interrupted publication
                // is synced here too.
                sync_directory(path.parent().unwrap())?;
                if index == last {
                    sync_directory(&base)?;
                    if created_records_directory {
                        sync_directory(base.parent().unwrap())?;
                    }
                }
                progress(Progress::Renamed(index))
            })();
            if let Err(e) = published {
                let published: Vec<_> = renamed.iter().map(|p| p.display().to_string()).collect();
                // A rename that failed leaves its temporary file; later ones were never tried.
                for (_, temp) in &planned[index..] {
                    if let Some(temp) = temp {
                        let _ = fs::remove_file(temp);
                    }
                }
                return Err(if renamed.is_empty() {
                    invalid(format!(
                        "not applied: no record written; rename to {} failed: {e}",
                        path.display()
                    ))
                } else {
                    Error::PublicationUnknown(format!(
                        "result unknown after publication at {}: {e}; renamed so far: [{}]; inspect state before retrying",
                        path.display(),
                        published.join(", ")
                    ))
                });
            }
        }
        Ok(())
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
            b"old",
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
            b"old",
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
    fn record_publication_is_one_file_per_record_and_a_temporary_file_is_ignored() {
        use crate::lifecycle::record::{Context, Current, Kind, Lifecycle};
        let root =
            std::env::temp_dir().join(format!("axon-records-{:032x}", rand::random::<u128>()));
        fs::create_dir(&root).unwrap();
        let location = Location::discover(&root, true).unwrap();
        location.init("demo").unwrap();
        let mut store = location.open().unwrap();
        let context = Context {
            at: chrono::Utc::now(),
            recorder: None,
        };
        let id = store
            .update(|header, records, _| {
                assert_eq!(header.prefix, "demo");
                let id = record::new_entity_id(&header.prefix)?;
                let record = records.create(
                    id.clone(),
                    Current {
                        kind: Kind::Issue,
                        lifecycle: Lifecycle::NotStarted,
                        owner: None,
                        title: "one".into(),
                        description: String::new(),
                        condition: None,
                        parent: None,
                        needs: Default::default(),
                    },
                    context.clone(),
                )?;
                Ok((vec![Entry::Record(record)], id))
            })
            .unwrap();
        let (_, records, _) = store.read().unwrap();
        assert_eq!(records.len(), 1);
        let (record_id, _) = records.entries().next().unwrap();
        let path = store
            .records_path()
            .join(record_id.subdirectory())
            .join(record_id.as_ref());
        assert!(path.is_file());
        let bytes = fs::read(&path).unwrap();
        assert_eq!(RecordId::of(&bytes), *record_id);
        // A stale temporary file beside it is ignored by reads and by the corruption listing.
        let temp = path.with_file_name(format!("{record_id}.tmp"));
        fs::write(&temp, b"partial").unwrap();
        let loaded = store.load().unwrap();
        assert!(loaded.is_intact());
        assert_eq!(loaded.records.len(), 1);
        // A file whose name is not a record ID, and one whose content does not hash to its
        // name, are corruption that stops every read.
        fs::write(store.records_path().join("stray"), b"x").unwrap();
        let forged = store
            .records_path()
            .join(record_id.subdirectory())
            .join("0".repeat(RECORD_ID_LENGTH));
        fs::write(&forged, &bytes).unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(loaded.corruption.len(), 2);
        assert!(
            loaded
                .corruption
                .iter()
                .any(|c| c.path == Path::new("stray"))
        );
        assert!(
            loaded
                .corruption
                .iter()
                .any(|c| c.reason.contains("hash") || c.reason.contains("subdirectory"))
        );
        let error = store.read().unwrap_err().to_string();
        assert!(
            error.contains("corrupt") && error.contains("stray"),
            "{error}"
        );
        assert!(
            store
                .update(|_, _, _| Ok((Vec::new(), ())))
                .unwrap_err()
                .to_string()
                .contains("corrupt")
        );
        fs::remove_file(store.records_path().join("stray")).unwrap();
        fs::remove_file(&forged).unwrap();
        // The same record published again is the same file: nothing changes.
        let (_, records, _) = store.read().unwrap();
        let entry = records.entries().next().unwrap().1.clone();
        store.update(|_, _, _| Ok((vec![entry], ()))).unwrap();
        assert_eq!(store.read().unwrap().1.len(), 1);
        assert!(store.read().unwrap().2.is_known(&id));
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod publication_tests {
    use super::*;
    use crate::lifecycle::record::{Context, Current, Kind, Lifecycle};
    use proptest::prelude::*;

    fn fixture() -> (PathBuf, Store) {
        let root =
            std::env::temp_dir().join(format!("axon-publish-{:032x}", rand::random::<u128>()));
        fs::create_dir(&root).unwrap();
        let location = Location::discover(&root, true).unwrap();
        location.init("demo").unwrap();
        let store = location.open().unwrap();
        (root, store)
    }
    /// Two registration records, built against an empty set so they are the same bytes (and
    /// files) however many of them a store already holds.
    fn two_records() -> Vec<Entry> {
        ["demo-one", "demo-two"]
            .into_iter()
            .map(|id| {
                let record = record::Store::new()
                    .create(
                        id.to_string().try_into().unwrap(),
                        Current {
                            kind: Kind::Issue,
                            lifecycle: Lifecycle::NotStarted,
                            owner: None,
                            title: id.into(),
                            description: String::new(),
                            condition: None,
                            parent: None,
                            needs: Default::default(),
                        },
                        Context {
                            at: chrono::DateTime::from_timestamp(1000, 0).unwrap(),
                            recorder: None,
                        },
                    )
                    .unwrap();
                Entry::Record(record)
            })
            .collect()
    }

    fn records(count: usize) -> Vec<Entry> {
        (0..count)
            .map(|index| {
                let id = format!("demo-{index}");
                Entry::Record(
                    record::Store::new()
                        .create(
                            id.clone().try_into().unwrap(),
                            Current {
                                kind: Kind::Issue,
                                lifecycle: Lifecycle::NotStarted,
                                owner: None,
                                title: id,
                                description: String::new(),
                                condition: None,
                                parent: None,
                                needs: Default::default(),
                            },
                            Context {
                                at: chrono::DateTime::from_timestamp(1000, 0).unwrap(),
                                recorder: None,
                            },
                        )
                        .unwrap(),
                )
            })
            .collect()
    }
    fn temporary_files(store: &Store) -> usize {
        let mut count = 0;
        for subdirectory in fs::read_dir(store.records_path()).unwrap() {
            for file in fs::read_dir(subdirectory.unwrap().path()).unwrap() {
                if is_temporary(&file.unwrap().file_name()) {
                    count += 1;
                }
            }
        }
        count
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(24))]
        #[test]
        fn generated_publication_failure_matches_published_files(
            (count, existing, middle) in (3usize..7).prop_flat_map(|count| {
                (Just(count), prop::collection::vec(any::<bool>(), count), 1..count - 1)
            })
        ) {
          for scenario in 0..4 {
            let (root, mut store) = fixture();
            let mut existing = existing.clone();
            let stop = if scenario == 0 || scenario == 3 { 0 } else { middle };
            let missing_temp = scenario < 2;
            existing[0] = scenario == 3;
            existing[stop] = !missing_temp;
            existing[count - 1] = true;
            let entries = records(count);
            let paths: Vec<_> = entries.iter().map(|entry| {
                let id = RecordId::of(&record::encode(entry).unwrap());
                store.records_path().join(id.subdirectory()).join(id.as_ref())
            }).collect();
            for (index, path) in paths.iter().enumerate() {
                if existing[index] {
                    fs::create_dir_all(path.parent().unwrap()).unwrap();
                    fs::write(path, record::encode(&entries[index]).unwrap()).unwrap();
                }
            }
            let mut renamed = Vec::new();
            let error = store.update_with(|_, _, _| Ok((entries.clone(), ())), &mut |progress| {
                match progress {
                    Progress::BeforePublish if missing_temp && !existing[stop] => {
                        fs::remove_file(paths[stop].with_file_name(format!("{}{}", paths[stop].file_name().unwrap().to_str().unwrap(), TEMPORARY_SUFFIX))).unwrap();
                    }
                    Progress::Renamed(index) if index == stop && (!missing_temp || existing[stop]) => {
                        return Err(invalid("injected after rename"));
                    }
                    _ => {}
                }
                Ok(())
            }).unwrap_err();
            for index in 0..count {
                let published = existing[index] || index < stop || (!missing_temp && index == stop);
                prop_assert_eq!(paths[index].exists(), published);
                if !existing[index] && published {
                    renamed.push(paths[index].display().to_string());
                }
            }
            prop_assert_eq!(matches!(&error, Error::PublicationUnknown(_)), !renamed.is_empty());
            if renamed.is_empty() {
                prop_assert!(error.to_string().contains("not applied"));
            } else {
                let expected = format!("renamed so far: [{}]", renamed.join(", "));
                prop_assert!(error.to_string().contains(&expected));
            }
            prop_assert_eq!(temporary_files(&store), 0);
            store.update(|_, _, _| Ok((entries, ()))).unwrap();
            prop_assert_eq!(store.read().unwrap().1.len(), count);
            prop_assert!(paths.iter().all(|path| path.is_file()));
            prop_assert_eq!(temporary_files(&store), 0);
            drop(store);
            fs::remove_dir_all(root).unwrap();
          }
        }
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_header_and_record_files_are_reported_by_path() {
        use std::os::unix::fs::PermissionsExt;
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let (root, mut store) = fixture();
        store.update(|_, _, _| Ok((two_records(), ()))).unwrap();
        let first = store.read().unwrap().1;
        let (id, _) = first.entries().next().unwrap();
        let record = store
            .records_path()
            .join(id.subdirectory())
            .join(id.as_ref());
        fs::set_permissions(&record, PermissionsExt::from_mode(0o000)).unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(loaded.corruption.len(), 1);
        assert_eq!(
            loaded.corruption[0].path,
            Path::new(id.subdirectory()).join(id.as_ref())
        );
        assert!(loaded.corruption[0].reason.contains("unreadable"));
        assert_eq!(loaded.records.len(), 1);
        fs::set_permissions(&record, PermissionsExt::from_mode(0o644)).unwrap();
        let header = store.header_path();
        fs::set_permissions(&header, PermissionsExt::from_mode(0o000)).unwrap();
        let error = store.load().unwrap_err().to_string();
        assert!(
            error.contains("header.json") && error.contains("corrupt header"),
            "{error}"
        );
        fs::set_permissions(&header, PermissionsExt::from_mode(0o644)).unwrap();
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_failed_rename_is_classified_by_the_renames_before_it_and_leaves_no_temporary_file() {
        let (root, mut store) = fixture();
        let records = two_records();
        let temp_of = |store: &Store, index: usize| {
            let id = RecordId::of(&record::encode(&records[index]).unwrap());
            store
                .records_path()
                .join(id.subdirectory())
                .join(format!("{id}{TEMPORARY_SUFFIX}"))
        };
        let path_of = |store: &Store, index: usize| {
            let id = RecordId::of(&record::encode(&records[index]).unwrap());
            store
                .records_path()
                .join(id.subdirectory())
                .join(id.as_ref())
        };
        // The second record's temporary file vanishes before the renames: its rename fails
        // after the first record was renamed. Result unknown, listing the first only, and no
        // temporary file is left behind.
        let second_temp = temp_of(&store, 1);
        let error = store
            .update_with(|_, _, _| Ok((records.clone(), ())), &mut |progress| {
                if progress == Progress::BeforePublish {
                    fs::remove_file(&second_temp).unwrap();
                }
                Ok(())
            })
            .unwrap_err();
        assert!(matches!(&error, Error::PublicationUnknown(_)), "{error}");
        assert!(
            error.to_string().contains(&format!(
                "renamed so far: [{}]",
                path_of(&store, 0).display()
            )),
            "{error}"
        );
        assert_eq!(store.read().unwrap().1.len(), 1);
        assert_eq!(temporary_files(&store), 0);
        // The first record's temporary file vanishes instead: nothing is renamed, not
        // applied, and the second record's temporary file is removed.
        let (root2, mut store2) = fixture();
        let first_temp = temp_of(&store2, 0);
        let error = store2
            .update_with(|_, _, _| Ok((records.clone(), ())), &mut |progress| {
                if progress == Progress::BeforePublish {
                    fs::remove_file(&first_temp).unwrap();
                }
                Ok(())
            })
            .unwrap_err();
        assert!(matches!(&error, Error::Invalid(_)), "{error}");
        assert!(error.to_string().contains("not applied"), "{error}");
        assert_eq!(store2.read().unwrap().1.len(), 0);
        assert_eq!(temporary_files(&store2), 0);
        drop(store);
        drop(store2);
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(root2).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_records_directory_is_rejected() {
        let (root, store) = fixture();
        let elsewhere = root.join("elsewhere");
        fs::create_dir(&elsewhere).unwrap();
        fs::remove_dir(store.records_path()).unwrap();
        std::os::unix::fs::symlink(&elsewhere, store.records_path()).unwrap();
        let error = store.load().unwrap_err().to_string();
        assert!(error.contains("not a regular directory"), "{error}");
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_record_subdirectory_is_corruption_and_never_written_through() {
        let (root, mut store) = fixture();
        let records = two_records();
        let id = RecordId::of(&record::encode(&records[0]).unwrap());
        let elsewhere = root.join("elsewhere");
        fs::create_dir(&elsewhere).unwrap();
        std::os::unix::fs::symlink(&elsewhere, store.records_path().join(id.subdirectory()))
            .unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(loaded.corruption.len(), 1);
        assert_eq!(loaded.corruption[0].path, Path::new(id.subdirectory()));
        assert!(store.update(|_, _, _| Ok((records, ()))).is_err());
        assert_eq!(fs::read_dir(&elsewhere).unwrap().count(), 0);
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_temporary_path_is_replaced_without_following_it() {
        let (root, mut store) = fixture();
        let target = root.join("unrelated.txt");
        fs::write(&target, b"keep me").unwrap();
        let entry = two_records().remove(0);
        let id = RecordId::of(&record::encode(&entry).unwrap());
        let directory = store.records_path().join(id.subdirectory());
        fs::create_dir_all(&directory).unwrap();
        let temp = directory.join(format!("{id}{TEMPORARY_SUFFIX}"));
        std::os::unix::fs::symlink(&target, &temp).unwrap();
        store.update(|_, _, _| Ok((vec![entry], ()))).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"keep me");
        let published = directory.join(id.as_ref());
        assert!(
            fs::symlink_metadata(&published)
                .unwrap()
                .file_type()
                .is_file()
        );
        assert_eq!(store.read().unwrap().1.len(), 1);
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failures_before_and_after_the_first_rename_are_classified_and_leave_no_temporary_file() {
        let (root, mut store) = fixture();
        // Before any rename: nothing is applied and every temporary file is removed.
        let error = store
            .update_with(
                |_, _, _| Ok((two_records(), ())),
                &mut |progress| match progress {
                    Progress::BeforePublish => Err(invalid("injected before publish")),
                    Progress::Renamed(_) => Ok(()),
                },
            )
            .unwrap_err();
        assert!(matches!(&error, Error::Invalid(_)));
        assert!(error.to_string().contains("not applied"), "{error}");
        assert_eq!(store.read().unwrap().1.len(), 0);
        assert_eq!(temporary_files(&store), 0);
        // After the first rename: the first record is published, the rest is not, the result
        // is unknown and the untried temporary file is removed.
        let error = store
            .update_with(
                |_, _, _| Ok((two_records(), ())),
                &mut |progress| match progress {
                    Progress::Renamed(0) => Err(invalid("injected after the first rename")),
                    _ => Ok(()),
                },
            )
            .unwrap_err();
        assert!(matches!(&error, Error::PublicationUnknown(_)), "{error}");
        assert!(error.to_string().contains("result unknown"), "{error}");
        let first = store.read().unwrap().1;
        let (first_id, _) = first.entries().next().unwrap();
        assert!(
            error.to_string().contains(&format!(
                "renamed so far: [{}",
                store
                    .records_path()
                    .join(first_id.subdirectory())
                    .join(first_id.as_ref())
                    .display()
            )),
            "{error}"
        );
        assert_eq!(first.len(), 1);
        assert_eq!(temporary_files(&store), 0);
        // Retrying publishes only the missing record; the existing one is the same file.
        store.update(|_, _, _| Ok((two_records(), ()))).unwrap();
        assert_eq!(store.read().unwrap().1.len(), 2);
        // A failure at the second rename when the first record already existed is still
        // "not applied": no file was renamed.
        let mut third = two_records();
        third.truncate(1);
        let extra = record::Store::new()
            .create(
                "demo-three".to_string().try_into().unwrap(),
                Current {
                    kind: Kind::Group,
                    lifecycle: Lifecycle::NotStarted,
                    owner: None,
                    title: "three".into(),
                    description: String::new(),
                    condition: None,
                    parent: None,
                    needs: Default::default(),
                },
                Context {
                    at: chrono::DateTime::from_timestamp(1000, 0).unwrap(),
                    recorder: None,
                },
            )
            .unwrap();
        third.push(Entry::Record(extra));
        let error = store
            .update_with(|_, _, _| Ok((third, ())), &mut |progress| match progress {
                Progress::Renamed(0) => Err(invalid("injected after the existing record")),
                _ => Ok(()),
            })
            .unwrap_err();
        assert!(
            matches!(&error, Error::Invalid(_)) && error.to_string().contains("not applied"),
            "no file was renamed, so nothing was applied: {error}"
        );
        assert_eq!(store.read().unwrap().1.len(), 2);
        assert_eq!(temporary_files(&store), 0);
        drop(store);
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
        let _lock = lock(&root.join(WRITE_LOCK)).unwrap();
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
        let path = root.join(WRITE_LOCK);
        let thread = std::thread::spawn(move || {
            let _lock = lock(&path).unwrap();
            sender.send(()).unwrap();
        });
        assert!(receiver.recv_timeout(Duration::from_millis(100)).is_err());
        child.kill().unwrap();
        child.wait().unwrap();
        receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        thread.join().unwrap();
        assert!(root.join(WRITE_LOCK).is_file());
        fs::remove_dir_all(root).unwrap();
    }
}
