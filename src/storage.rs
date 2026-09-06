use crate::core::{ApplyOutcome, Change, Ctx, MetadataValue, StateSnapshot, StoreSnapshot};
use crate::db::{DbError, Result, RevisionSnapshot, ShowSnapshot};
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
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    schema: u32,
    backend: Backend,
    store_id: RecordId,
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
        for name in [
            "config.json",
            "axon.db",
            "state.jsonl",
            "state.db",
            "init.pending",
        ] {
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

pub(crate) fn file_config_identity(path: &Path) -> Result<RecordId> {
    let (config, _) = config(path)?;
    if config.backend != Backend::File {
        return Err(invalid(
            "destination configuration must select file backend",
        ));
    }
    Ok(config.store_id)
}

fn check_index(root: &Path, is_git: bool) -> Result<()> {
    if is_git
        && git(
            root,
            &[
                "ls-files",
                "--unmerged",
                "--",
                ".axon/config.json",
                ".axon/state.jsonl",
            ],
        )?
        .is_some_and(|s| !s.is_empty())
    {
        return Err(invalid(
            "config.json or state.jsonl is unmerged in the Git index; resolve and stage it before normal operations",
        ));
    }
    Ok(())
}
fn data_path(root: &Path, is_git: bool, backend: Backend) -> Result<PathBuf> {
    if backend == Backend::File {
        return Ok(root.join(".axon/state.jsonl"));
    }
    if is_git {
        let common = git(
            root,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )?
        .ok_or_else(|| invalid("missing Git common directory"))?;
        Ok(PathBuf::from(common).join("axon/state.db"))
    } else {
        Ok(root.join(".axon/state.db"))
    }
}
fn config(path: &Path) -> Result<(Config, Vec<u8>)> {
    let bytes = fs::read(path).map_err(|e| invalid(format!("not initialized or incomplete configuration at {}: {e}; preserve any legacy .axon/axon.db and use manual migration; no fallback is performed", path.display())))?;
    let config: Config = serde_json::from_slice(&bytes).map_err(invalid)?;
    if config.schema != 1 || config.store_id.kind() != RecordKind::Store {
        return Err(invalid(
            "unsupported configuration schema or invalid store ID",
        ));
    }
    Ok((config, bytes))
}
fn store_id(state: &StateSnapshot) -> Result<RecordId> {
    match state.metadata.get("store_id") {
        Some(MetadataValue::Text(id)) => id.parse().map_err(invalid),
        _ => Err(invalid("missing store ID")),
    }
}
fn lock(path: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    file.lock()?;
    Ok(file)
}
fn sync_dir(path: &Path) -> Result<()> {
    File::open(path)?.sync_all()?;
    Ok(())
}
fn temporary(path: &Path, bytes: &[u8]) -> Result<PathBuf> {
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
    let temp = temporary(path, bytes)?;
    fs::hard_link(&temp, path)?;
    fs::remove_file(temp)
        .map_err(DbError::from)
        .and_then(|_| sync_dir(path.parent().unwrap()))
        .map_err(|e| {
            invalid(format!(
                "result unknown after publishing {}: {e}; inspect before retry",
                path.display()
            ))
        })
}

pub enum Store {
    Sqlite(db::Store),
    File(FileStore),
}
pub struct FileStore {
    root: PathBuf,
    is_git: bool,
    path: PathBuf,
    config_bytes: Vec<u8>,
    config: Config,
    evaluation: Rc<Evaluation>,
}
impl FileStore {
    fn read(&self) -> Result<(Vec<u8>, StateSnapshot)> {
        check_index(&self.root, self.is_git)?;
        if fs::read(self.root.join(".axon/config.json"))? != self.config_bytes {
            return Err(invalid("configuration changed; operation not applied"));
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
        if store_id(&state)? != self.config.store_id {
            return Err(invalid("configuration and state store IDs differ"));
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
        checkpoint("before-write")?;
        let bytes = codec::encode(change.state())
            .map_err(|e| invalid(format!("operation not applied before replace: {e}")))?;
        let temp = temporary(&self.path, &bytes)
            .map_err(|e| invalid(format!("operation not applied before replace: {e}")))?;
        let mut replaced = false;
        let mut publish = || -> Result<()> {
            checkpoint("after-sync")?;
            check_index(&self.root, self.is_git)?;
            if fs::read(&self.path)? != before
                || fs::read(self.root.join(".axon/config.json"))? != self.config_bytes
            {
                return Err(invalid(
                    "input changed before replace; operation not applied",
                ));
            }
            fs::rename(&temp, &self.path)?;
            replaced = true;
            checkpoint("after-replace")
                .and_then(|_| sync_dir(self.path.parent().unwrap()))
                .map_err(|e| {
                    invalid(format!(
                        "result unknown after replace: {e}; inspect state before retry"
                    ))
                })
        };
        let result = publish();
        let result = if replaced {
            result
        } else {
            result.map_err(|e| invalid(format!("operation not applied before replace: {e}")))
        };
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
        Self::init_with(prefix, backend, |_| Ok(()))
    }
    fn init_with(
        prefix: Option<&str>,
        backend: Option<Backend>,
        mut checkpoint: impl FnMut(&str) -> Result<()>,
    ) -> Result<(PathBuf, String)> {
        let (root, is_git) = root(true)?;
        check_index(&root, is_git)?;
        let directory = root.join(".axon");
        fs::create_dir_all(&directory)?;
        sync_dir(&root)?;
        let _lock = lock(&directory.join("write.lock"))?;
        let config_path = directory.join("config.json");
        if present(&config_path)? {
            let (cfg, _) = config(&config_path)?;
            if backend.is_some_and(|b| b != cfg.backend) {
                return Err(invalid("init cannot change an existing backend"));
            }
            let mut existing = Self::open(false, |_| {})?;
            let actual_prefix = existing.prefix()?;
            if prefix.is_some_and(|p| p != actual_prefix) {
                return Err(invalid("init cannot change an existing prefix"));
            }
            let _ = existing.snapshot()?;
            return Ok((data_path(&root, is_git, cfg.backend)?, actual_prefix));
        }
        let pending = directory.join("init.pending");
        if present(&pending)? {
            return Err(invalid(format!(
                "incomplete initialization at {}; preserve the pending marker and state, restore a matching config before retry",
                pending.display()
            )));
        }
        if directory.join("axon.db").exists() {
            return Err(invalid(format!(
                "legacy database at {}; preserve it and use manual migration",
                directory.join("axon.db").display()
            )));
        }
        let backend = backend.unwrap_or(Backend::Sqlite);
        let path = data_path(&root, is_git, backend)?;
        if directory.join("state.jsonl").exists() || directory.join("state.db").exists() {
            return Err(invalid(format!(
                "incomplete initialization at {}; preserve state and restore a matching config.json; no empty state was generated",
                directory.display()
            )));
        }
        let requested_prefix = prefix;
        let prefix = prefix.map(str::to_owned).unwrap_or_else(|| {
            root.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        });
        let evaluation = Rc::new(Evaluation::new(root.clone()));
        fs::create_dir_all(path.parent().unwrap())?;
        sync_dir(path.parent().unwrap().parent().unwrap())?;
        let _shared_lock = if backend == Backend::Sqlite && is_git {
            Some(lock(&path.with_extension("lock"))?)
        } else {
            None
        };
        let existing = if backend == Backend::Sqlite && present(&path)? {
            let state = db::Store::open_at(&path, evaluation.clone(), |_| {})?.state()?;
            if requested_prefix.is_some_and(|p| !matches!(state.metadata.get("prefix"),Some(MetadataValue::Text(actual)) if actual == p)) {
                return Err(invalid("init prefix differs from existing shared SQLite; no configuration was published"));
            }
            Some(state)
        } else {
            None
        };
        let new_state = existing.is_none();
        let state = if let Some(state) = existing {
            state
        } else {
            if prefix.is_empty() {
                return Err(invalid("Entity prefix must not be empty"));
            }
            create(&pending, format!("{}\n", path.display()).as_bytes())?;
            if path.exists() {
                return Err(invalid(format!(
                    "incomplete initialization at {}",
                    path.display()
                )));
            }
            match backend {
                Backend::File => {
                    let state = StateSnapshot {
                        declaration: StoreSnapshot {
                            evaluation,
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
                    state
                }
                Backend::Sqlite => {
                    let temp = temporary(&path, &[])?;
                    db::Store::init_at(&temp, &prefix)?;
                    File::open(&temp)?.sync_all()?;
                    fs::hard_link(&temp, &path)?;
                    fs::remove_file(temp)?;
                    sync_dir(path.parent().unwrap())?;
                    db::Store::open_at(&path, evaluation, |_| {})?.state()?
                }
            }
        };
        if requested_prefix.is_some_and(|p| !matches!(state.metadata.get("prefix"),Some(MetadataValue::Text(actual)) if actual == p)) {
            return Err(invalid("init prefix differs from existing shared SQLite; no configuration was published"));
        }
        checkpoint("after-state")?;
        let cfg = Config {
            schema: 1,
            backend,
            store_id: store_id(&state)?,
        };
        let mut bytes = serde_json::to_vec_pretty(&cfg).map_err(invalid)?;
        bytes.push(b'\n');
        create(&config_path, &bytes)?;
        checkpoint("after-config")?;
        if new_state {
            fs::remove_file(&pending).and_then(|_| File::open(&directory)?.sync_all()).map_err(|e| invalid(format!("initialization applied but cleanup result unknown at {}: {e}; inspect config/state before retry", directory.display())))?;
        }
        let actual_prefix = match &state.metadata["prefix"] {
            MetadataValue::Text(p) => p.clone(),
            _ => return Err(invalid("invalid prefix")),
        };
        Ok((path, actual_prefix))
    }
    pub fn open(trace: bool, report: impl FnOnce(&db::migration::Outcome)) -> Result<Self> {
        let (root, is_git) = root(false)?;
        check_index(&root, is_git)?;
        let (cfg, config_bytes) = config(&root.join(".axon/config.json"))?;
        let path = data_path(&root, is_git, cfg.backend)?;
        if !path.is_file() {
            return Err(invalid(format!(
                "missing state at {}; preserve existing files and restore matching state/config; no fallback",
                path.display()
            )));
        }
        let evaluation = Rc::new(if trace {
            Evaluation::tracing(root.clone())
        } else {
            Evaluation::new(root.clone())
        });
        match cfg.backend {
            Backend::File => {
                let store = FileStore {
                    root,
                    is_git,
                    path,
                    config_bytes,
                    config: cfg,
                    evaluation,
                };
                store.read()?;
                Ok(Self::File(store))
            }
            Backend::Sqlite => {
                let store = db::Store::open_at(&path, evaluation, report)?;
                if store_id(&store.state()?)? != cfg.store_id {
                    return Err(invalid("configuration and SQLite store IDs differ"));
                }
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
    pub fn get(&self, id: &EntityId) -> Result<Entity> {
        match self {
            Self::Sqlite(s) => s.get(id),
            Self::File(s) => entity(&s.read()?.1, id),
        }
    }
    pub fn status_snapshot(&mut self, group: Option<&str>) -> Result<(Option<EntityId>, View)> {
        match self {
            Self::Sqlite(s) => s.status_snapshot(group),
            Self::File(s) => {
                let state = s.read()?.1;
                let id = group.map(|g| resolve(&state, g)).transpose()?;
                if let Some(id) = &id
                    && entity(&state, id)?.kind != EntityKind::Group
                {
                    return Err(DbError::NotGroup(id.to_string()));
                }
                Ok((id, state.declaration.view()))
            }
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
    pub fn insert(&mut self, entity: &Entity) -> Result<()> {
        match self {
            Self::Sqlite(s) => s.insert(entity),
            Self::File(s) => {
                s.mutate(
                    core::Operation::Insert(entity.clone()),
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
            fs::write(
                root.join(".axon/config.json"),
                serde_json::to_vec(&Config {
                    schema: 1,
                    backend: Backend::File,
                    store_id: store_id(&state).unwrap(),
                })
                .unwrap(),
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
    fn load(root: &Path) -> FileStore {
        let (config, config_bytes) = config(&root.join(".axon/config.json")).unwrap();
        FileStore {
            root: root.into(),
            is_git: false,
            path: root.join(".axon/state.jsonl"),
            config,
            config_bytes,
            evaluation: Rc::new(Evaluation::new(root.into())),
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
            core::tests::execute(&state, core::Operation::Insert(entity), 1).unwrap(),
        )
    }
    #[test]
    fn file_runs_same_core_contract_with_fixed_context_and_bytes() {
        let fixture = Fixture::new();
        let mut store = fixture.store();
        let mut sqlite = db::Store::in_memory("t").unwrap();
        let initial = sqlite.state().unwrap();
        store.config.store_id = store_id(&initial).unwrap();
        store.config_bytes = serde_json::to_vec(&store.config).unwrap();
        fs::write(fixture.root.join(".axon/config.json"), &store.config_bytes).unwrap();
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
                assert!(error.to_string().contains("result unknown"));
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
            let status = child_command()
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
            Ok(())
        });
        if stage == "retry" {
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
    #[test]
    fn interrupted_initialization_never_regenerates_state() {
        for backend in ["file", "sqlite"] {
            let fixture = Fixture::new();
            fs::remove_file(fixture.root.join(".axon/config.json")).unwrap();
            fs::remove_file(fixture.root.join(".axon/state.jsonl")).unwrap();
            let child = |stage: &str| {
                child_command()
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
                ".axon/state.db"
            });
            let bytes = fs::read(&path).unwrap();
            assert!(!fixture.root.join(".axon/config.json").exists());
            assert!(child("retry").success());
            assert_eq!(fs::read(path).unwrap(), bytes);
        }
    }
}
