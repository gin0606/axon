//! The approvals of condition commands: the digests of the commands this machine may run for
//! one store. They are not records and live outside `.axon/`, where Git does not track them.
//! The contract is in `docs/reference/candidates.md` (条件の許可) and
//! `docs/reference/storage.md` (Axon が `.axon/` の外に書くもの).
use crate::{
    file::{self, Reach},
    lifecycle::StoreId,
    location::{Location, git_command, rev_parse},
};
use std::{
    collections::BTreeSet,
    fs,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

/// The name of the approvals file inside the common Git directory, and of the directory that
/// holds one approvals file per management root inside the state directory outside Git.
pub const APPROVALS_NAME: &str = "axon-condition-approvals";
/// The first line of an approvals file.
pub const FORMAT: &str = "axon-condition-approvals/v1";
/// The length of a digest in lowercase hexadecimal.
pub const DIGEST_LENGTH: usize = 64;

/// The digest of a condition command: the BLAKE3 hash of its UTF-8 bytes in lowercase hex.
pub fn digest(command: &str) -> String {
    blake3::hash(command.as_bytes()).to_hex().to_string()
}
/// A complete digest: 64 lowercase hexadecimal characters.
pub fn is_digest(value: &str) -> bool {
    file::is_hex_name(value, DIGEST_LENGTH)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProblemKind {
    /// The file exists and cannot be read as approvals: its owner fixes or removes it.
    Unreadable,
    /// The file or its directory or lock cannot be written.
    Unwritable,
    /// Outside Git, no absolute state directory is known.
    NoStateDirectory,
    /// The place of the approvals file could not be determined: Git could not name the
    /// common Git directory, or a path could not be resolved.
    Unlocated,
    /// Inside Git, the place Git names is not the worktree's own `.git`.
    Untrusted,
}
/// Why the approvals could not be read or written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub kind: ProblemKind,
    /// The approvals file, when its place is known.
    pub path: Option<PathBuf>,
    pub reason: String,
}
impl Problem {
    fn new(kind: ProblemKind, path: Option<&Path>, reason: impl Into<String>) -> Self {
        Self {
            kind,
            path: path.map(Path::to_path_buf),
            reason: reason.into(),
        }
    }
    /// The `DETAIL` a diagnostic shows: the path and the reason, then what to fix.
    pub fn detail(&self) -> String {
        let mut detail = match &self.path {
            Some(path) => format!("{}: {}", path.display(), self.reason),
            None => self.reason.clone(),
        };
        match (self.kind, &self.path) {
            (ProblemKind::Unreadable, Some(path)) => detail.push_str(&format!(
                "; fix or remove {} (removing it discards every approval)",
                path.display()
            )),
            (ProblemKind::NoStateDirectory, _) => detail.push_str("; set HOME to an absolute path"),
            _ => {}
        }
        detail
    }
}
impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.detail())
    }
}

/// The approved digests of one store, read once.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Approvals {
    digests: BTreeSet<String>,
}
impl Approvals {
    pub fn contains(&self, digest: &str) -> bool {
        self.digests.contains(digest)
    }
    /// Whether the command may run.
    pub fn allows(&self, command: &str) -> bool {
        self.contains(&digest(command))
    }
}

/// The outcome of adding a digest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Added {
    New,
    Already,
}
/// A write that failed before the file was replaced, or after it was, when the approval may
/// be in effect.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WriteError {
    NotApplied(Problem),
    Unknown(Problem),
}

