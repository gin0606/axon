//! Storage discovery keeps Git boundaries and never falls back from an artifact.
use crate::{
    error::{Result, invalid, validate_prefix},
    file,
    lifecycle::Snapshot,
};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
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
pub(crate) fn present(path: &Path) -> Result<bool> {
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
/// A canonical snapshot or an initialization marker settles discovery; an empty `.axon` or a
/// lock alone does not.
fn settles(root: &Path) -> Result<bool> {
    Ok(present(&root.join(".axon/state.jsonl"))? || present(&root.join(".axon/init.pending"))?)
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
            if !init
                && git.linked
                && !settles(&root)?
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
    pub(crate) fn state(&self) -> PathBuf {
        self.root.join(".axon/state.jsonl")
    }
    /// Whether the canonical snapshot exists; an incomplete initialization is an error.
    pub fn initialized(&self) -> Result<bool> {
        let marker = self.root.join(".axon/init.pending");
        if present(&marker)? {
            return Err(invalid(format!(
                "incomplete initialization at {}; preserve retained artifacts before manual recovery",
                marker.display()
            )));
        }
        present(&self.state())
    }
    pub fn check_index(&self) -> Result<()> {
        if self.git.is_some() {
            let out = git_command(&self.root)
                .args(["ls-files", "--unmerged", "--", ".axon/state.jsonl"])
                .output()?;
            if !out.status.success() {
                return Err(invalid("cannot inspect Git index"));
            }
            if !out.stdout.is_empty() {
                return Err(invalid(
                    "unmerged Git index for .axon/state.jsonl; validate resolution and stage it before normal operations",
                ));
            }
        }
        Ok(())
    }
    pub fn open(&self) -> Result<file::Store> {
        if !self.initialized()? {
            return Err(invalid("not initialized; run axon init"));
        }
        file::Store::at(self.clone())
    }
    pub fn init(&self, prefix: &str) -> Result<Initialized> {
        validate_prefix(prefix)?;
        let destination = self.state();
        let directory = destination.parent().unwrap();
        let prepare = || -> Result<()> {
            fs::create_dir_all(directory)?;
            if !fs::symlink_metadata(directory)?.file_type().is_dir() {
                return Err(invalid("management directory is not a regular directory"));
            }
            Ok(())
        };
        // Inside Git the lock lives in the common directory, so a rejected init leaves no
        // `.axon` behind in this worktree.
        let lock_root = match &self.git {
            Some(git) => &git.common,
            None => {
                prepare()?;
                directory
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
        if self.initialized()? {
            return Err(invalid(
                "already initialized; init never repairs or replaces an existing store",
            ));
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
                "the main worktree already holds a store or an incomplete initialization at {}; this worktree uses it, and init never creates a second store beside it",
                main.join(".axon").display()
            )));
        }
        prepare()?;
        let pending = directory.join("init.pending");
        let temp = directory.join(format!(".axon-{:032x}.tmp", rand::random::<u128>()));
        let result = (|| -> Result<Initialized> {
            let mut marker = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&pending)?;
            writeln!(marker, "{}", temp.display())?;
            marker.sync_all()?;
            File::open(directory)?.sync_all()?;
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)?;
            output.write_all(&file::encode(prefix, &Snapshot::empty())?)?;
            output.sync_all()?;
            fs::hard_link(&temp, &destination)?;
            File::open(directory)?.sync_all()?;
            fs::remove_file(&temp)?;
            fs::remove_file(&pending)?;
            File::open(directory)?.sync_all()?;
            Ok(Initialized {
                git: self.git.is_some(),
                linked_worktree,
            })
        })();
        result.map_err(|e| invalid(format!("initialization failed at {}: {e}; result may be partial; retain {}, {} and {} for inspection", directory.display(), pending.display(), temp.display(), destination.display())))
    }
}
