//! Storage discovery keeps Git boundaries and never falls back from a store it found.
use crate::{
    error::{Result, invalid, validate_prefix},
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
    /// The current worktree root; the management root outside Git.
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
/// The worktree root, the common directory and the Git directory, from a single rev-parse.
fn git(cwd: &Path) -> Result<Option<Discovered>> {
    let mut boundary = None;
    for ancestor in cwd.ancestors() {
        if present(&ancestor.join(".git"))? {
            boundary = Some(ancestor);
            break;
        }
    }
    let result = git_command(cwd)
        .args([
            "rev-parse",
            "--show-toplevel",
            "--path-format=absolute",
            "--git-common-dir",
            "--git-dir",
        ])
        .output();
    match result {
        Ok(output) if output.status.success() => {
            let text =
                String::from_utf8(output.stdout).map_err(|_| invalid("Git path is not UTF-8"))?;
            let Some([root, common, git_dir]) = rev_parse(&text) else {
                return Err(invalid(
                    "Git discovery failed: expected the working tree root, the common directory and the Git directory on three lines; a Git worktree whose path contains a newline is not supported, so move or rename it",
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
            // Bare repositories have no .git entry but still form a boundary.
            let repository = git_command(cwd).args(["rev-parse", "--git-dir"]).output();
            if boundary.is_none() && !repository.is_ok_and(|o| o.status.success()) {
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
                && fs::read(&path)? == written.as_bytes()
        } else {
            false
        };
        if !residue {
            return Ok(Presence::Obstructed(path));
        }
    }
    Ok(Presence::Absent)
}
/// The management root whose `.axon` stopped discovery: the first candidate in discovery
/// order (the current worktree root, then the main worktree's; outside Git each ancestor of
/// `cwd`) that holds records or foreign files without a header, unless a store comes first.
pub fn obstructed_root(cwd: &Path) -> Result<Option<PathBuf>> {
    let cwd = fs::canonicalize(cwd)?;
    let candidates = match git(&cwd)? {
        Some(found) => {
            let mut roots = vec![found.root];
            if fs::canonicalize(&found.git_dir)? != fs::canonicalize(&found.common)?
                && let Some(main) = main_worktree(&found.common)?
            {
                roots.push(main);
            }
            roots
        }
        None => cwd.ancestors().map(Path::to_path_buf).collect(),
    };
    for root in candidates {
        match presence(&root)? {
            Presence::Absent => {}
            Presence::Store => return Ok(None),
            Presence::Obstructed(_) => return Ok(Some(root)),
        }
    }
    Ok(None)
}
/// Whether a header settles discovery here. Records without a header, or any other file,
/// stop discovery: they are not silently skipped for a store further away.
fn settles(root: &Path) -> Result<bool> {
    match presence(root)? {
        Presence::Absent => Ok(false),
        Presence::Store => Ok(true),
        Presence::Obstructed(path) => Err(invalid(format!(
            "{} is not a store: no {HEADER_FILE} beside {}; an earlier format or a foreign file is not converted",
            root.join(".axon").display(),
            path.display()
        ))),
    }
}
impl Location {
    /// `init` targets the current worktree and never the store a linked worktree would share.
    pub fn discover(cwd: &Path, init: bool) -> Result<Self> {
        let cwd = fs::canonicalize(cwd)?;
        if let Some(found) = git(&cwd)? {
            let git = Git {
                linked: fs::canonicalize(&found.git_dir)? != fs::canonicalize(&found.common)?,
                common: found.common,
            };
            let mut root = found.root.clone();
            // A directory that is not a store stops here; only an absent store falls through
            // to the main worktree's.
            if !init
                && !settles(&root)?
                && git.linked
                && let Some(main) = main_worktree(&git.common)?
                && settles(&main)?
            {
                root = main;
            }
            return Ok(Self {
                root,
                worktree: found.root,
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
    /// inspects. Git is not consulted.
    pub fn explicit(root: &Path) -> Result<Self> {
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
    pub fn check_index(&self) -> Result<()> {
        if self.git.is_some() {
            let out = git_command(&self.root)
                .args(["ls-files", "--unmerged", "--", ".axon"])
                .output()?;
            if !out.status.success() {
                return Err(invalid("cannot inspect Git index"));
            }
            if !out.stdout.is_empty() {
                return Err(invalid(
                    "unmerged Git index under .axon/; resolve and stage it before normal operations",
                ));
            }
        }
        Ok(())
    }
    pub fn open(&self) -> Result<file::Store> {
        if !settles(&self.root)? {
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
            && settles(&main)?
        {
            return Err(invalid(format!(
                "the main worktree already holds a store at {}; this worktree uses it, and init never creates a second store beside it",
                main.join(".axon").display()
            )));
        }
        prepare()?;
        let header = self.header();
        let result = (|| -> Result<Initialized> {
            fs::create_dir_all(directory.join(RECORDS_DIRECTORY))?;
            for (name, content) in STORE_FILES {
                let path = directory.join(name);
                if !present(&path)? {
                    let temp = file::temporary(&path, content.as_bytes())?;
                    fs::rename(&temp, &path)?;
                }
            }
            // The record directory, the ignore file and the attributes file are durable before
            // the header, which marks the store, is published.
            file::sync_directory(&directory)?;
            let bytes = encode_header(&Header::new(prefix)?)?;
            let temp = file::temporary(&header, &bytes)?;
            fs::rename(&temp, &header)?;
            file::sync_directory(&directory)?;
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
