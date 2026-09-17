//! Storage discovery keeps Git boundaries and never falls back from an artifact.
use crate::{
    file,
    lifecycle::Snapshot,
    sqlite::{self, Result, invalid},
};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Clone)]
pub struct Location {
    pub root: PathBuf,
    pub sqlite: PathBuf,
    common: Option<PathBuf>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntegrationChange {
    Created,
    Appended,
    Unchanged,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IntegrationFile {
    pub path: PathBuf,
    pub change: IntegrationChange,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InitResult {
    Sqlite,
    File(Vec<IntegrationFile>),
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
    fn artifacts(&self) -> Result<Option<bool>> {
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
            (_, true) => Ok(Some(true)),
            (true, false) => Ok(Some(false)),
            _ => Ok(None),
        }
    }
    pub fn is_file(&self) -> Result<bool> {
        Ok(self.artifacts()? == Some(true))
    }
    pub fn check_index(&self) -> Result<()> {
        if self.common.is_some() {
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
    pub fn open(&self) -> Result<Store> {
        match self.artifacts()? {
            Some(true) => Ok(Store::File(file::Store::open(&self.root)?)),
            Some(false) => {
                if !fs::symlink_metadata(&self.sqlite)?.file_type().is_file() {
                    return Err(invalid("SQLite canonical path is not a regular file"));
                }
                Ok(Store::Sqlite(sqlite::Store::open(&self.sqlite)?))
            }
            None => Err(invalid("not initialized; run axon init")),
        }
    }
    pub fn init_backend(&self, prefix: &str, file_backend: bool) -> Result<InitResult> {
        sqlite::validate_prefix(prefix)?;
        let destination = if file_backend {
            self.root.join(".axon/state.jsonl")
        } else {
            self.sqlite.clone()
        };
        let directory = destination.parent().unwrap();
        fs::create_dir_all(directory)?;
        if file_backend && !fs::symlink_metadata(directory)?.file_type().is_dir() {
            return Err(invalid(
                "file management directory is not a regular directory",
            ));
        }
        let lock_root = self.common.as_deref().unwrap_or(directory);
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_root.join("axon-init.lock"))?;
        lock.lock()?;
        self.check_index()?;
        if self.artifacts()?.is_some() {
            return Err(invalid(
                "already initialized; init never repairs or replaces an existing store",
            ));
        }
        let pending = directory.join("init.pending");
        let temp = directory.join(format!(".axon-{:032x}.tmp", rand::random::<u128>()));
        let result = (|| -> Result<InitResult> {
            let mut marker = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&pending)?;
            writeln!(marker, "{}", temp.display())?;
            marker.sync_all()?;
            File::open(directory)?.sync_all()?;
            if file_backend {
                let mut output = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&temp)?;
                output.write_all(&file::encode(prefix, &sqlite::empty())?)?;
            } else {
                drop(sqlite::Store::create(&temp, prefix, &sqlite::empty())?);
            }
            File::open(&temp)?.sync_all()?;
            fs::hard_link(&temp, &destination)?;
            File::open(directory)?.sync_all()?;
            fs::remove_file(&temp)?;
            let result = if file_backend {
                let integration = ensure_file_integration(&self.root)?;
                if self.common.is_some() {
                    let attributes = git_command(&self.root)
                        .args(["check-attr", "-z", "merge", "--", ".axon/state.jsonl"])
                        .output()?;
                    if !attributes.status.success()
                        || attributes.stdout != b".axon/state.jsonl\0merge\0axon\0"
                    {
                        return Err(invalid(
                            "effective Git merge attribute conflicts with merge=axon; inspect .axon/.gitattributes and Git info/attributes",
                        ));
                    }
                }
                InitResult::File(integration)
            } else {
                InitResult::Sqlite
            };
            fs::remove_file(&pending)?;
            File::open(directory)?.sync_all()?;
            Ok(result)
        })();
        result.map_err(|e| invalid(format!("initialization failed at {}: {e}; result may be partial; retain {}, {} and {} for inspection", directory.display(), pending.display(), temp.display(), destination.display())))
    }
}

pub enum Store {
    Sqlite(sqlite::Store),
    File(file::Store),
}
impl Store {
    pub fn read(&self) -> Result<(String, Snapshot)> {
        match self {
            Self::Sqlite(s) => s.read(),
            Self::File(s) => s.read(),
        }
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
        match self {
            Self::Sqlite(s) => s.update_with(change, before_publish),
            Self::File(s) => s.update_with(change, before_publish),
        }
    }
}
fn ensure_file_integration(root: &Path) -> Result<Vec<IntegrationFile>> {
    [
        (
            root.join(".axon/.gitignore"),
            &["*", "!.gitignore", "!state.jsonl"][..],
        ),
        (
            root.join(".gitattributes"),
            &["/.axon/state.jsonl merge=axon"][..],
        ),
    ]
    .into_iter()
    .map(|(path, rules)| {
        let change = append_rules(&path, rules)?;
        Ok(IntegrationFile { path, change })
    })
    .collect()
}
fn append_rules(path: &Path, rules: &[&str]) -> Result<IntegrationChange> {
    let existed = present(path)?;
    if existed && !fs::symlink_metadata(path)?.file_type().is_file() {
        return Err(invalid(format!("{} is not a regular file", path.display())));
    }
    let before = if existed { fs::read(path)? } else { Vec::new() };
    let text = std::str::from_utf8(&before).map_err(|e| invalid(e.to_string()))?;
    let ignore = path.file_name().is_some_and(|name| name == ".gitignore");
    let mut exception_seen = false;
    for line in text.lines().map(str::trim).filter(|s| !s.starts_with('#')) {
        let conflict = if ignore {
            ["state.jsonl", "/state.jsonl", ".gitignore", "/.gitignore"].contains(&line)
                || (line == "*" && exception_seen)
        } else {
            let mut fields = line.split_whitespace();
            fields.next().is_some_and(|p| {
                [
                    "/.axon/state.jsonl",
                    ".axon/state.jsonl",
                    "\"/.axon/state.jsonl\"",
                    "\".axon/state.jsonl\"",
                ]
                .contains(&p)
            }) && fields.any(|a| {
                a == "merge"
                    || a == "-merge"
                    || a == "!merge"
                    || (a.starts_with("merge=") && a != "merge=axon")
            })
        };
        if conflict {
            return Err(invalid(format!(
                "conflicting rule in {}: {line}",
                path.display()
            )));
        }
        exception_seen |= [
            "!state.jsonl",
            "!/state.jsonl",
            "!.gitignore",
            "!/.gitignore",
        ]
        .contains(&line);
    }
    // Put the required exceptions/attribute last: an earlier occurrence can
    // be shadowed by a later broad rule. Preserve unrelated rules verbatim.
    let trailing: Vec<_> = rules.iter().filter(|rule| **rule != "*").collect();
    let mut bytes = Vec::new();
    for line in text.split_inclusive('\n') {
        if !trailing.iter().any(|rule| line.trim() == **rule) {
            bytes.extend_from_slice(line.as_bytes());
        }
    }
    if ignore && !text.lines().any(|line| line.trim() == "*") {
        bytes.splice(0..0, b"*\n".iter().copied());
    }
    for rule in trailing {
        if !bytes.is_empty() && !bytes.ends_with(b"\n") {
            bytes.push(b'\n');
        }
        bytes.extend_from_slice(rule.as_bytes());
        bytes.push(b'\n');
    }
    if bytes == before {
        return Ok(IntegrationChange::Unchanged);
    }
    if !existed {
        file::publish(path, None, &bytes, || Ok(()))?;
        return Ok(IntegrationChange::Created);
    }
    file::publish(path, Some(&before), &bytes, || Ok(()))?;
    Ok(IntegrationChange::Appended)
}