/// The approvals file of the store at `location` with this store ID.
pub fn path(location: &Location, store: &StoreId) -> Result<PathBuf, Problem> {
    if location.in_git() {
        Ok(trusted_common_directory(&location.root)?.join(APPROVALS_NAME))
    } else {
        let state = state_directory().ok_or_else(|| {
            Problem::new(
                ProblemKind::NoStateDirectory,
                None,
                "no absolute state directory or local data directory is known",
            )
        })?;
        let root = fs::canonicalize(&location.root).map_err(|error| {
            Problem::new(
                ProblemKind::Unlocated,
                None,
                format!(
                    "cannot resolve the management root {}: {error}",
                    location.root.display()
                ),
            )
        })?;
        Ok(state.join(APPROVALS_NAME).join(key(store, &root)))
    }
}
/// The state directory outside Git: `dirs::state_dir()`, else `dirs::data_local_dir()`, each
/// only when absolute.
fn state_directory() -> Option<PathBuf> {
    dirs::state_dir()
        .filter(|path| path.is_absolute())
        .or_else(|| dirs::data_local_dir().filter(|path| path.is_absolute()))
}
/// The name of a management root's approvals file outside Git: the store ID and the resolved
/// root path together, so neither a store moved elsewhere nor another store put at the same
/// path inherits the approvals.
fn key(store: &StoreId, root: &Path) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(store.as_ref().as_bytes());
    hasher.update(&[0]);
    hasher.update(root.as_os_str().as_bytes());
    hasher.finalize().to_hex().to_string()
}
/// The common Git directory of the worktree whose root is `root`, asked from that root and
/// trusted only when the Git directory comes from the `.git` directly inside it. A directory
/// shaped like a bare repository in a received archive could otherwise name a place inside
/// the worktree that holds someone else's approvals.
fn trusted_common_directory(root: &Path) -> Result<PathBuf, Problem> {
    let unlocated = |reason: String| Problem::new(ProblemKind::Unlocated, None, reason);
    let output = git_command(root)
        .args([
            "rev-parse",
            "--path-format=absolute",
            "--git-dir",
            "--git-common-dir",
        ])
        .output()
        .map_err(|error| unlocated(format!("git rev-parse could not run: {error}")))?;
    if !output.status.success() {
        return Err(unlocated(format!(
            "git rev-parse failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let text = String::from_utf8(output.stdout)
        .map_err(|_| unlocated("git rev-parse printed a path that is not UTF-8".into()))?;
    let Some([git_dir, common]) = rev_parse::<2>(&text) else {
        return Err(unlocated(
            "git rev-parse did not print the Git directory and the common Git directory on two lines".into(),
        ));
    };
    let canonical = |path: &Path| {
        fs::canonicalize(path)
            .map_err(|error| unlocated(format!("cannot resolve {}: {error}", path.display())))
    };
    let root = canonical(root)?;
    let git_dir = canonical(&git_dir)?;
    let common = canonical(&common)?;
    let untrusted = |reason: &str| {
        Problem::new(
            ProblemKind::Untrusted,
            None,
            format!(
                "{reason} (Git directory {}, common Git directory {})",
                git_dir.display(),
                common.display()
            ),
        )
    };
    let marker = root.join(".git");
    match fs::symlink_metadata(&marker) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(untrusted("the worktree root has no .git"));
        }
        Err(error) => {
            return Err(unlocated(format!(
                "cannot inspect {}: {error}",
                marker.display()
            )));
        }
    }
    // Like Git, follow a link at `.git`: it may name a directory or a gitfile, whose relative
    // target Git reads against the worktree root.
    let expected = if fs::metadata(&marker).is_ok_and(|metadata| metadata.is_file()) {
        gitfile_target(&marker, &root).map_err(unlocated)?
    } else {
        canonical(&marker)?
    };
    if git_dir != expected {
        return Err(untrusted(
            "the Git directory does not come from .git in the worktree root",
        ));
    }
    // Inside the worktree only the `.git` directory itself may hold them; a gitfile or a link
    // naming another place inside the worktree does not.
    if common.starts_with(&root) && common != marker {
        return Err(untrusted(
            "the common Git directory is inside the worktree but is not its .git",
        ));
    }
    Ok(common)
}
/// The directory a gitfile points to, resolved.
fn gitfile_target(marker: &Path, root: &Path) -> Result<PathBuf, String> {
    let text = fs::read_to_string(marker)
        .map_err(|error| format!("cannot read {}: {error}", marker.display()))?;
    let target = text
        .strip_prefix("gitdir:")
        .map(str::trim)
        .filter(|target| !target.is_empty())
        .ok_or_else(|| format!("{} is not a gitfile", marker.display()))?;
    let target = root.join(target);
    fs::canonicalize(&target)
        .map_err(|error| format!("cannot resolve {}: {error}", target.display()))
}

/// Reads the approvals of the store at `location`. A missing file is no approvals.
pub fn read(location: &Location, store: &StoreId) -> Result<Approvals, Problem> {
    read_file(&path(location, store)?)
}
fn read_file(path: &Path) -> Result<Approvals, Problem> {
    let unreadable = |reason: String| Problem::new(ProblemKind::Unreadable, Some(path), reason);
    match file::optional_bytes(path) {
        Ok(None) => Ok(Approvals::default()),
        Ok(Some(bytes)) => parse(&bytes)
            .map(|digests| Approvals { digests })
            .map_err(unreadable),
        Err(crate::Error::Invalid(_)) => Err(unreadable("not a regular file".into())),
        Err(error) => Err(unreadable(error.to_string())),
    }
}
/// The digests of an approvals file. Blank lines, surrounding whitespace, CRLF, a missing
/// final LF, order and duplicates are accepted, so a hand-edited file still reads.
fn parse(bytes: &[u8]) -> Result<BTreeSet<String>, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "not UTF-8 text".to_string())?;
    // Physical line numbers, so a diagnostic points at the line to fix.
    let mut lines = text
        .split('\n')
        .map(str::trim)
        .enumerate()
        .filter(|(_, line)| !line.is_empty());
    match lines.next() {
        None => return Ok(BTreeSet::new()),
        Some((_, FORMAT)) => {}
        Some(_) => return Err(format!("the first line is not {FORMAT}")),
    }
    let mut digests = BTreeSet::new();
    for (index, line) in lines {
        if !is_digest(line) {
            return Err(format!(
                "line {} is not 64 lowercase hexadecimal characters",
                index + 1
            ));
        }
        digests.insert(line.to_string());
    }
    Ok(digests)
}
fn encode(digests: &BTreeSet<String>) -> String {
    let mut text = format!("{FORMAT}\n");
    for digest in digests {
        text.push_str(digest);
        text.push('\n');
    }
    text
}

