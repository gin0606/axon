use crate::core::{ApplyOutcome, Change, Ctx, MetadataValue, StateSnapshot, StoreSnapshot};
use crate::db::{DbError, NoteMatches, Result, RevisionSnapshot, ShowSnapshot};
use crate::derived::{Evaluation, View};
use crate::domain::*;
use crate::{codec, core, db};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    rc::Rc,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    Sqlite,
    File,
}
fn invalid(message: impl std::fmt::Display) -> DbError {
    DbError::Storage(message.to_string())
}
fn present(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}
fn git(root: &Path, args: &[&str]) -> Result<Option<String>> {
    let result = Command::new("git").current_dir(root).args(args).output();
    match result {
        Ok(output) if output.status.success() => {
            Ok(Some(String::from_utf8_lossy(&output.stdout).trim().into()))
        }
        output if !root.ancestors().any(|p| p.join(".git").exists()) => {
            let _ = output;
            Ok(None)
        }
        Ok(output) => Err(DbError::GitRoot(
            String::from_utf8_lossy(&output.stderr).trim().into(),
        )),
        Err(error) => Err(DbError::GitRoot(error.to_string())),
    }
}
fn root(init: bool) -> Result<(PathBuf, bool)> {
    let cwd = std::env::current_dir()?;
    if let Some(path) = git(&cwd, &["rev-parse", "--show-toplevel"])? {
        return Ok((path.into(), true));
    }
    for path in cwd.ancestors() {
        let mut boundary = false;
        for name in ["axon.db", "state.jsonl", "init.pending"] {
            boundary |= present(&path.join(".axon").join(name))?;
        }
        if boundary {
            if init && path != cwd {
                return Err(DbError::AlreadyInitialized(path.into()));
            }
            return Ok((path.into(), false));
        }
    }
    if init {
        Ok((cwd, false))
    } else {
        Err(DbError::NotInitialized)
    }
}
pub(crate) fn merge_evaluation_root() -> Result<PathBuf> {
    match root(false) {
        Ok((root, _)) => Ok(root),
        Err(DbError::NotInitialized) => Ok(std::env::current_dir()?),
        Err(error) => Err(error),
    }
}

pub(crate) fn check_index(root: &Path, is_git: bool) -> Result<()> {
    if is_git
        && git(root, &["ls-files", "--unmerged", "--", ".axon/state.jsonl"])?
            .is_some_and(|s| !s.is_empty())
    {
        return Err(invalid(
            "state.jsonl is unmerged in the Git index; resolve and stage it before normal operations",
        ));
    }
    Ok(())
}
fn sqlite_root(root: &Path, is_git: bool) -> Result<PathBuf> {
    if !is_git {
        return Ok(root.to_path_buf());
    }
    let common = git(
        root,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?
    .ok_or_else(|| invalid("missing Git common directory"))?;
    Path::new(&common)
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| invalid("Git common directory has no parent"))
}
fn data_path(root: &Path, is_git: bool, backend: Backend) -> Result<PathBuf> {
    Ok(match backend {
        Backend::File => root.join(".axon/state.jsonl"),
        Backend::Sqlite => sqlite_root(root, is_git)?.join(".axon/axon.db"),
    })
}
pub(crate) fn discover(root: &Path, is_git: bool) -> Result<(Backend, PathBuf)> {
    let sql = data_path(root, is_git, Backend::Sqlite)?;
    let file = data_path(root, is_git, Backend::File)?;
    for marker in [
        root.join(".axon/init.pending"),
        sql.parent().unwrap().join("init.pending"),
    ] {
        if present(&marker)? {
            return Err(invalid(format!(
                "incomplete initialization at {}; inspect retained files before manual recovery",
                marker.display()
            )));
        }
    }
    match (present(&sql)?, present(&file)?) {
        (true, true) => Err(invalid(format!(
            "mixed backends at {} and {}",
            sql.display(),
            file.display()
        ))),
        (true, false) => Ok((Backend::Sqlite, sql)),
        (false, true) => Ok((Backend::File, file)),
        (false, false) => Err(DbError::NotInitialized),
    }
}

pub(crate) fn check_file_destination(root: &Path) -> Result<()> {
    let is_git = git(root, &["rev-parse", "--show-toplevel"])?.is_some();
    let sql = data_path(root, is_git, Backend::Sqlite)?;
    if present(&sql)?
        || present(&sql.parent().unwrap().join("init.pending"))?
        || present(&root.join(".axon/init.pending"))?
    {
        return Err(invalid(
            "merge destination has SQLite or incomplete initialization",
        ));
    }
    Ok(())
}

