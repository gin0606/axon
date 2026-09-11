//! Storage discovery keeps Git boundaries and never falls back from an artifact.
use crate::sqlite::{self, Result, Store, invalid};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};

pub struct Location {
    pub root: PathBuf,
    pub sqlite: PathBuf,
    common: Option<PathBuf>,
}
fn present(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}
fn git_command(cwd: &Path) -> Command {
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
fn git(cwd: &Path, args: &[&str]) -> Result<Option<PathBuf>> {
    let mut boundary = None;
    for ancestor in cwd.ancestors() {
        if present(&ancestor.join(".git"))? {
            boundary = Some(ancestor);
            break;
        }
    }
    let result = git_command(cwd).args(args).output();
    match result {
        Ok(output) if output.status.success() => {
            let text =
                String::from_utf8(output.stdout).map_err(|_| invalid("Git path is not UTF-8"))?;
            let path = PathBuf::from(text.strip_suffix('\n').unwrap_or(&text));
            if args.contains(&"--show-toplevel")
                && let Some(boundary) = boundary
                && fs::canonicalize(&path)? != fs::canonicalize(boundary)?
            {
                return Err(invalid(format!(
                    "Git discovery failed: Git skipped the repository marker at {}",
                    boundary.join(".git").display()
                )));
            }
            Ok(Some(path))
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
impl Location {
    pub fn discover(cwd: &Path, init: bool) -> Result<Self> {
        let cwd = fs::canonicalize(cwd)?;
        if let Some(root) = git(&cwd, &["rev-parse", "--show-toplevel"])? {
            let common = git(
                &root,
                &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            )?
            .ok_or_else(|| invalid("missing Git common directory"))?;
            let sql_root = common
                .parent()
                .ok_or_else(|| invalid("Git common directory has no parent"))?;
            return Ok(Self {
                root,
                sqlite: sql_root.join(".axon/axon.db"),
                common: Some(common),
            });
        }
        for root in cwd.ancestors() {
            let mut boundary = false;
            for name in ["axon.db", "state.jsonl", "init.pending"] {
                boundary |= present(&root.join(".axon").join(name))?;
            }
            if boundary {
                if init && root != cwd {
                    return Err(invalid(format!(
                        "already inside a management root: {}",
                        root.display()
                    )));
                }
                return Ok(Self {
                    root: root.into(),
                    sqlite: root.join(".axon/axon.db"),
                    common: None,
                });
            }
        }
        if init {
            Ok(Self {
                root: cwd.clone(),
                sqlite: cwd.join(".axon/axon.db"),
                common: None,
            })
        } else {
            Err(invalid(
                "not initialized; run axon init in the intended management root",
            ))
        }
    }
    fn artifacts(&self) -> Result<bool> {
        for marker in [
            self.root.join(".axon/init.pending"),
            self.sqlite.parent().unwrap().join("init.pending"),
        ] {
            if present(&marker)? {
                return Err(invalid(format!(
                    "incomplete initialization at {}; preserve retained artifacts before manual recovery",
                    marker.display()
                )));
            }
        }
        let sql = present(&self.sqlite)?;
        let file = present(&self.root.join(".axon/state.jsonl"))?;
        match (sql, file) {
            (true, true) => Err(invalid("mixed SQLite and file backends")),
            (_, true) => Err(invalid(
                "file backend is not supported by this SQLite milestone; no files were changed",
            )),
            _ => Ok(sql),
        }
    }
    pub fn open(&self) -> Result<Store> {
        if !self.artifacts()? {
            return Err(invalid("not initialized; run axon init"));
        }
        if !fs::symlink_metadata(&self.sqlite)?.file_type().is_file() {
            return Err(invalid("SQLite canonical path is not a regular file"));
        }
        Store::open(&self.sqlite)
    }
    pub fn init(&self, prefix: &str) -> Result<()> {
        sqlite::validate_prefix(prefix)?;
        let directory = self.sqlite.parent().unwrap();
        fs::create_dir_all(directory)?;
        let lock_root = self.common.as_deref().unwrap_or(directory);
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_root.join("axon-init.lock"))?;
        lock.lock()?;
        if self.artifacts()? {
            return Err(invalid(
                "already initialized; init never repairs or replaces an existing store",
            ));
        }
        let pending = directory.join("init.pending");
        let temp = directory.join(format!(".axon-{:032x}.tmp", rand::random::<u128>()));
        let result = (|| -> Result<()> {
            let mut marker = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&pending)?;
            writeln!(marker, "{}", temp.display())?;
            marker.sync_all()?;
            File::open(directory)?.sync_all()?;
            drop(Store::create(&temp, prefix, &sqlite::empty())?);
            File::open(&temp)?.sync_all()?;
            fs::hard_link(&temp, &self.sqlite)?;
            File::open(directory)?.sync_all()?;
            fs::remove_file(&temp)?;
            fs::remove_file(&pending)?;
            File::open(directory)?.sync_all()?;
            Ok(())
        })();
        result.map_err(|e| invalid(format!("initialization failed at {}: {e}; result may be partial; retain {}, {} and {} for inspection", directory.display(), pending.display(), temp.display(), self.sqlite.display())))
    }
}