/// Adds a digest to the approvals of the store at `location` under the approvals lock: the
/// file is read again, rewritten whole to a temporary file and renamed into place. A file
/// that cannot be read is never overwritten.
pub fn approve(location: &Location, store: &StoreId, digest: &str) -> Result<Added, WriteError> {
    let path = path(location, store).map_err(WriteError::NotApplied)?;
    approve_at(&path, digest, |directory| {
        file::sync_directory(directory, Reach::Durable).map_err(|error| error.to_string())
    })
}
fn approve_at(
    path: &Path,
    digest: &str,
    sync: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<Added, WriteError> {
    if !is_digest(digest) {
        return Err(WriteError::NotApplied(Problem::new(
            ProblemKind::Unwritable,
            Some(path),
            format!("{digest:?} is not a digest: expected 64 lowercase hexadecimal characters"),
        )));
    }
    let unwritable = |reason: String| {
        WriteError::NotApplied(Problem::new(ProblemKind::Unwritable, Some(path), reason))
    };
    let directory = path
        .parent()
        .ok_or_else(|| unwritable("no parent directory".into()))?;
    fs::create_dir_all(directory)
        .map_err(|error| unwritable(format!("cannot create {}: {error}", directory.display())))?;
    let mut lock_name = path.as_os_str().to_os_string();
    lock_name.push(".lock");
    let lock_path = PathBuf::from(lock_name);
    let _lock = file::lock(&lock_path)
        .map_err(|error| unwritable(format!("cannot lock {}: {error}", lock_path.display())))?;
    let mut digests = read_file(path).map_err(WriteError::NotApplied)?.digests;
    if !digests.insert(digest.to_string()) {
        return Ok(Added::Already);
    }
    let temp = file::temporary(path, encode(&digests).as_bytes())
        .map_err(|error| unwritable(error.to_string()))?;
    if let Err(error) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(unwritable(format!("cannot rename into place: {error}")));
    }
    sync(directory).map_err(|reason| {
        WriteError::Unknown(Problem::new(ProblemKind::Unwritable, Some(path), reason))
    })?;
    Ok(Added::New)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "axon-approval-test-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir(&path).unwrap();
        fs::canonicalize(path).unwrap()
    }
    const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    #[test]
    fn digests_are_blake3_of_the_utf8_bytes_in_lowercase_hex() {
        let value = digest("test -f ready.txt");
        assert!(is_digest(&value));
        assert_eq!(
            value,
            blake3::hash(b"test -f ready.txt").to_hex().to_string()
        );
        assert!(!is_digest(&value.to_uppercase()));
        assert!(!is_digest(&value[1..]));
    }

    #[test]
    fn hand_edited_files_read_and_anything_else_is_unreadable() {
        for (text, expected) in [
            ("", vec![]),
            ("\n \n", vec![]),
            ("axon-condition-approvals/v1\n", vec![]),
            (
                "\r\n  axon-condition-approvals/v1  \r\n\r\nbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\r\n aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\nbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                vec![A, B],
            ),
        ] {
            let parsed = parse(text.as_bytes()).unwrap();
            assert_eq!(
                parsed.iter().map(String::as_str).collect::<Vec<_>>(),
                expected
            );
        }
        for text in [
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
            "axon-condition-approvals/v2\n",
            "axon-condition-approvals/v1\nAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\n",
            "axon-condition-approvals/v1\nabc\n",
        ] {
            assert!(parse(text.as_bytes()).is_err(), "{text:?}");
        }
        assert!(parse(b"\xff").is_err());
        // The line a diagnostic names is the physical one, blank lines included.
        let error = parse(format!("{FORMAT}\n\n{A}\n\nnot a digest\n").as_bytes()).unwrap_err();
        assert_eq!(error, "line 5 is not 64 lowercase hexadecimal characters");
    }

    #[test]
    fn approving_rewrites_the_file_sorted_and_reports_an_existing_digest() {
        let dir = scratch();
        let path = dir.join("nested").join("approvals");
        assert_eq!(read_file(&path).unwrap(), Approvals::default());
        assert_eq!(approve_at(&path, B, |_| Ok(())), Ok(Added::New));
        assert_eq!(approve_at(&path, A, |_| Ok(())), Ok(Added::New));
        assert_eq!(approve_at(&path, A, |_| Ok(())), Ok(Added::Already));
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            format!("{FORMAT}\n{A}\n{B}\n")
        );
        assert!(read_file(&path).unwrap().contains(A));
        // An empty file is no approvals and is replaced.
        fs::write(&path, "").unwrap();
        assert_eq!(approve_at(&path, A, |_| Ok(())), Ok(Added::New));
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            format!("{FORMAT}\n{A}\n")
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn only_a_complete_digest_is_written() {
        let dir = scratch();
        let path = dir.join("approvals");
        for value in [&A.to_uppercase(), &A[1..], "x"] {
            assert!(matches!(
                approve_at(&path, value, |_| Ok(())),
                Err(WriteError::NotApplied(_))
            ));
        }
        assert!(!path.exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_unreadable_file_is_never_overwritten() {
        let dir = scratch();
        let path = dir.join("approvals");
        fs::write(&path, "not approvals\n").unwrap();
        let Err(WriteError::NotApplied(problem)) = approve_at(&path, A, |_| Ok(())) else {
            panic!("expected a refusal");
        };
        assert_eq!(problem.kind, ProblemKind::Unreadable);
        assert!(
            problem
                .detail()
                .ends_with("(removing it discards every approval)"),
            "{}",
            problem.detail()
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "not approvals\n");
        fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(dir.join("elsewhere"), &path).unwrap();
        assert_eq!(read_file(&path).unwrap_err().kind, ProblemKind::Unreadable);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_failed_sync_after_the_rename_is_unknown() {
        let dir = scratch();
        let path = dir.join("approvals");
        let Err(WriteError::Unknown(problem)) = approve_at(&path, A, |_| Err("no sync".into()))
        else {
            panic!("expected an unknown result");
        };
        assert!(problem.detail().contains("no sync"));
        assert!(read_file(&path).unwrap().contains(A));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_key_outside_git_depends_on_the_store_and_the_root() {
        let store: StoreId = "s-one".to_string().try_into().unwrap();
        let other: StoreId = "s-two".to_string().try_into().unwrap();
        let base = key(&store, Path::new("/a"));
        assert!(is_digest(&base));
        assert_ne!(base, key(&other, Path::new("/a")));
        assert_ne!(base, key(&store, Path::new("/b")));
    }
}