fn store_id(state: &StateSnapshot) -> Result<RecordId> {
    match state.metadata.get("store_id") {
        Some(MetadataValue::Text(id)) => id.parse().map_err(invalid),
        _ => Err(invalid("missing store ID")),
    }
}
fn boundary(path: &Path, phase: &'static str, result: &'static str, source: DbError) -> DbError {
    DbError::Boundary {
        path: path.display().to_string(),
        phase,
        result,
        source: Box::new(source),
    }
}
pub(crate) fn lock(path: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|e| boundary(path, "open writer lock", "Not applied", e.into()))?;
    file.lock()
        .map_err(|e| boundary(path, "acquire writer lock", "Not applied", e.into()))?;
    Ok(file)
}
pub(crate) fn sync_dir(path: &Path) -> Result<()> {
    File::open(path)?.sync_all()?;
    Ok(())
}
pub(crate) fn temporary(path: &Path, bytes: &[u8]) -> Result<PathBuf> {
    let temp = path.with_file_name(format!(
        ".{}-{}.tmp",
        path.file_name().unwrap().to_string_lossy(),
        RecordId::new(RecordKind::Store)
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(temp)
}
fn create(path: &Path, bytes: &[u8]) -> Result<()> {
    let temp = temporary(path, bytes)
        .map_err(|e| boundary(path, "create temporary file", "Not applied", e))?;
    fs::hard_link(&temp, path)
        .map_err(|e| boundary(path, "publish file", "Not applied", e.into()))?;
    fs::remove_file(temp)
        .map_err(DbError::from)
        .and_then(|_| sync_dir(path.parent().unwrap()))
        .map_err(|e| boundary(path, "sync published file", "Result unknown", e))
}

fn ensure_file_integration(root: &Path, applied: &mut Vec<String>) -> Result<()> {
    for (path, rules) in [
        (
            root.join(".axon/.gitignore"),
            &["*", "!.gitignore", "!state.jsonl"][..],
        ),
        (
            root.join(".gitattributes"),
            &["/.axon/state.jsonl merge=axon"][..],
        ),
    ] {
        append_rules(&path, rules)
            .map_err(|e| boundary(&path, "update Git integration", "Result unknown", e))?;
        applied.push(format!("Git integration at {}", path.display()));
    }
    Ok(())
}
fn append_rules(path: &Path, rules: &[&str]) -> Result<()> {
    let existed = present(path)?;
    if existed && !fs::symlink_metadata(path)?.file_type().is_file() {
        return Err(invalid(format!("{} is not a regular file", path.display())));
    }
    let before = if existed { fs::read(path)? } else { Vec::new() };
    let text = std::str::from_utf8(&before).map_err(invalid)?;
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
    let mut bytes = before.clone();
    for rule in rules {
        if text.lines().any(|line| {
            let line = line.trim();
            if ignore {
                line.replace("!/", "!") == *rule
            } else {
                let mut fields = line.split_whitespace();
                fields.next().is_some_and(|p| {
                    p.trim_matches('"').trim_start_matches('/') == ".axon/state.jsonl"
                }) && fields.any(|a| a == "merge=axon")
            }
        }) {
            continue;
        }
        if ignore && *rule == "*" && exception_seen {
            bytes.splice(0..0, b"*\n".iter().copied());
            continue;
        }
        if !bytes.is_empty() && !bytes.ends_with(b"\n") {
            bytes.push(b'\n');
        }
        bytes.extend_from_slice(rule.as_bytes());
        bytes.push(b'\n');
    }
    if bytes == before {
        return Ok(());
    }
    if !existed {
        return create(path, &bytes);
    }
    let temp = temporary(path, &bytes)?;
    if fs::read(path)? != before {
        return Err(invalid(format!("{} changed during init", path.display())));
    }
    fs::rename(temp, path)?;
    sync_dir(path.parent().unwrap())
}

pub enum Store {
    Sqlite(db::Store),
    File(FileStore),
}
pub struct FileStore {
    root: PathBuf,
    is_git: bool,
    path: PathBuf,
    identity: RecordId,
    evaluation: Rc<Evaluation>,
}
impl FileStore {
    fn read(&self) -> Result<(Vec<u8>, StateSnapshot)> {
        check_index(&self.root, self.is_git)?;
        if discover(&self.root, self.is_git)?.0 != Backend::File {
            return Err(invalid("backend changed; operation not applied"));
        }
        let bytes = fs::read(&self.path).map_err(|e| {
            invalid(format!(
                "{}: {e}; missing state is never regenerated",
                self.path.display()
            ))
        })?;
        let mut state = codec::decode(&bytes, self.evaluation.clone())?;
        state
            .declaration
            .entities
            .sort_by(|a, b| (a.created_at, &a.id).cmp(&(b.created_at, &b.id)));
        if store_id(&state)? != self.identity {
            return Err(invalid("state store identity changed"));
        }
        Ok((bytes, state))
    }
    fn publish(&self, before: &[u8], change: &core::ValidatedChange) -> Result<()> {
        self.publish_with(before, change, |_| Ok(()))
    }
    fn publish_with(
        &self,
        before: &[u8],
        change: &core::ValidatedChange,
        mut checkpoint: impl FnMut(&str) -> Result<()>,
    ) -> Result<()> {
        if change.outcome == ApplyOutcome::Unchanged {
            return Ok(());
        }
        checkpoint("before-write")
            .map_err(|e| boundary(&self.path, "before replace", "Not applied", e))?;
        let bytes = codec::encode(change.state())
            .map_err(|e| boundary(&self.path, "before replace", "Not applied", e.into()))?;
        let temp = temporary(&self.path, &bytes)
            .map_err(|e| boundary(&self.path, "before replace", "Not applied", e))?;
        let mut replaced = false;
        let mut publish = || -> Result<()> {
            checkpoint("after-sync")?;
            check_index(&self.root, self.is_git)?;
            if fs::read(&self.path)? != before
                || discover(&self.root, self.is_git)?.0 != Backend::File
            {
                return Err(invalid(
                    "input changed before replace; operation not applied",
                ));
            }
            fs::rename(&temp, &self.path)?;
            replaced = true;
            checkpoint("after-replace").and_then(|_| sync_dir(self.path.parent().unwrap()))
        };
        let result = publish();
        let result = result.map_err(|e| {
            boundary(
                &self.path,
                if replaced {
                    "directory sync after replace"
                } else {
                    "before replace"
                },
                if replaced {
                    "Result unknown"
                } else {
                    "Not applied"
                },
                e,
            )
        });
        if temp.exists() {
            let _ = fs::remove_file(temp);
        }
        result
    }
    fn mutate(&self, op: core::Operation, ctx: &Ctx) -> Result<core::ValidatedChange> {
        let _lock = lock(&self.root.join(".axon/write.lock"))?;
        let (bytes, before) = self.read()?;
        let result = before.execute(
            op,
            &mut core::Context {
                at: Utc::now(),
                evaluation: self.evaluation.clone(),
                actor: &ctx.actor,
                reason: ctx.reason.as_deref(),
                ids: &mut RecordId::new,
            },
        )?;
        self.publish(&bytes, &result)?;
        Ok(result)
    }
}
impl Store {
    pub fn init(prefix: Option<&str>, backend: Option<Backend>) -> Result<(PathBuf, String)> {
        Self::init_with(prefix, backend, |_| Ok(())).map_err(|e| match e {
            DbError::Boundary { .. } | DbError::Partial { .. } => {
                DbError::Initialization(Box::new(e))
            }
            other => other,
        })
    }
    fn init_with(
        prefix: Option<&str>,
        backend: Option<Backend>,
        mut checkpoint: impl FnMut(&str) -> Result<()>,
    ) -> Result<(PathBuf, String)> {
        let (root, is_git) = root(true)?;
        check_index(&root, is_git)?;
        let shared = sqlite_root(&root, is_git)?;
        let lock_root = if is_git {
            PathBuf::from(
                git(
                    &root,
                    &["rev-parse", "--path-format=absolute", "--git-common-dir"],
                )?
                .ok_or_else(|| invalid("missing Git common directory"))?,
            )
        } else {
            fs::create_dir_all(root.join(".axon"))?;
            root.join(".axon")
        };
        let _init_lock = lock(&lock_root.join("axon-init.lock"))?;
        match discover(&root, is_git) {
            Ok(_) => return Err(DbError::AlreadyInitialized(root)),
            Err(DbError::NotInitialized) => {}
            Err(e) => return Err(e),
        }
        let backend = backend.unwrap_or(Backend::Sqlite);
        let storage_root = if backend == Backend::Sqlite {
            &shared
        } else {
            &root
        };
        let directory = storage_root.join(".axon");
        fs::create_dir_all(&directory)?;
        sync_dir(storage_root)?;
        let _lock = lock(&directory.join("write.lock"))?;
        let path = data_path(&root, is_git, backend)?;
        let pending = directory.join("init.pending");
        let prefix = prefix.map(str::to_owned).unwrap_or_else(|| {
            storage_root
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        });
        if prefix.is_empty() {
            return Err(invalid("Entity prefix must not be empty"));
        }
        let mut applied = Vec::new();
        let initialization = (|| -> Result<(PathBuf, String)> {
            create(&pending, format!("{}\n", path.display()).as_bytes())?;
            applied.push(format!("initialization marker at {}", pending.display()));
            checkpoint("after-marker")?;
            checkpoint("before-state-publication")?;
            match backend {
                Backend::File => {
                    let state = StateSnapshot {
                        declaration: StoreSnapshot {
                            evaluation: Rc::new(Evaluation::new(root.clone())),
                            entities: vec![],
                            dependencies: vec![],
                        },
                        metadata: [
                            ("prefix".into(), MetadataValue::Text(prefix.clone())),
                            (
                                "store_id".into(),
                                MetadataValue::Text(RecordId::new(RecordKind::Store).to_string()),
                            ),
                        ]
                        .into(),
                        histories: Default::default(),
                        causal: Default::default(),
                    };
                    create(&path, &codec::encode(&state)?)?;
                }
                Backend::Sqlite => {
                    let temp = temporary(&path, &[])?;
                    db::Store::init_at(&temp, &prefix)?;
                    File::open(&temp)?.sync_all()?;
                    fs::hard_link(&temp, &path)?;
                    fs::remove_file(temp)?;
                    sync_dir(&directory)?;
                }
            }
            applied.push(format!("state at {}", path.display()));
            checkpoint("state-published")?;
            checkpoint("after-state")?;
            if backend == Backend::File {
                ensure_file_integration(&root, &mut applied)?;
            }
            checkpoint("before-marker-cleanup")?;
            fs::remove_file(&pending)?;
            applied.retain(|p| !p.starts_with("initialization marker"));
            checkpoint("after-marker-cleanup")?;
            sync_dir(&directory)?;
            Ok((path.clone(), prefix))
        })();
        initialization.map_err(|source| DbError::Partial {
            applied: applied.join("\nApplied: "),
            source: Box::new(boundary(
                &path,
                "initialize storage",
                "Result unknown",
                source,
            )),
        })
    }
    pub fn open(
        trace: bool,
        condition_timeout: std::time::Duration,
        report: impl FnOnce(&db::migration::Outcome),
    ) -> Result<Self> {
        let (root, is_git) = root(false)?;
        check_index(&root, is_git)?;
        let (backend, path) = discover(&root, is_git)?;
        let evaluation = Rc::new(if trace {
            Evaluation::tracing(root.clone(), condition_timeout)
        } else {
            Evaluation::with_timeout(root.clone(), condition_timeout)
        });
        match backend {
            Backend::File => {
                crate::file_upgrade::open(&path, &root, is_git, evaluation.clone())?;
                let store = FileStore {
                    root,
                    is_git,
                    path: path.clone(),
                    identity: store_id(&codec::decode(&fs::read(&path)?, evaluation.clone())?)?,
                    evaluation,
                };
                store.read()?;
                Ok(Self::File(store))
            }
            Backend::Sqlite => {
                let store = db::Store::open_at(&path, evaluation, report)?;
                Ok(Self::Sqlite(store))
            }
        }
    }
    pub fn prefix(&self) -> Result<String> {
        match self {
            Self::Sqlite(s) => s.prefix(),
            Self::File(s) => match s.read()?.1.metadata.get("prefix") {
                Some(MetadataValue::Text(p)) => Ok(p.clone()),
                _ => Err(invalid("invalid prefix")),
            },
        }
    }
    pub fn search_snapshot(&mut self, text: &str) -> Result<(View, NoteMatches)> {
        match self {
            Self::Sqlite(s) => s.search_snapshot(text),
            Self::File(s) => {
                let state = s.read()?.1;
                let mut matches = NoteMatches::new();
                for (owner, history) in &state.histories {
                    let mut ids: Vec<_> = history
                        .notes
                        .iter()
                        .filter(|note| note.body.contains(text))
                        .map(|note| note.id)
                        .collect();
                    ids.sort();
                    if !ids.is_empty() {
                        matches.insert(owner.clone(), ids);
                    }
                }
                Ok((state.declaration.view(), matches))
            }
        }
    }

    pub fn snapshot(&mut self) -> Result<StoreSnapshot> {
        match self {
            Self::Sqlite(s) => s.snapshot(),
            Self::File(s) => Ok(s.read()?.1.declaration),
        }
    }
    pub fn view(&mut self) -> Result<View> {
        match self {
            Self::Sqlite(s) => s.view(),
            Self::File(s) => Ok(s.read()?.1.declaration.view()),
        }
    }
    pub fn resolve_id(&self, input: &str) -> Result<EntityId> {
        match self {
            Self::Sqlite(s) => s.resolve_id(input),
            Self::File(s) => resolve(&s.read()?.1, input),
        }
    }
    #[cfg(test)]
    pub fn get(&self, id: &EntityId) -> Result<Entity> {
        match self {
            Self::Sqlite(s) => s.get(id),
            Self::File(s) => entity(&s.read()?.1, id),
        }
    }
    pub fn show_snapshot(&mut self, input: &str) -> Result<ShowSnapshot> {
        match self {
            Self::Sqlite(s) => s.show_snapshot(input),
            Self::File(s) => {
                let state = s.read()?.1;
                let id = resolve(&state, input)?;
                let history = state.histories.get(&id).cloned().unwrap_or_default();
                Ok(ShowSnapshot {
                    id,
                    view: state.declaration.view(),
                    counts: RecordCounts {
                        notes: history.notes.len(),
                        revisions: history.revisions.len(),
                        decisions: history.decisions.len(),
                        progressions: history.progress.len(),
                    },
                    decision_events: history.decisions,
                    progress_events: history.progress,
                    revisions: history.revisions,
                    notes: history.notes,
                    causal: state.causal,
                })
            }
        }
    }
    pub fn notes(&self, id: &EntityId) -> Result<Vec<Note>> {
        match self {
            Self::Sqlite(s) => s.notes(id),
            Self::File(s) => {
                let state = s.read()?.1;
                entity(&state, id)?;
                Ok(state.histories.get(id).cloned().unwrap_or_default().notes)
            }
        }
    }
    pub fn note(&self, id: &EntityId, input: &str) -> Result<Note> {
        if let Self::Sqlite(s) = self {
            return s.note(id, input);
        }
        let matches: Vec<_> = self
            .notes(id)?
            .into_iter()
            .filter(|n| n.id.matches(input))
            .collect();
        match matches.as_slice() {
            [note] => Ok(note.clone()),
            [] => Err(DbError::NoSuchNote {
                id: id.to_string(),
                number: input.into(),
            }),
            _ => Err(DbError::AmbiguousRecord(input.into())),
        }
    }
    pub fn events(&self, id: &EntityId) -> Result<Vec<core::Event>> {
        match self {
            Self::Sqlite(s) => s.events(id),
            Self::File(s) => {
                let state = s.read()?.1;
                entity(&state, id)?;
                Ok(state
                    .histories
                    .get(id)
                    .cloned()
                    .unwrap_or_default()
                    .decisions)
            }
        }
    }
    pub fn revision_snapshot(&mut self, id: &EntityId) -> Result<RevisionSnapshot> {
        match self {
            Self::Sqlite(s) => s.revision_snapshot(id),
            Self::File(s) => {
                let state = s.read()?.1;
                Ok(RevisionSnapshot {
                    entity: entity(&state, id)?,
                    revisions: state
                        .histories
                        .get(id)
                        .cloned()
                        .unwrap_or_default()
                        .revisions,
                })
            }
        }
    }
    #[cfg(test)]
    pub fn insert(&mut self, entity: &Entity) -> Result<()> {
        self.insert_with_dependencies(entity, Vec::new())
    }
    pub fn insert_with_dependencies(
        &mut self,
        entity: &Entity,
        dependencies: Vec<EntityId>,
    ) -> Result<()> {
        match self {
            Self::Sqlite(s) => s.insert_with_dependencies(entity, dependencies),
            Self::File(s) => {
                s.mutate(
                    core::Operation::Insert(entity.clone(), dependencies),
                    &Ctx {
                        actor: String::new(),
                        reason: None,
                    },
                )?;
                Ok(())
            }
        }
    }
    pub fn apply(&mut self, id: &EntityId, change: Change, ctx: &Ctx) -> Result<ApplyOutcome> {
        match self {
            Self::Sqlite(s) => s.apply(id, change, ctx),
            Self::File(s) => Ok(s
                .mutate(core::Operation::Change(id.clone(), change), ctx)?
                .outcome),
        }
    }
    pub fn add_note(&mut self, id: &EntityId, body: &str, actor: &str) -> Result<Note> {
        match self {
            Self::Sqlite(s) => s.add_note(id, body, actor),
            Self::File(s) => {
                let result = s.mutate(
                    core::Operation::AddNote {
                        owner: id.clone(),
                        body: body.into(),
                    },
                    &Ctx {
                        actor: actor.into(),
                        reason: None,
                    },
                )?;
                Ok(result.state().histories[id]
                    .notes
                    .last()
                    .expect("added Note")
                    .clone())
            }
        }
    }
    pub fn add_dep(&mut self, source: &EntityId, target: &EntityId) -> Result<ApplyOutcome> {
        self.dependency(source, target, true)
    }
    pub fn remove_dep(&mut self, source: &EntityId, target: &EntityId) -> Result<ApplyOutcome> {
        self.dependency(source, target, false)
    }
    fn dependency(
        &mut self,
        source: &EntityId,
        target: &EntityId,
        present: bool,
    ) -> Result<ApplyOutcome> {
        match self {
            Self::Sqlite(s) => {
                if present {
                    s.add_dep(source, target)
                } else {
                    s.remove_dep(source, target)
                }
            }
            Self::File(s) => Ok(s
                .mutate(
                    core::Operation::Dependency {
                        source: source.clone(),
                        target: target.clone(),
                        present,
                    },
                    &Ctx {
                        actor: String::new(),
                        reason: None,
                    },
                )?
                .outcome),
        }
    }
    pub fn transactional_import<T, E, F>(
        &mut self,
        build: F,
    ) -> std::result::Result<(T, StoreSnapshot), E>
    where
        E: From<DbError>,
        F: FnOnce(StoreSnapshot) -> std::result::Result<(T, StoreSnapshot), E>,
    {
        match self {
            Self::Sqlite(s) => s.transactional_import(build),
            Self::File(s) => {
                let _lock = lock(&s.root.join(".axon/write.lock"))?;
                let (bytes, before) = s.read()?;
                let (value, desired) = build(before.declaration.clone())?;
                let result = before
                    .execute(
                        core::Operation::Import(desired),
                        &mut core::Context {
                            at: Utc::now(),
                            evaluation: s.evaluation.clone(),
                            actor: "",
                            reason: None,
                            ids: &mut RecordId::new,
                        },
                    )
                    .map_err(DbError::from)?;
                s.publish(&bytes, &result)?;
                Ok((value, result.state().declaration.clone()))
            }
        }
    }
}
fn entity(state: &StateSnapshot, id: &EntityId) -> Result<Entity> {
    state
        .declaration
        .entities
        .iter()
        .find(|e| e.id == *id)
        .cloned()
        .ok_or_else(|| DbError::NoSuchEntity(id.to_string()))
}
fn resolve(state: &StateSnapshot, input: &str) -> Result<EntityId> {
    let all = &state.declaration.entities;
    let mut matches: Vec<_> = all
        .iter()
        .filter(|e| {
            e.id.as_str()
                .to_ascii_lowercase()
                .ends_with(&input.to_ascii_lowercase())
        })
        .collect();
    matches.sort_by(|a, b| a.id.cmp(&b.id));
    match matches.as_slice() {
        [e] => Ok(e.id.clone()),
        [] => Err(DbError::NoSuchEntity(input.into())),
        _ => Err(DbError::AmbiguousId {
            id: input.into(),
            candidates: matches
                .iter()
                .map(|e| e.id.to_string())
                .collect::<Vec<_>>()
                .join(", "),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture {
        root: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "axon-file-unit-{}",
                RecordId::new(RecordKind::Store)
            ));
            fs::create_dir_all(root.join(".axon")).unwrap();
            let sql = db::Store::in_memory("t").unwrap();
            let state = sql.state().unwrap();
            fs::write(
                root.join(".axon/state.jsonl"),
                codec::encode(&state).unwrap(),
            )
            .unwrap();
            Self { root }
        }
        fn store(&self) -> FileStore {
            load(&self.root)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).unwrap();
        }
    }
    fn child_command() -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("GIT_") {
                command.env_remove(key);
            }
        }
        command
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null");
        command
    }
    fn crash_child_command() -> Command {
        let mut command = child_command();
        // An intentionally interrupted process cannot produce a mergeable coverage profile.
        if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
            command.env("LLVM_PROFILE_FILE", "/dev/null");
        }
        command
    }
    fn load(root: &Path) -> FileStore {
        let evaluation = Rc::new(Evaluation::new(root.into()));
        let bytes = fs::read(root.join(".axon/state.jsonl")).unwrap();
        FileStore {
            root: root.into(),
            is_git: false,
            path: root.join(".axon/state.jsonl"),
            identity: store_id(&codec::decode(&bytes, evaluation.clone()).unwrap()).unwrap(),
            evaluation,
        }
    }
    fn change(store: &FileStore) -> (Vec<u8>, core::ValidatedChange) {
        let (bytes, state) = store.read().unwrap();
        let at = Utc::now();
        let entity = Entity {
            id: EntityId::from_stored("t-fixture"),
            kind: EntityKind::Issue,
            title: "test".into(),
            description: None,
            progress: Progress::NotStarted,
            disposition: Disposition::Undecided,
            current_revision: None,
            resurface_condition: ResurfaceCondition::Always,
            parent: None,
            created_at: at,
            updated_at: at,
        };
        (
            bytes,
            core::tests::execute(&state, core::Operation::Insert(entity, Vec::new()), 1).unwrap(),
        )
    }
    #[test]
    fn file_runs_same_core_contract_with_fixed_context_and_bytes() {
        let fixture = Fixture::new();
        let mut store = fixture.store();
        let mut sqlite = db::Store::in_memory("t").unwrap();
        let initial = sqlite.state().unwrap();
        store.identity = store_id(&initial).unwrap();
        fs::write(&store.path, codec::encode(&initial).unwrap()).unwrap();
        let mut tick = 0;
        core::tests::contract(|op| {
            tick += 1;
            let _lock = lock(&fixture.root.join(".axon/write.lock")).unwrap();
            let (bytes, before) = store.read().unwrap();
            let sql_result = sqlite.contract_step(op.clone(), tick);
            let file_result = core::tests::execute(&before, op, tick).map_err(|e| e.to_string());
            match (sql_result, file_result) {
                (Err(sql), Err(file)) => {
                    assert_eq!(sql, file);
                    assert_eq!(fs::read(&store.path).unwrap(), bytes);
                    Err(file)
                }
                (Ok((sql_outcome, sql_state)), Ok(result)) => {
                    store.publish(&bytes, &result).unwrap();
                    let (after_bytes, after) = store.read().unwrap();
                    assert_eq!(sql_outcome, result.outcome);
                    assert_eq!(
                        codec::encode(&sql_state).unwrap(),
                        codec::encode(&after).unwrap()
                    );
                    assert_eq!(
                        codec::encode(&after).unwrap(),
                        codec::encode(result.state()).unwrap()
                    );
                    if result.outcome == ApplyOutcome::Unchanged {
                        assert_eq!(bytes, after_bytes);
                    }
                    Ok((result.outcome, after))
                }
                _ => panic!("backend results differ"),
            }
        });
    }
    #[test]
    fn failure_boundaries_and_drift_do_not_partially_publish() {
        for stage in ["before-write", "after-sync", "after-replace"] {
            let fixture = Fixture::new();
            let store = fixture.store();
            let (bytes, change) = change(&store);
            let error = store
                .publish_with(&bytes, &change, |point| {
                    if point == stage {
                        Err(invalid("injected failure"))
                    } else {
                        Ok(())
                    }
                })
                .unwrap_err();
            let actual = fs::read(&store.path).unwrap();
            if stage == "after-replace" {
                assert!(error.to_string().contains("Result unknown"));
                assert_eq!(actual, codec::encode(change.state()).unwrap());
            } else {
                assert_eq!(actual, bytes);
            }
        }
        let fixture = Fixture::new();
        let store = fixture.store();
        let (bytes, change) = change(&store);
        let drift = [bytes.as_slice(), b"\n"].concat();
        let error = store
            .publish_with(&bytes, &change, |point| {
                if point == "after-sync" {
                    fs::write(&store.path, &drift)?;
                }
                Ok(())
            })
            .unwrap_err();
        assert!(error.to_string().contains("not applied"));
        assert_eq!(fs::read(&store.path).unwrap(), drift);
    }
    #[test]
    fn noop_retains_noncanonical_bytes() {
        let fixture = Fixture::new();
        let store = fixture.store();
        let (bytes, change) = change(&store);
        store.publish(&bytes, &change).unwrap();
        let bytes = [
            b"\n".as_slice(),
            fs::read(&store.path).unwrap().as_slice(),
            b"\n",
        ]
        .concat();
        fs::write(&store.path, &bytes).unwrap();
        let state = store.read().unwrap().1;
        let result = core::tests::execute(
            &state,
            core::Operation::Import(state.declaration.clone()),
            2,
        )
        .unwrap();
        assert_eq!(result.outcome, ApplyOutcome::Unchanged);
        store.publish(&bytes, &result).unwrap();
        assert_eq!(fs::read(&store.path).unwrap(), bytes);
    }
    #[test]
    #[ignore]
    fn crash_child() {
        let root = std::env::var("AXON_TEST_CRASH_ROOT").unwrap();
        let stage = std::env::var("AXON_TEST_CRASH_STAGE").unwrap();
        let store = load(Path::new(&root));
        let _lock = lock(&store.root.join(".axon/write.lock")).unwrap();
        let (bytes, change) = change(&store);
        store
            .publish_with(&bytes, &change, |point| {
                if point == stage {
                    std::process::exit(71);
                }
                Ok(())
            })
            .unwrap();
        panic!("crash point not reached");
    }
    #[test]
    fn process_exit_releases_stable_lock_and_leaves_complete_state() {
        for stage in ["after-sync", "after-replace"] {
            let fixture = Fixture::new();
            let store = fixture.store();
            let before = fs::read(&store.path).unwrap();
            let status = crash_child_command()
                .args(["--exact", "storage::tests::crash_child", "--ignored"])
                .env("AXON_TEST_CRASH_ROOT", &fixture.root)
                .env("AXON_TEST_CRASH_STAGE", stage)
                .status()
                .unwrap();
            assert_eq!(status.code(), Some(71));
            let _lock = lock(&fixture.root.join(".axon/write.lock")).unwrap();
            let (bytes, state) = store.read().unwrap();
            assert_eq!(
                state.declaration.entities.len(),
                usize::from(stage == "after-replace")
            );
            if stage == "after-sync" {
                assert_eq!(bytes, before);
            }
        }
    }
    #[test]
    #[ignore]
    fn init_crash_child() {
        let root = std::env::var("AXON_TEST_CRASH_ROOT").unwrap();
        let stage = std::env::var("AXON_TEST_CRASH_STAGE").unwrap();
        std::env::set_current_dir(root).unwrap();
        let backend = if std::env::var("AXON_TEST_BACKEND").unwrap() == "file" {
            Backend::File
        } else {
            Backend::Sqlite
        };
        let result = Store::init_with(Some("t"), Some(backend), |point| {
            if point == stage {
                std::process::exit(71);
            }
            if stage == format!("error-{point}") {
                return Err(invalid("injected initialization failure"));
            }
            Ok(())
        });
        if stage.starts_with("error-") {
            let error = result.unwrap_err().to_string();
            let (marker, state) = initialization_stage_files(&stage);
            assert_eq!(
                error.contains("Applied: initialization marker at"),
                marker,
                "{error}"
            );
            assert_eq!(error.contains("Applied: state at"), state, "{error}");
            if stage == "error-after-marker-cleanup" {
                assert!(error.contains("Result unknown: storage update"));
            }
        } else if stage == "retry" {
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("incomplete initialization")
            );
        } else {
            panic!("crash point not reached");
        }
    }
    fn initialization_stage_files(stage: &str) -> (bool, bool) {
        let marker = stage != "error-after-marker-cleanup";
        let state = !matches!(
            stage,
            "error-after-marker" | "error-before-state-publication"
        );
        (marker, state)
    }
    #[test]
    fn initialization_fault_diagnostics_match_published_stages() {
        for backend in ["file", "sqlite"] {
            for stage in [
                "error-after-marker",
                "error-before-state-publication",
                "error-state-published",
                "error-after-state",
                "error-before-marker-cleanup",
                "error-after-marker-cleanup",
            ] {
                let fixture = Fixture::new();
                fs::remove_file(fixture.root.join(".axon/state.jsonl")).unwrap();
                let status = child_command()
                    .args(["--exact", "storage::tests::init_crash_child", "--ignored"])
                    .env("AXON_TEST_CRASH_ROOT", &fixture.root)
                    .env("AXON_TEST_CRASH_STAGE", stage)
                    .env("AXON_TEST_BACKEND", backend)
                    .status()
                    .unwrap();
                assert!(status.success());
                let state = fixture.root.join(if backend == "file" {
                    ".axon/state.jsonl"
                } else {
                    ".axon/axon.db"
                });
                let (marker_expected, state_expected) = initialization_stage_files(stage);
                assert_eq!(state.is_file(), state_expected);
                assert_eq!(
                    fixture.root.join(".axon/init.pending").is_file(),
                    marker_expected
                );
            }
        }
    }

    #[test]
    fn interrupted_initialization_never_regenerates_state() {
        for backend in ["file", "sqlite"] {
            let fixture = Fixture::new();
            fs::remove_file(fixture.root.join(".axon/state.jsonl")).unwrap();
            let child = |stage: &str| {
                let mut command = if stage == "after-state" {
                    crash_child_command()
                } else {
                    child_command()
                };
                command
                    .args(["--exact", "storage::tests::init_crash_child", "--ignored"])
                    .env("AXON_TEST_CRASH_ROOT", &fixture.root)
                    .env("AXON_TEST_CRASH_STAGE", stage)
                    .env("AXON_TEST_BACKEND", backend)
                    .status()
                    .unwrap()
            };
            assert_eq!(child("after-state").code(), Some(71));
            let path = fixture.root.join(if backend == "file" {
                ".axon/state.jsonl"
            } else {
                ".axon/axon.db"
            });
            let bytes = fs::read(&path).unwrap();
            assert!(!fixture.root.join(".axon/config.json").exists());
            assert!(child("retry").success());
            assert_eq!(fs::read(path).unwrap(), bytes);
        }
    }
}
