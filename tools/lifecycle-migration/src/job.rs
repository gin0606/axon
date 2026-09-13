use crate::{
    Result,
    convert::{self, Rules},
    legacy::Legacy,
    require, sql, util,
};
use axon::{file, lifecycle::Snapshot, location::Location, sqlite};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    File,
    Sqlite,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Input {
    Legacy,
    Current,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub name: String,
    pub root: PathBuf,
    pub source: PathBuf,
    pub backend: Backend,
    pub input: Input,
    #[serde(default)]
    pub rules: Rules,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub version: u32,
    pub axon_binary: PathBuf,
    #[serde(default)]
    pub legacy_binary: Option<PathBuf>,
    pub targets: Vec<Target>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    version: u32,
    id: String,
    at: DateTime<Utc>,
    config: Config,
    binaries: BTreeMap<PathBuf, String>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Prepared {
    plan_digest: String,
    source_digest: String,
    candidate_digest: String,
    topology: BTreeMap<String, String>,
    artifacts: BTreeMap<String, String>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Checked {
    plan_digest: String,
    targets: Vec<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceProof {
    plan_digest: String,
    bytes_digest: String,
    logical_digest: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    ApplyIntent,
    Applied,
    RestoreIntent,
    Restored,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Event {
    phase: Phase,
    at: DateTime<Utc>,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    events: Vec<Event>,
}
#[derive(Debug, Serialize)]
pub struct Outcome {
    pub name: String,
    pub status: String,
}

fn temporary(directory: &Path) -> PathBuf {
    directory.join(format!(
        ".migration-{}.tmp",
        axon::lifecycle::RecordId::generate()
    ))
}
fn atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().ok_or("missing parent")?;
    let temp = temporary(parent);
    util::save_new(&temp, bytes)?;
    fs::rename(temp, path)?;
    util::sync_dir(parent)
}
fn save<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    atomic(path, &serde_json::to_vec_pretty(value)?)
}
fn git(root: &Path, args: &[&str]) -> Result<Option<String>> {
    let mut command = Command::new("git");
    command.current_dir(root).args(args);
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    let output = command.output()?;
    if output.status.success() {
        Ok(Some(String::from_utf8(output.stdout)?))
    } else {
        Ok(None)
    }
}
fn guard(target: &Target) -> Result<BTreeMap<String, String>> {
    util::absolute(&target.root)?;
    util::absolute(&target.source)?;
    let directory = target
        .source
        .parent()
        .ok_or("missing management directory")?;
    require(
        fs::symlink_metadata(directory)?.file_type().is_dir(),
        "management directory is not a regular directory",
    )?;
    let location = Location::discover(&target.root, false)?;
    require(location.root == target.root, "management root changed")?;
    require(
        location.is_file()? == (target.backend == Backend::File),
        "backend changed",
    )?;
    let source = if target.backend == Backend::File {
        target.root.join(".axon/state.jsonl")
    } else {
        location.sqlite
    };
    require(
        source == target.source,
        "source does not match discovered canonical storage",
    )?;
    Location::discover(&target.root, false)?.check_index()?;
    let mut topology = BTreeMap::new();
    if let Some(common) = git(
        &target.root,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )? {
        topology.insert("common".into(), common);
        topology.insert(
            "index".into(),
            git(
                &target.root,
                &["ls-files", "--stage", "--", ".axon/state.jsonl"],
            )?
            .ok_or("Git index unavailable")?,
        );
    }
    Ok(topology)
}
fn locks(target: &Target) -> Result<Vec<File>> {
    let directory = target.source.parent().ok_or("missing parent")?;
    // Old file writers and new file writers use different stable lock names.
    ["write.lock", "state.lock"]
        .iter()
        .map(|name| util::lock(&directory.join(name)))
        .collect()
}
fn current(backend: Backend, path: &Path) -> Result<(String, Snapshot)> {
    match backend {
        Backend::File => Ok(file::decode(&util::read(path)?)?),
        Backend::Sqlite => Ok(sqlite::Store::open(path)?.read()?),
    }
}
fn canonical(backend: Backend, path: &Path) -> Result<Vec<u8>> {
    let (prefix, snapshot) = current(backend, path)?;
    Ok(file::encode(&prefix, &snapshot)?)
}
fn source_digest(target: &Target, path: &Path) -> Result<String> {
    match target.backend {
        Backend::File => Ok(util::digest(&util::read(path)?)),
        Backend::Sqlite if target.input == Input::Legacy => sql::dump(path)?.digest(),
        Backend::Sqlite => Ok(util::digest(&canonical(target.backend, path)?)),
    }
}
fn candidate_digest(backend: Backend, path: &Path) -> Result<String> {
    match backend {
        Backend::File => Ok(util::digest(&util::read(path)?)),
        Backend::Sqlite => Ok(util::digest(&canonical(backend, path)?)),
    }
}
fn copy_snapshot(backend: Backend, source: &Path, destination: &Path) -> Result<()> {
    let temp = temporary(destination.parent().ok_or("missing parent")?);
    match backend {
        Backend::File => util::save_new(&temp, &util::read(source)?)?,
        Backend::Sqlite => sql::backup(source, &temp)?,
    }
    fs::rename(temp, destination)?;
    util::sync_dir(destination.parent().unwrap())
}
fn verify_artifacts(directory: &Path, prepared: &Prepared) -> Result<()> {
    require(
        prepared
            .artifacts
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>()
            == BTreeSet::from([
                "source",
                "source-proof.json",
                "candidate",
                "candidate.jsonl",
                "report.json",
            ]),
        "unexpected artifact set",
    )?;
    for (name, digest) in &prepared.artifacts {
        require(
            util::digest(&util::read(&directory.join(name))?) == *digest,
            format!("artifact changed: {}", directory.join(name).display()),
        )?;
    }
    Ok(())
}
fn validate_config(config: &Config, job: &Path) -> Result<()> {
    require(
        config.version == 1 && !config.targets.is_empty(),
        "expected config version 1 with at least one target",
    )?;
    let mut sources = BTreeSet::new();
    let mut names = BTreeSet::new();
    for target in &config.targets {
        require(
            !target.name.is_empty()
                && target
                    .name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "target name must contain only ASCII letters, digits, hyphens or underscores",
        )?;
        require(names.insert(&target.name), "duplicate target name")?;
        guard(target)?;
        require(
            sources.insert(&target.source),
            "duplicate canonical source (shared SQLite worktree)",
        )?;
        require(
            !job.starts_with(&target.root)
                && !job.starts_with(target.source.parent().unwrap())
                && !target.root.starts_with(job),
            "job must be outside every management root",
        )?;
        if target.input == Input::Current {
            require(
                target.rules == Rules::default(),
                "current input cannot have conversion rules",
            )?;
        }
    }
    Ok(())
}
fn plan_digest(job: &Path) -> Result<String> {
    Ok(util::digest(&util::read(&job.join("plan.json"))?))
}
fn load_plan(job: &Path) -> Result<Plan> {
    util::absolute(job)?;
    let plan: Plan = util::load(&job.join("plan.json"))?;
    require(plan.version == 1, "unsupported job version")?;
    for (path, digest) in &plan.binaries {
        require(
            util::digest(&util::read(path)?) == *digest,
            format!("binary changed since prepare: {}", path.display()),
        )?;
    }
    let exe = fs::canonicalize(std::env::current_exe()?)?;
    require(
        plan.binaries.contains_key(&exe),
        "use the migration binary recorded in plan.json",
    )?;
    Ok(plan)
}
pub fn prepare(config: &Config, job: &Path) -> Result<Vec<Outcome>> {
    require(job.is_absolute(), "job path must be absolute")?;
    util::absolute(job.parent().ok_or("job requires a parent")?)?;
    validate_config(config, job)?;
    if !job.exists() {
        fs::create_dir(job)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(job, fs::Permissions::from_mode(0o700))?;
        }
        util::sync_dir(job.parent().unwrap())?;
    }
    util::absolute(job)?;
    let _lock = util::lock(&job.join("job.lock"))?;
    if !job.join("plan.json").exists() {
        for entry in fs::read_dir(job)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            require(
                name == "job.lock"
                    || (name.starts_with(".migration-record-") && name.ends_with(".tmp")),
                "job directory contains unrelated files; use an empty dedicated directory",
            )?;
        }
        let mut binaries = BTreeMap::new();
        for path in [
            Some(fs::canonicalize(std::env::current_exe()?)?),
            Some(config.axon_binary.clone()),
            config.legacy_binary.clone(),
        ]
        .into_iter()
        .flatten()
        {
            util::absolute(&path)?;
            binaries.insert(path.clone(), util::digest(&util::read(&path)?));
        }
        let plan = Plan {
            version: 1,
            id: axon::lifecycle::RecordId::generate().to_string(),
            at: Utc::now(),
            config: config.clone(),
            binaries,
        };
        save(&job.join("plan.json"), &plan)?;
    }
    let plan = load_plan(job)?;
    require(
        &plan.config == config,
        "config differs from fixed job plan; use a new job",
    )?;
    let digest = plan_digest(job)?;
    let mut prepared_digests = Vec::new();
    let mut legacy_stores = BTreeSet::new();
    let mut outcomes = Vec::new();
    for target in &config.targets {
        let dir = job.join(&target.name);
        if !dir.exists() {
            fs::create_dir(&dir)?;
            util::sync_dir(job)?;
        }
        util::absolute(&dir)?;
        let _locks = locks(target)?;
        let topology = guard(target)?;
        if dir.join("prepared.json").exists() {
            let prepared: Prepared = util::load(&dir.join("prepared.json"))?;
            require(prepared.plan_digest == digest, "plan changed")?;
            verify_artifacts(&dir, &prepared)?;
            if target.input == Input::Legacy {
                let report: convert::Report = util::load(&dir.join("report.json"))?;
                require(
                    legacy_stores.insert(report.old_store),
                    "duplicate legacy store: consolidate its branches before migration",
                )?;
            }
            require(
                prepared.topology == topology,
                "Git topology or index changed",
            )?;
            require(
                source_digest(target, &target.source).is_ok_and(|d| d == prepared.source_digest)
                    || candidate_digest(target.backend, &target.source)
                        .is_ok_and(|d| d == prepared.candidate_digest),
                "source drift; prepare a new job",
            )?;
        } else {
            if !dir.join("source").exists() {
                copy_snapshot(target.backend, &target.source, &dir.join("source"))?;
            }
            let source_digest = source_digest(target, &dir.join("source"))?;
            let source_proof = SourceProof {
                plan_digest: digest.clone(),
                bytes_digest: util::digest(&util::read(&dir.join("source"))?),
                logical_digest: source_digest.clone(),
            };
            if dir.join("source-proof.json").exists() {
                require(
                    util::read(&dir.join("source-proof.json"))?
                        == serde_json::to_vec_pretty(&source_proof)?,
                    "fixed source or plan changed during incomplete prepare",
                )?;
            } else {
                require(
                    !dir.join("candidate").exists() && !dir.join("candidate.jsonl").exists(),
                    "candidate exists without its source proof; retain this job and prepare a new one",
                )?;
                save(&dir.join("source-proof.json"), &source_proof)?;
            }
            require(
                source_digest == self::source_digest(target, &target.source)?,
                "source changed during prepare; preserve this job and prepare a new one",
            )?;
            let (prefix, snapshot, report) = if target.input == Input::Current {
                let (prefix, snapshot) = current(target.backend, &dir.join("source"))?;
                (
                    prefix,
                    snapshot,
                    serde_json::json!({"input":"current","action":"preserved","source_digest":source_digest}),
                )
            } else {
                let legacy = match target.backend {
                    Backend::File => Legacy::parse(&util::read(&dir.join("source"))?)?,
                    Backend::Sqlite => sql::dump(&dir.join("source"))?.legacy()?,
                };
                require(
                    legacy_stores.insert(legacy.store.clone()),
                    "duplicate legacy store: consolidate its branches before migration",
                )?;
                let (snapshot, report) =
                    convert::run(&legacy, &source_digest, &plan.id, plan.at, &target.rules)?;
                (legacy.prefix, snapshot, serde_json::to_value(report)?)
            };
            let bytes = file::encode(&prefix, &snapshot)?;
            atomic(&dir.join("candidate.jsonl"), &bytes)?;
            save(&dir.join("report.json"), &report)?;
            if target.input == Input::Current {
                atomic(&dir.join("candidate"), &util::read(&dir.join("source"))?)?;
            } else if target.backend == Backend::File {
                atomic(&dir.join("candidate"), &bytes)?;
            } else {
                let temp = temporary(&dir);
                drop(sqlite::Store::create(&temp, &prefix, &snapshot)?);
                File::open(&temp)?.sync_all()?;
                require(
                    canonical(Backend::Sqlite, &temp)? == bytes,
                    "SQLite candidate differs from verified snapshot",
                )?;
                fs::rename(temp, dir.join("candidate"))?;
                util::sync_dir(&dir)?;
            }
            require(
                canonical(target.backend, &dir.join("candidate"))? == bytes,
                "candidate round trip changed records",
            )?;
            let candidate_digest = candidate_digest(target.backend, &dir.join("candidate"))?;
            let artifacts = [
                "source",
                "source-proof.json",
                "candidate",
                "candidate.jsonl",
                "report.json",
            ]
            .into_iter()
            .map(|name| Ok((name.into(), util::digest(&util::read(&dir.join(name))?))))
            .collect::<Result<_>>()?;
            require(
                topology == guard(target)?
                    && source_digest == self::source_digest(target, &target.source)?,
                "source changed before prepare completed",
            )?;
            save(
                &dir.join("prepared.json"),
                &Prepared {
                    plan_digest: digest.clone(),
                    source_digest,
                    candidate_digest,
                    topology,
                    artifacts,
                },
            )?;
        }
        prepared_digests.push(util::digest(&util::read(&dir.join("prepared.json"))?));
        outcomes.push(Outcome {
            name: target.name.clone(),
            status: if target.input == Input::Current {
                "preserved"
            } else {
                "prepared"
            }
            .into(),
        });
    }
    let checked = Checked {
        plan_digest: digest,
        targets: prepared_digests,
    };
    if job.join("checked.json").exists() {
        require(
            util::read(&job.join("checked.json"))? == serde_json::to_vec_pretty(&checked)?,
            "checked manifest changed",
        )?;
    } else {
        save(&job.join("checked.json"), &checked)?;
    }
    Ok(outcomes)
}
fn checked(job: &Path) -> Result<(Plan, Vec<Prepared>)> {
    let plan = load_plan(job)?;
    let checked: Checked = util::load(&job.join("checked.json"))?;
    require(
        checked.plan_digest == plan_digest(job)?
            && checked.targets.len() == plan.config.targets.len(),
        "job manifest changed or incomplete",
    )?;
    let mut result = Vec::new();
    for (target, digest) in plan.config.targets.iter().zip(&checked.targets) {
        let dir = job.join(&target.name);
        util::absolute(&dir)?;
        require(
            util::digest(&util::read(&dir.join("prepared.json"))?) == *digest,
            "prepared manifest changed",
        )?;
        let prepared: Prepared = util::load(&dir.join("prepared.json"))?;
        require(
            prepared.plan_digest == checked.plan_digest,
            "prepared plan changed",
        )?;
        verify_artifacts(&dir, &prepared)?;
        require(
            canonical(target.backend, &dir.join("candidate"))?
                == util::read(&dir.join("candidate.jsonl"))?,
            "candidate validation changed",
        )?;
        result.push(prepared);
    }
    Ok((plan, result))
}
fn journal(dir: &Path) -> Result<Journal> {
    if dir.join("journal.json").exists() {
        util::load(&dir.join("journal.json"))
    } else {
        Ok(Journal::default())
    }
}
fn record(dir: &Path, phase: Phase) -> Result<()> {
    let mut journal = journal(dir)?;
    journal.events.push(Event {
        phase,
        at: Utc::now(),
    });
    save(&dir.join("journal.json"), &journal)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Boundary {
    AfterIntent,
    AfterCheckpoint,
    BeforeReplace,
    AfterReplace,
}
pub fn execute(job: &Path, restore: bool, writers_stopped: bool) -> Result<Vec<Outcome>> {
    execute_with(job, restore, writers_stopped, |_, _| Ok(()))
}
// Injection is explicit to the library caller, never a production environment variable.
pub fn execute_with(
    job: &Path,
    restore: bool,
    writers_stopped: bool,
    mut boundary: impl FnMut(&str, Boundary) -> Result<()>,
) -> Result<Vec<Outcome>> {
    require(
        writers_stopped,
        "stop all writers, SQLite connections, editors and Git operations; then pass --writers-stopped",
    )?;
    util::absolute(job)?;
    let _lock = util::lock(&job.join("job.lock"))?;
    let (plan, prepared) = checked(job)?;
    let mut outcomes = Vec::new();
    for (target, prepared) in plan.config.targets.iter().zip(prepared) {
        let dir = job.join(&target.name);
        let result = (|| -> Result<String> {
            let _locks = locks(target)?;
            require(
                guard(target)? == prepared.topology,
                "Git topology or index changed",
            )?;
            let history = journal(&dir)?;
            let last = history.events.last().map(|e| &e.phase);
            let is_source =
                source_digest(target, &target.source).is_ok_and(|d| d == prepared.source_digest);
            let is_candidate = candidate_digest(target.backend, &target.source)
                .is_ok_and(|d| d == prepared.candidate_digest);
            if target.input == Input::Current {
                require(
                    is_source && is_candidate,
                    "preserved current source drifted",
                )?;
                return Ok("preserved".into());
            }
            if restore {
                if is_source {
                    require(
                        matches!(
                            last,
                            None | Some(
                                Phase::ApplyIntent | Phase::RestoreIntent | Phase::Restored
                            )
                        ),
                        "source reappeared without a restore intent; inspect external changes",
                    )?;
                    util::sync_dir(target.source.parent().unwrap())?;
                    if matches!(last, Some(Phase::ApplyIntent | Phase::RestoreIntent)) {
                        record(&dir, Phase::Restored)?;
                    }
                    return Ok(if history.events.is_empty() {
                        "not_applied"
                    } else {
                        "restored_or_not_applied"
                    }
                    .into());
                }
                require(
                    is_candidate,
                    "restore refused: current data differs from the applied candidate; retain new writes",
                )?;
                require(
                    matches!(
                        last,
                        Some(Phase::ApplyIntent | Phase::Applied | Phase::RestoreIntent)
                    ),
                    "restore requires this job's apply journal",
                )?;
            } else {
                require(
                    !matches!(last, Some(Phase::Restored | Phase::RestoreIntent)),
                    "job has entered restore; prepare a new job before applying again",
                )?;
                if is_candidate {
                    require(
                        matches!(last, Some(Phase::ApplyIntent | Phase::Applied)),
                        "candidate appeared without this job's apply intent",
                    )?;
                    util::sync_dir(target.source.parent().unwrap())?;
                    if last != Some(&Phase::Applied) {
                        record(&dir, Phase::Applied)?;
                    }
                    return Ok("applied".into());
                }
                require(
                    is_source,
                    "source drift: no data was replaced; prepare a new job",
                )?;
                require(
                    last != Some(&Phase::Applied),
                    "applied source disappeared; inspect external changes",
                )?;
            }
            let source = dir.join(if restore { "source" } else { "candidate" });
            let attempt = dir.join(format!(
                "{}-{}",
                if restore { "restore" } else { "apply" },
                axon::lifecycle::RecordId::generate()
            ));
            fs::create_dir(&attempt)?;
            util::sync_dir(&dir)?;
            copy_snapshot(target.backend, &target.source, &attempt.join("before"))?;
            if target.backend == Backend::Sqlite {
                // Preserve the stopped physical set before checkpointing it.
                for suffix in ["", "-wal", "-shm", "-journal"] {
                    let path = PathBuf::from(format!("{}{suffix}", target.source.display()));
                    if path.exists() {
                        util::save_new(
                            &attempt.join(format!("physical{suffix}")),
                            &util::read(&path)?,
                        )?;
                    }
                }
            }
            record(
                &dir,
                if restore {
                    Phase::RestoreIntent
                } else {
                    Phase::ApplyIntent
                },
            )?;
            boundary(&target.name, Boundary::AfterIntent)?;
            let expected = if restore {
                &prepared.candidate_digest
            } else {
                &prepared.source_digest
            };
            let inspect = |path: &Path| {
                if restore {
                    candidate_digest(target.backend, path)
                } else {
                    source_digest(target, path)
                }
            };
            require(
                inspect(&target.source)? == *expected,
                "source changed while preserving preimage",
            )?;
            if target.backend == Backend::Sqlite {
                sql::quiesce(&target.source)?;
                boundary(&target.name, Boundary::AfterCheckpoint)?;
                require(
                    inspect(&target.source)? == *expected,
                    "SQLite changed during checkpoint",
                )?;
                for suffix in ["-wal", "-shm", "-journal"] {
                    let path = PathBuf::from(format!("{}{suffix}", target.source.display()));
                    require(
                        !path.exists(),
                        format!(
                            "SQLite sidecar remains: {}; close every connection",
                            path.display()
                        ),
                    )?;
                }
            }
            let temp = temporary(target.source.parent().unwrap());
            util::save_new(&temp, &util::read(&source)?)?;
            fs::set_permissions(&temp, fs::metadata(&target.source)?.permissions())?;
            File::open(&temp)?.sync_all()?;
            boundary(&target.name, Boundary::BeforeReplace)?;
            require(
                guard(target)? == prepared.topology && inspect(&target.source)? == *expected,
                "source changed immediately before replacement",
            )?;
            fs::rename(temp, &target.source)?;
            boundary(&target.name, Boundary::AfterReplace)?;
            util::sync_dir(target.source.parent().unwrap())?;
            let expected_after = if restore {
                &prepared.source_digest
            } else {
                &prepared.candidate_digest
            };
            let actual = if restore {
                source_digest(target, &target.source)?
            } else {
                candidate_digest(target.backend, &target.source)?
            };
            require(
                actual == *expected_after,
                "read-back differs after replacement",
            )?;
            record(
                &dir,
                if restore {
                    Phase::Restored
                } else {
                    Phase::Applied
                },
            )?;
            Ok(if restore { "restored" } else { "applied" }.into())
        })();
        match result {
            Ok(status)=>outcomes.push(Outcome {name:target.name.clone(),status}),
            Err(error)=>return Err(format!("{}: {error}. Result may be partial; later targets were not processed. Retain {} and inspect journal.json and live data before retrying the same job",target.name,job.display()).into()),
        }
    }
    Ok(outcomes)
}
