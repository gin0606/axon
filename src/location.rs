//! Storage discovery keeps Git boundaries and never falls back from a store it found.
use crate::{
    error::{Error, Result, invalid, validate_prefix},
    file::{self, HEADER_FILE, RECORDS_DIRECTORY, STORE_FILES, is_temporary},
    lifecycle::record::{Header, encode_header},
};
use std::{
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Clone)]
pub struct Location {
    /// The directory holding the selected `.axon`.
    pub root: PathBuf,
    /// The worktree root: the current one after discovery, the one holding an explicit root
    /// otherwise; the management root outside Git.
    pub worktree: PathBuf,
    git: Option<Git>,
}
#[derive(Clone)]
struct Git {
    common: PathBuf,
    linked: bool,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Initialized {
    /// Whether the management root is inside a Git repository.
    pub git: bool,
    /// A store in a linked worktree is not the one other worktrees fall back to.
    pub linked_worktree: bool,
}
pub fn present(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}
pub(crate) fn git_command(cwd: &Path) -> Command {
    let mut command = Command::new("git");
    command.current_dir(cwd);
    // Repository selection belongs to cwd, not a calling Git hook's environment.
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("GIT_") {
            command.env_remove(name);
        }
    }
    command
}
/// Absolute paths printed by one rev-parse, one per line.
fn rev_parse<const N: usize>(text: &str) -> Option<[PathBuf; N]> {
    let lines: Vec<_> = text.strip_suffix('\n')?.split('\n').collect();
    let paths: [&str; N] = lines.try_into().ok()?;
    Some(paths.map(PathBuf::from))
}
struct Discovered {
    root: PathBuf,
    common: PathBuf,
    git_dir: PathBuf,
}
impl Git {
    fn of(found: Discovered) -> Result<Self> {
        Ok(Self {
            linked: fs::canonicalize(&found.git_dir)? != fs::canonicalize(&found.common)?,
            common: found.common,
        })
    }
}
/// The Git directory, worktree root and common directory, from a single rev-parse.
fn git(cwd: &Path) -> Result<Option<Discovered>> {
    let mut boundary = None;
    // Every directory Git takes for a repository, a bare one included, holds `HEAD`. Without
    // it or a `.git` entry in any ancestor, rev-parse would find nothing and is not run. This
    // holds because `git_command` drops `GIT_*` variables that could point elsewhere.
    let mut repository_candidate = false;
    for ancestor in cwd.ancestors() {
        if present(&ancestor.join(".git"))? {
            boundary = Some(ancestor);
            repository_candidate = true;
            break;
        }
        repository_candidate |= present(&ancestor.join("HEAD"))?;
    }
    if !repository_candidate {
        return Ok(None);
    }
    let result = git_command(cwd)
        .args([
            "rev-parse",
            "--path-format=absolute",
            "--git-dir",
            "--show-toplevel",
            "--git-common-dir",
        ])
        .output();
    match result {
        Ok(output) if output.status.success() => {
            let text =
                String::from_utf8(output.stdout).map_err(|_| invalid("Git path is not UTF-8"))?;
            let Some([git_dir, root, common]) = rev_parse(&text) else {
                return Err(invalid(
                    "Git discovery failed: expected the Git directory, the working tree root and the common directory on three lines; a Git worktree whose path contains a newline is not supported, so move or rename it",
                ));
            };
            if let Some(boundary) = boundary
                && fs::canonicalize(&root)? != fs::canonicalize(boundary)?
            {
                return Err(invalid(format!(
                    "Git discovery failed: Git skipped the repository marker at {}",
                    boundary.join(".git").display()
                )));
            }
            Ok(Some(Discovered {
                root,
                common,
                git_dir,
            }))
        }
        failed => {
            // A bare repository prints its Git directory before --show-toplevel fails;
            // outside Git the first option fails without printing a path.
            if boundary.is_none()
                && !failed
                    .as_ref()
                    .is_ok_and(|output| !output.stdout.is_empty())
            {
                return Ok(None);
            }
            match failed {
                Ok(output) => Err(invalid(format!(
                    "Git discovery failed (a working tree is required): {}",
                    String::from_utf8_lossy(&output.stderr)
                ))),
                Err(error) => Err(error.into()),
            }
        }
    }
}
/// The main worktree, when `common` is the `.git` directory directly inside it. The common
/// directory of a bare repository or a submodule lives elsewhere, and its parent may be shared
/// with unrelated repositories.
fn main_worktree(common: &Path) -> Result<Option<PathBuf>> {
    let Some(parent) = common.parent() else {
        return Ok(None);
    };
    if common.file_name().is_none_or(|name| name != ".git") {
        return Ok(None);
    }
    // An unanswered question is not "no main worktree": that would hide its store.
    let undetermined = |detail: &str| {
        invalid(format!(
            "cannot determine the main worktree from {}: {detail}",
            common.display()
        ))
    };
    let output = git_command(parent)
        .args([
            "rev-parse",
            "--is-bare-repository",
            "--path-format=absolute",
            "--git-common-dir",
            "--show-toplevel",
        ])
        .output()
        .map_err(|error| undetermined(&error.to_string()))?;
    let text = String::from_utf8(output.stdout).map_err(|_| invalid("Git path is not UTF-8"))?;
    // A bare repository named `.git` answers before rev-parse fails for want of a working tree.
    if text.lines().next() == Some("true") {
        return Ok(None);
    }
    if !output.status.success() {
        return Err(undetermined(String::from_utf8_lossy(&output.stderr).trim()));
    }
    let Some([_, main_common, root]) = rev_parse(&text) else {
        return Err(undetermined("unexpected git rev-parse output"));
    };
    let canonical =
        |path: &Path| fs::canonicalize(path).map_err(|error| undetermined(&error.to_string()));
    let same =
        canonical(&root)? == canonical(parent)? && canonical(&main_common)? == canonical(common)?;
    Ok(same.then_some(root))
}
/// What a `.axon` directory holds for discovery and initialization.
pub enum Presence {
    /// No `.axon`, or only the residue of an interrupted initialization.
    Absent,
    /// A header file: a store.
    Store,
    /// Something else: records without a header, an earlier format, a foreign file.
    Obstructed(PathBuf),
}
/// The first file in the records directory that is not residue (empty subdirectories and
/// temporary files are), if any.
fn first_record_file(path: &Path) -> Result<Option<PathBuf>> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if is_temporary(&entry.file_name()) {
            continue;
        }
        if !entry.file_type()?.is_dir() {
            return Ok(Some(entry.path()));
        }
        for inner in fs::read_dir(entry.path())? {
            let inner = inner?;
            if !is_temporary(&inner.file_name()) {
                return Ok(Some(inner.path()));
            }
        }
    }
    Ok(None)
}
/// Classifies the `.axon` of a management root. A directory that is not a regular directory
/// is an error.
pub fn presence(root: &Path) -> Result<Presence> {
    let directory = root.join(".axon");
    if !present(&directory)? {
        return Ok(Presence::Absent);
    }
    if !fs::symlink_metadata(&directory)?.file_type().is_dir() {
        return Err(invalid("management directory is not a regular directory"));
    }
    if present(&directory.join(HEADER_FILE))? {
        return Ok(Presence::Store);
    }
    for entry in fs::read_dir(&directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let text = name.to_string_lossy();
        let path = entry.path();
        let residue = if text.ends_with(".lock") || is_temporary(&name) {
            true
        } else if name == RECORDS_DIRECTORY {
            if !entry.file_type()?.is_dir() {
                return Ok(Presence::Obstructed(path));
            }
            match first_record_file(&path)? {
                Some(file) => return Ok(Presence::Obstructed(file)),
                None => true,
            }
        } else if let Some((_, written)) = STORE_FILES.iter().find(|(file, _)| name == *file) {
            fs::symlink_metadata(&path)?.file_type().is_file()
                && file::crlf_as_lf(&fs::read(&path)?) == written.as_bytes()
        } else {
            false
        };
        if !residue {
            return Ok(Presence::Obstructed(path));
        }
    }
    Ok(Presence::Absent)
}
/// Refuses an unmerged Git index under the `.axon` of `root`, which must be inside Git.
fn unmerged(root: &Path) -> Result<()> {
    let inspected = |out: std::process::Output| {
        if out.status.success() {
            Ok(out.stdout)
        } else {
            Err(invalid(format!(
                "cannot inspect Git index: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )))
        }
    };
    let listed = inspected(
        git_command(root)
            .args(["ls-files", "--unmerged", "--full-name", "-z", "--", ".axon"])
            .output()?,
    )?;
    if listed.is_empty() {
        return Ok(());
    }
    // Each entry is `<mode> <object> <stage>\t<path>`, one per stage of the same path.
    let mut paths: Vec<Vec<u8>> = Vec::new();
    for entry in listed
        .split(|&byte| byte == 0)
        .filter(|entry| !entry.is_empty())
    {
        let Some(tab) = entry.iter().position(|&byte| byte == b'\t') else {
            return Err(invalid(
                "cannot inspect Git index: unexpected ls-files output",
            ));
        };
        // Git lists the stages of one path together.
        let path = &entry[tab + 1..];
        if paths.last().map(Vec::as_slice) != Some(path) {
            paths.push(path.to_vec());
        }
    }
    // Only a failing check pays for the worktree's name.
    let toplevel = inspected(
        git_command(root)
            .args(["rev-parse", "--show-toplevel"])
            .output()?,
    )?;
    let worktree = String::from_utf8_lossy(&toplevel);
    Err(Error::Unmerged {
        worktree: PathBuf::from(worktree.strip_suffix('\n').unwrap_or(&worktree)),
        paths: paths.into_iter().map(path_from_bytes).collect(),
    })
}
/// A path as Git printed it; outside Unix only UTF-8 survives.
fn path_from_bytes(bytes: Vec<u8>) -> PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        PathBuf::from(std::ffi::OsString::from_vec(bytes))
    }
    #[cfg(not(unix))]
    {
        PathBuf::from(String::from_utf8_lossy(&bytes).into_owned())
    }
}
/// [`settles`] inside Git. Short of a header, the candidate's index is checked first, whether
/// its `.axon` holds other files, residue or nothing: an unmerged index there means an
/// integration left unresolved, which explains a missing header better than "not a store" or
/// "not initialized", and a store this worktree tracks is not skipped for another one. With a
/// header, the store's own guard checks the index before anything is read.
fn settles_in_git(root: &Path) -> Result<bool> {
    // An unreadable `.axon` is reported by `settles`, after the index.
    if let Ok(Presence::Store) = presence(root) {
        return Ok(true);
    }
    unmerged(root)?;
    settles(root)
}
/// Whether a header settles discovery here. Records without a header, or any other file,
/// stop discovery: they are not silently skipped for a store further away.
fn settles(root: &Path) -> Result<bool> {
    match presence(root)? {
        Presence::Absent => Ok(false),
        Presence::Store => Ok(true),
        Presence::Obstructed(file) => Err(Error::NotAStore {
            root: root.to_path_buf(),
            file,
        }),
    }
}
impl Location {
    /// `init` targets the current worktree and never the store a linked worktree would share.
    pub fn discover(cwd: &Path, init: bool) -> Result<Self> {
        let cwd = fs::canonicalize(cwd)?;
        if let Some(found) = git(&cwd)? {
            let worktree = found.root.clone();
            let git = Git::of(found)?;
            let mut root = worktree.clone();
            // An unmerged index or a directory that is not a store stops here; only an absent
            // store falls through to the main worktree's.
            if !init
                && !settles_in_git(&root)?
                && git.linked
                && let Some(main) = main_worktree(&git.common)?
                && settles_in_git(&main)?
            {
                root = main;
            }
            return Ok(Self {
                root,
                worktree,
                git: Some(git),
            });
        }
        for root in cwd.ancestors() {
            if settles(root)? {
                if init && root != cwd {
                    return Err(invalid(format!(
                        "already inside a management root: {}",
                        root.display()
                    )));
                }
                return Ok(Self {
                    root: root.into(),
                    worktree: root.into(),
                    git: None,
                });
            }
        }
        if init {
            Ok(Self {
                root: cwd.clone(),
                worktree: cwd,
                git: None,
            })
        } else {
            Err(invalid(
                "not initialized; run axon init in the intended management root",
            ))
        }
    }
    /// A management root given explicitly, without discovery: what `axon storage check ROOT`
    /// inspects. Git is consulted as in discovery, so inside a Git worktree its index is
    /// checked and a Git boundary discovery rejects is an error here too.
    pub fn explicit(root: &Path) -> Result<Self> {
        let root = fs::canonicalize(root)?;
        let Some(found) = git(&root)? else {
            return Ok(Self {
                worktree: root.clone(),
                root,
                git: None,
            });
        };
        Ok(Self {
            root,
            worktree: found.root.clone(),
            git: Some(Git::of(found)?),
        })
    }
    /// A management root that an application owns, opened without discovery and without Git
    /// even when it lies inside a Git worktree: no Git process runs, no index is checked, and
    /// initialization takes its lock inside `.axon` as it does outside Git.
    pub fn standalone(root: &Path) -> Result<Self> {
        let root = fs::canonicalize(root)?;
        Ok(Self {
            worktree: root.clone(),
            root,
            git: None,
        })
    }
    pub(crate) fn header(&self) -> PathBuf {
        self.root.join(".axon").join(HEADER_FILE)
    }
    /// Whether the header file exists.
    pub fn initialized(&self) -> Result<bool> {
        present(&self.header())
    }
    /// Refuses an unmerged Git index under `.axon/`; outside Git there is no index to check.
    pub fn check_index(&self) -> Result<()> {
        if self.git.is_some() {
            unmerged(&self.root)?;
        }
        Ok(())
    }
    /// Inside Git an unmerged index is reported before a missing header, as in discovery.
    pub fn open(&self) -> Result<file::Store> {
        let settled = match self.git {
            Some(_) => settles_in_git(&self.root)?,
            None => settles(&self.root)?,
        };
        if !settled {
            return Err(invalid("not initialized; run axon init"));
        }
        file::Store::at(self.clone())
    }
    /// Creates `.axon/records/`, `.axon/.gitignore`, `.axon/.gitattributes` and, last, the
    /// header under the initialization lock. Anything but the residue of an interrupted
    /// initialization is refused with its path.
    pub fn init(&self, prefix: &str) -> Result<Initialized> {
        validate_prefix(prefix)?;
        let directory = self.root.join(".axon");
        let prepare = || -> Result<()> {
            fs::create_dir_all(&directory)?;
            if !fs::symlink_metadata(&directory)?.file_type().is_dir() {
                return Err(invalid("management directory is not a regular directory"));
            }
            Ok(())
        };
        // Inside Git the lock lives in the common directory, so a rejected init leaves no
        // `.axon` behind in this worktree.
        let lock_root = match &self.git {
            Some(git) => git.common.clone(),
            None => {
                prepare()?;
                directory.clone()
            }
        };
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_root.join("axon-init.lock"))?;
        lock.lock()?;
        self.check_index()?;
        match presence(&self.root)? {
            Presence::Absent => {}
            Presence::Store => {
                return Err(invalid(
                    "already initialized; init never repairs or replaces an existing store",
                ));
            }
            Presence::Obstructed(path) => {
                return Err(invalid(format!(
                    "{} is not empty: {} is not a store file that init creates; an earlier format is not converted, and init never repairs or replaces an existing store",
                    directory.display(),
                    path.display()
                )));
            }
        }
        let linked_worktree = self.git.as_ref().is_some_and(|git| git.linked);
        // Worktrees without a store of their own read the main worktree's. A second store here
        // would silently take over, and as a separate store it could never be merged back.
        if linked_worktree
            && let Some(git) = &self.git
            && let Some(main) = main_worktree(&git.common)?
        {
            // A store whose header an unresolved merge removed is still the main worktree's.
            if settles_in_git(&main)? {
                return Err(invalid(format!(
                    "the main worktree already holds a store at {}; this worktree uses it, and init never creates a second store beside it",
                    main.join(".axon").display()
                )));
            }
        }
        prepare()?;
        let header = self.header();
        let result = (|| -> Result<Initialized> {
            fs::create_dir_all(directory.join(RECORDS_DIRECTORY))?;
            for (name, content) in STORE_FILES {
                let path = directory.join(name);
                // Residue with CRLF line endings is replaced: under `* -text` Git would stage
                // it as is. Anything else already there is left alone.
                let replace = match file::optional_bytes(&path)? {
                    None => true,
                    Some(bytes) => {
                        bytes != content.as_bytes()
                            && file::crlf_as_lf(&bytes) == content.as_bytes()
                    }
                };
                if replace {
                    let temp = file::temporary(&path, content.as_bytes())?;
                    fs::rename(&temp, &path)?;
                }
            }
            // `.axon/` itself, the record directory, the ignore file and the attributes file are
            // durable before the header, which marks the store, is published. `.axon/` may be
            // the residue of an interrupted initialization that never synced its entry.
            file::sync_directory(&self.root, file::Reach::Ordered)?;
            file::sync_directory(&directory, file::Reach::Ordered)?;
            let bytes = encode_header(&Header::new(prefix)?)?;
            let temp = file::temporary(&header, &bytes)?;
            fs::rename(&temp, &header)?;
            file::sync_directory(&directory, file::Reach::Durable)?;
            Ok(Initialized {
                git: self.git.is_some(),
                linked_worktree,
            })
        })();
        result.map_err(|e| {
            invalid(format!(
                "initialization failed at {}: {e}; without {} the store is not initialized and init can be retried, with it the store exists but its directory entry may not be durable yet",
                directory.display(),
                header.display()
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_standalone_root_never_consults_git() {
        let base =
            std::env::temp_dir().join(format!("axon-standalone-{:032x}", rand::random::<u128>()));
        // An empty `.git` makes Git discovery fail: only a root that skips Git can use it.
        fs::create_dir_all(base.join(".git")).unwrap();
        let root = base.join("app/project");
        fs::create_dir_all(&root).unwrap();
        assert!(Location::explicit(&root).is_err());

        let location = Location::standalone(&root).unwrap();
        assert!(!location.initialized().unwrap());
        location.init("demo").unwrap();
        assert!(location.initialized().unwrap());
        location.check_index().unwrap();
        let (header, _, view) = location.open().unwrap().read().unwrap();
        assert_eq!(header.prefix, "demo");
        assert_eq!(view.known().count(), 0);
        assert!(root.join(".axon/axon-init.lock").exists());
        assert!(!base.join(".git/axon-init.lock").exists());
        fs::remove_dir_all(&base).unwrap();
    }
}
