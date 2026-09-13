use crate::{actor, codec, core, derived::Evaluation, domain::*, merge};
use chrono::{DateTime, Utc};
use clap::Subcommand;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    rc::Rc,
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Debug, thiserror::Error)]
#[error("{source}\n{details}")]
struct ArtifactFailure {
    #[source]
    source: Box<dyn std::error::Error>,
    details: String,
    guidance: String,
}

pub fn error_guidance<'a>(error: &'a (dyn std::error::Error + 'static)) -> Option<&'a str> {
    error
        .downcast_ref::<ArtifactFailure>()
        .map(|failure| failure.guidance.as_str())
}

#[derive(Subcommand)]
pub enum StorageCmd {
    Check { snapshot: PathBuf },
}
#[derive(Subcommand)]
pub enum MergeCmd {
    /// Freeze explicit complete inputs; never changes the destination
    Prepare {
        #[arg(long)]
        base: PathBuf,
        #[arg(long)]
        ours: PathBuf,
        #[arg(long)]
        theirs: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        workspace: PathBuf,
    },
    /// Recompute using edited resolution.json; preserves original inputs
    Check { workspace: PathBuf },
    /// Publish only the last checked candidate; does not stage Git files
    Apply { workspace: PathBuf },
    /// Git low-level driver: base (%O), ours (%A), theirs (%B)
    Driver {
        base: PathBuf,
        ours: PathBuf,
        theirs: PathBuf,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Frozen {
    path: PathBuf,
    digest: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
    inputs: Vec<Frozen>,
    output: PathBuf,
    preimage: Option<String>,
    active_root: Option<PathBuf>,
    store_id: Option<RecordId>,
    context: String,
    driver: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Context {
    at: DateTime<Utc>,
    actor: String,
    root: PathBuf,
    seed: RecordId,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Resolution {
    choices: Vec<merge::Choice>,
    repairs: Vec<Repair>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Repair {
    operation: RepairOp,
    reason: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum RepairOp {
    Start {
        owner: EntityId,
    },
    Change {
        owner: EntityId,
        change: core::Change,
    },
    Dependency {
        source: EntityId,
        target: EntityId,
        present: bool,
    },
    AddNote {
        owner: EntityId,
        body: String,
    },
}
impl RepairOp {
    fn operation(&self, context: &Context) -> Result<core::Operation> {
        Ok(match self {
            Self::Start { owner } => core::Operation::Change(
                owner.clone(),
                core::Change::Start(Claim {
                    actor: context.actor.clone(),
                    worktree: context.root.to_string_lossy().into_owned(),
                    at: context.at,
                }),
            ),
            Self::Change {
                change: core::Change::Start(_),
                ..
            } => return Err(
                "use the start repair operation to retain fixed actor, time and worktree context"
                    .into(),
            ),
            Self::Change { owner, change } => {
                core::Operation::Change(owner.clone(), change.clone())
            }
            Self::Dependency {
                source,
                target,
                present,
            } => core::Operation::Dependency {
                source: source.clone(),
                target: target.clone(),
                present: *present,
            },
            Self::AddNote { owner, body } => core::Operation::AddNote {
                owner: owner.clone(),
                body: body.clone(),
            },
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Checked {
    manifest: String,
    resolution: String,
    candidate: String,
}
fn digest(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}
fn read<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
fn save(path: &Path, value: &impl Serialize) -> Result<()> {
    atomic(path, &serde_json::to_vec_pretty(value)?)
}
fn absolute(path: &Path) -> Result<PathBuf> {
    if path.exists() {
        return Ok(fs::canonicalize(path)?);
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    Ok(fs::canonicalize(parent)?.join(path.file_name().ok_or("missing file name")?))
}
fn output_path(path: &Path, workspace: &Path) -> Result<PathBuf> {
    let path: PathBuf = path.components().collect();
    let mut protected = vec![path.as_path()];
    if let Some(parent) = path.parent()
        && parent.file_name() == Some(std::ffi::OsStr::new(".axon"))
    {
        protected.push(parent);
    }
    for entry in protected {
        match fs::symlink_metadata(entry) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(format!("merge destination symlink is not supported: {}; use a regular destination to retain store binding and writer lock", entry.display()).into());
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    let output = absolute(&path)?;
    if output.starts_with(fs::canonicalize(workspace)?) {
        return Err("merge destination must be outside the workspace; originals and management artifacts cannot be publication targets".into());
    }
    Ok(output)
}
fn save_failure_report(
    path: &Path,
    report: &serde_json::Value,
    original: Box<dyn std::error::Error>,
) -> Box<dyn std::error::Error> {
    match save(path, report) {
        Ok(()) => original,
        Err(followup) => crate::operation_error(
            "merge validation",
            original,
            format!("\nError report update failed: {followup}"),
        ),
    }
}

fn optional(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(crate::operation_error(
            format!("{} read destination", path.display()),
            e,
            "",
        )),
    }
}
fn lock(path: &Path) -> Result<File> {
    let f = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    f.lock()?;
    Ok(f)
}
fn atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    atomic_with(path, bytes, |parent| File::open(parent)?.sync_all())
}
fn atomic_with(
    path: &Path,
    bytes: &[u8],
    sync: impl FnOnce(&Path) -> std::io::Result<()>,
) -> Result<()> {
    let publication = (|| -> Result<()> {
        let temporary =
            path.with_file_name(format!(".merge-{}.tmp", RecordId::new(RecordKind::Store)));
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    publication.map_err(|e| {
        format!(
            "{} publish: {e}\nNot applied: file replacement at {}",
            path.display(),
            path.display()
        )
    })?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    sync(parent).map_err(|e| {
        Box::new(ArtifactFailure {
            source: format!("result unknown after replace at {}: {e}", path.display()).into(),
            details: format!("Result unknown: file durability at {}", path.display()),
            guidance: "Preserve the destination and inspect its contents before retrying.".into(),
        }) as Box<dyn std::error::Error>
    })?;
    Ok(())
}
fn evaluate(root: PathBuf) -> Rc<Evaluation> {
    Rc::new(Evaluation::new(root))
}
pub fn storage(command: StorageCmd) -> Result<()> {
    let StorageCmd::Check { snapshot } = command;
    let s = codec::decode(&fs::read(snapshot)?, evaluate(std::env::current_dir()?))?;
    merge::validate(&s)?;
    println!("Valid snapshot");
    Ok(())
}
fn active_root(output: &Path) -> Option<PathBuf> {
    (output.file_name() == Some(std::ffi::OsStr::new("state.jsonl"))
        && output.parent().and_then(Path::file_name) == Some(std::ffi::OsStr::new(".axon")))
    .then(|| output.parent().unwrap().parent().unwrap().to_path_buf())
}
fn prepare(
    base: &Path,
    ours: &Path,
    theirs: &Path,
    output: &Path,
    workspace: &Path,
    driver: bool,
    preserved_ours: &mut Option<Vec<u8>>,
) -> Result<()> {
    fs::create_dir(workspace)?;
    prepare_created(base, ours, theirs, output, workspace, driver, preserved_ours).map_err(|source| Box::new(ArtifactFailure {
        source,
        details: format!("Applied: workspace directory created at {}\nResult unknown: completeness of workspace artifacts at {}\nNot applied: merge candidate publication at {}", workspace.display(), workspace.display(), output.display()),
        guidance: "Preserve the partial workspace and inspect its artifacts. Correct the reported cause and use a new workspace path for merge prepare.".into(),
    }) as Box<dyn std::error::Error>)
}

fn prepare_created(
    base: &Path,
    ours: &Path,
    theirs: &Path,
    output: &Path,
    workspace: &Path,
    driver: bool,
    preserved_ours: &mut Option<Vec<u8>>,
) -> Result<()> {
    let workspace = fs::canonicalize(workspace)?;
    let mut inputs = Vec::new();
    // Preserve every readable input even when another is missing; Git discards its temporaries.
    let mut failures = Vec::new();
    for (name, path) in ["base", "ours", "theirs"]
        .into_iter()
        .zip([base, ours, theirs])
    {
        let saved = (|| -> Result<Frozen> {
            let bytes = fs::read(path)?;
            atomic(&workspace.join(format!("{name}.jsonl")), &bytes)?;
            if name == "ours" {
                *preserved_ours = Some(bytes.clone());
            }
            Ok(Frozen {
                path: absolute(path)?,
                digest: digest(&bytes),
            })
        })();
        match saved {
            Ok(input) => inputs.push(input),
            Err(error) => failures.push(format!("{name} ({}): {error}", path.display())),
        }
    }
    if !failures.is_empty() {
        let original = format!(
            "input preservation failed; inspect {}: {}",
            workspace.display(),
            failures.join("; ")
        )
        .into();
        return Err(save_failure_report(
            &workspace.join("report.json"),
            &serde_json::json!({"status":"invalid", "errors":failures}),
            original,
        ));
    }
    let output = output_path(output, &workspace).map_err(|e| {
        crate::operation_error(format!("{} resolve destination", output.display()), e, "")
    })?;
    let preimage = optional(&output)?;
    if let Some(bytes) = &preimage {
        atomic(&workspace.join("preimage"), bytes)?;
    }
    let binding = if driver { None } else { active_root(&output) };
    let evaluation_root = if let Some(root) = &binding {
        root.clone()
    } else if driver {
        std::env::current_dir()?
    } else {
        crate::storage::merge_evaluation_root()?
    };
    let store_id = if binding.is_some() {
        let original = codec::decode(
            &fs::read(workspace.join("ours.jsonl"))?,
            evaluate(evaluation_root.clone()),
        )?;
        if let Some(bytes) = &preimage
            && let Ok(destination) = codec::decode(bytes, evaluate(evaluation_root.clone()))
            && destination.metadata.get("store_id") != original.metadata.get("store_id")
        {
            return Err("destination store identity differs from merge inputs".into());
        }
        match original.metadata.get("store_id") {
            Some(core::MetadataValue::Text(id)) => Some(id.parse()?),
            _ => return Err("missing input store identity".into()),
        }
    } else {
        None
    };
    let context = Context {
        at: Utc::now(),
        actor: actor::actor(),
        root: evaluation_root,
        seed: RecordId::new(RecordKind::Store),
    };
    save(&workspace.join("context.json"), &context)?;
    let manifest = Manifest {
        version: 1,
        inputs,
        output: output.clone(),
        preimage: preimage.as_deref().map(digest),
        active_root: binding,
        store_id,
        context: digest(&fs::read(workspace.join("context.json"))?),
        driver,
    };
    save(&workspace.join("manifest.json"), &manifest)?;
    fs::write(
        workspace.join("manifest.digest"),
        digest(&fs::read(workspace.join("manifest.json"))?),
    )?;
    save(
        &workspace.join("resolution.json"),
        &Resolution {
            choices: vec![],
            repairs: vec![],
        },
    )?;
    crate::write_mutation_output(
        &format!(
            "Workspace: {}\n",
            crate::display::human_text(workspace.display())
        ),
        crate::OutputDecoration::Plain,
        &format!("merge workspace at {}", workspace.display()),
    )?;
    check_inner(&workspace)
}
fn drift_read(path: impl AsRef<Path>) -> Result<Vec<u8>> {
    let path = path.as_ref();
    fs::read(path)
        .map_err(|error| format!("input drift: {} cannot be read: {error}", path.display()).into())
}
fn frozen(workspace: &Path, manifest: &Manifest, applied: Option<&[u8]>) -> Result<[Vec<u8>; 3]> {
    output_path(&manifest.output, workspace)?;
    if String::from_utf8(drift_read(workspace.join("manifest.digest"))?)?
        != digest(&drift_read(workspace.join("manifest.json"))?)
    {
        return Err("input drift: manifest changed".into());
    }
    if manifest.version != 1 || manifest.inputs.len() != 3 {
        return Err("unsupported workspace".into());
    }
    if digest(&drift_read(workspace.join("context.json"))?) != manifest.context {
        return Err("input drift: fixed context changed".into());
    }
    let mut bytes = Vec::new();
    for (name, input) in ["base", "ours", "theirs"].into_iter().zip(&manifest.inputs) {
        let original = drift_read(workspace.join(format!("{name}.jsonl")))?;
        if digest(&original) != input.digest {
            return Err("input drift: frozen original changed".into());
        }
        if !manifest.driver {
            let current = drift_read(&input.path)?;
            if digest(&current) != input.digest
                && !(input.path == manifest.output && applied == Some(current.as_slice()))
            {
                return Err(format!("input drift: {}", input.path.display()).into());
            }
        }
        bytes.push(original);
    }
    if let Some(expected) = &manifest.preimage
        && digest(&drift_read(workspace.join("preimage"))?) != *expected
    {
        return Err("input drift: preimage changed".into());
    }
    if let Some(root) = &manifest.active_root {
        crate::storage::check_file_destination(root)?;
    }
    Ok(bytes.try_into().unwrap())
}
fn compute(workspace: &Path, manifest: &Manifest, bytes: [Vec<u8>; 3]) -> Result<merge::Outcome> {
    let context: Context = read(&workspace.join("context.json"))?;
    let resolution: Resolution = read(&workspace.join("resolution.json"))?;
    let [base, ours, theirs] = bytes;
    let evaluation = evaluate(context.root.clone());
    let prepared = merge::Prepared::new(base, ours, theirs, evaluation.clone())?;
    save(&workspace.join("choices.json"), &prepared.choices())?;
    let mut allocators: Vec<_> = resolution
        .repairs
        .iter()
        .enumerate()
        .map(|(index, repair)| {
            let seed = serde_json::to_vec(&(context.seed, index, repair)).unwrap();
            let mut counter = 0usize;
            move |kind| {
                counter += 1;
                RecordId::deterministic(kind, &[seed.as_slice(), &counter.to_le_bytes()].concat())
            }
        })
        .collect();
    let operations: Vec<_> = resolution
        .repairs
        .iter()
        .map(|r| r.operation.operation(&context))
        .collect::<Result<_>>()?;
    let mut repairs: Vec<_> = resolution
        .repairs
        .iter()
        .zip(&mut allocators)
        .zip(operations)
        .map(|((r, ids), operation)| merge::Repair {
            operation,
            context: core::Context {
                at: context.at,
                evaluation: evaluation.clone(),
                actor: &context.actor,
                reason: r.reason.as_deref(),
                ids,
            },
        })
        .collect();
    let outcome = prepared.resolve(&resolution.choices, &mut repairs)?;
    if let (Some(identity), merge::Outcome::Complete(candidate)) = (&manifest.store_id, &outcome)
        && candidate.state().metadata.get("store_id")
            != Some(&core::MetadataValue::Text(identity.to_string()))
    {
        return Err("candidate store identity differs from frozen input".into());
    }
    Ok(outcome)
}
fn check(workspace: &Path) -> Result<()> {
    check_inner(workspace).map_err(|source| Box::new(ArtifactFailure {
        source,
        details: format!("Result unknown: workspace artifact updates at {}\nNot applied: merge candidate publication", workspace.display()),
        guidance: "Preserve the workspace, inspect its original snapshots and generated artifacts, and resolve the reported cause before running merge check again.".into(),
    }) as Box<dyn std::error::Error>)
}
fn check_inner(workspace: &Path) -> Result<()> {
    let _lock = lock(&workspace.join("workspace.lock"))?;
    let manifest: Manifest = read(&workspace.join("manifest.json"))?;
    output_path(&manifest.output, workspace)?;
    let checked = workspace.join("checked.json");
    if checked.exists() {
        fs::remove_file(&checked)?;
    }
    let result = (|| -> Result<()> {
        let bytes = frozen(workspace, &manifest, None)?;
        let resolution_digest = digest(&fs::read(workspace.join("resolution.json"))?);
        match compute(workspace, &manifest, bytes)? {
            merge::Outcome::Complete(candidate) => {
                frozen(workspace, &manifest, None)?;
                if resolution_digest != digest(&fs::read(workspace.join("resolution.json"))?) {
                    return Err("resolution drift during check".into());
                }
                atomic(&workspace.join("candidate.jsonl"), candidate.bytes())?;
                save(
                    &workspace.join("report.json"),
                    &serde_json::json!({"status":"valid","conflicts":[]}),
                )?;
                save(
                    &checked,
                    &Checked {
                        manifest: digest(&fs::read(workspace.join("manifest.json"))?),
                        resolution: resolution_digest,
                        candidate: digest(candidate.bytes()),
                    },
                )?;
                Ok(())
            }
            merge::Outcome::Conflicts(conflicts) => {
                let original: Box<dyn std::error::Error> = "unresolved conflicts; inspect choices.json, report.json and the original snapshots".into();
                let report = (|| -> Result<()> {
                    let choices: serde_json::Value = read(&workspace.join("choices.json"))?;
                    let conflicts: Vec<_> = conflicts.into_iter().map(|conflict| {
                        let kind = if choices.as_array().is_some_and(|items| items.iter().any(|c| c["id"] == conflict.id)) { "entity_bundle" } else { "structure" };
                        serde_json::json!({"id":conflict.id,"kind":kind,"owner":conflict.owner,"message":conflict.message,"inputs":["base.jsonl","ours.jsonl","theirs.jsonl"]})
                    }).collect();
                    save(
                        &workspace.join("report.json"),
                        &serde_json::json!({"status":"unresolved","conflicts":conflicts}),
                    )
                })();
                Err(match report {
                    Ok(()) => original,
                    Err(followup) => crate::operation_error(
                        "merge validation",
                        original,
                        format!("\nConflict report update failed: {followup}"),
                    ),
                })
            }
        }
    })();
    match result {
        Err(e) if !e.to_string().starts_with("unresolved") => Err(save_failure_report(
            &workspace.join("report.json"),
            &serde_json::json!({"status":if e.to_string().contains("drift") {"input_drift"} else {"invalid"},"error":e.to_string()}),
            e,
        )),
        result => result,
    }
}
fn apply(workspace: &Path) -> Result<()> {
    apply_with(workspace, |_| Ok(()))
}
fn apply_with(workspace: &Path, mut checkpoint: impl FnMut(&str) -> Result<()>) -> Result<()> {
    let _workspace_lock = lock(&workspace.join("workspace.lock"))?;
    let manifest_bytes = fs::read(workspace.join("manifest.json"))?;
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes)?;
    let checked: Checked = read(&workspace.join("checked.json"))?;
    let candidate = fs::read(workspace.join("candidate.jsonl"))?;
    if digest(&manifest_bytes) != checked.manifest
        || digest(&fs::read(workspace.join("resolution.json"))?) != checked.resolution
        || digest(&candidate) != checked.candidate
    {
        return Err("checked artifact drift; run merge check again".into());
    }
    output_path(&manifest.output, workspace)?;
    let lockpath = if manifest.active_root.is_some() {
        manifest.output.parent().unwrap().join("write.lock")
    } else {
        manifest.output.with_extension("merge.lock")
    };
    let _lock = lock(&lockpath)?;
    frozen(workspace, &manifest, Some(&candidate))?;
    let state = codec::decode(&candidate, evaluate(std::env::current_dir()?))?;
    merge::validate(&state)?;
    let before = optional(&manifest.output)?;
    if before.as_deref() == Some(&candidate) {
        crate::write_mutation_output(
            "Already applied\n",
            crate::OutputDecoration::Plain,
            &format!("destination already matches {}", manifest.output.display()),
        )?;
        return Ok(());
    }
    if before.as_deref().map(digest) != manifest.preimage {
        return Err("destination preimage drift; not applied".into());
    }
    checkpoint("before-replace")?;
    if optional(&manifest.output)? != before {
        return Err("destination drift; not applied".into());
    }
    frozen(workspace, &manifest, None)?;
    if digest(&fs::read(workspace.join("resolution.json"))?) != checked.resolution
        || digest(&fs::read(workspace.join("candidate.jsonl"))?) != checked.candidate
    {
        return Err("checked artifact drift before publish".into());
    }
    atomic(&manifest.output, &candidate)?;
    checkpoint("after-replace")
        .map_err(|e| format!("result unknown after replace: {e}; inspect retained candidate"))?;
    crate::write_mutation_output(
        &format!(
            "Applied: {}\n",
            crate::display::human_text(manifest.output.display())
        ),
        crate::OutputDecoration::Plain,
        &format!("destination at {}", manifest.output.display()),
    )?;
    Ok(())
}
pub fn run(command: MergeCmd) -> Result<()> {
    match command {
        MergeCmd::Prepare {
            base,
            ours,
            theirs,
            output,
            workspace,
        } => prepare(&base, &ours, &theirs, &output, &workspace, false, &mut None),
        MergeCmd::Check { workspace } => check(&workspace),
        MergeCmd::Apply { workspace } => apply(&workspace),
        MergeCmd::Driver { base, ours, theirs } => {
            let parent = std::env::current_dir()?.join(".axon/merge");
            fs::create_dir_all(&parent)?;
            let workspace = parent.join(RecordId::new(RecordKind::Store).to_string());
            let mut preserved_ours = None;
            match prepare(
                &base,
                &ours,
                &theirs,
                &ours,
                &workspace,
                true,
                &mut preserved_ours,
            )
            .and_then(|()| apply(&workspace))
            {
                Ok(()) => Ok(()),
                Err(e) => driver_conflict(&ours, &workspace, preserved_ours, e),
            }
        }
    }
}

fn driver_conflict(
    ours: &Path,
    workspace: &Path,
    preserved_ours: Option<Vec<u8>>,
    original: Box<dyn std::error::Error>,
) -> Result<()> {
    if original.to_string().starts_with("result unknown") || preserved_ours.is_none() {
        return Err(original);
    }
    let observed = match optional(ours) {
        Ok(bytes) => bytes,
        Err(error) => return Err(driver_followup_failure(original, error, workspace)),
    };
    if observed != preserved_ours {
        return Err(original);
    }
    let marker = format!(
        "<<<<<<< Axon unresolved\nWorkspace: {}\n{}\n=======\nUse preserved base/ours/theirs with merge prepare to resolve explicitly.\n>>>>>>> Axon unresolved\n",
        workspace.display(),
        original
    );
    if let Err(error) = atomic(ours, marker.as_bytes()) {
        return Err(driver_followup_failure(original, error, workspace));
    }
    Err(Box::new(ArtifactFailure {
        source: original,
        details: format!("Applied: conflict marker at {}", ours.display()),
        guidance: format!("Resolve using preserved inputs in {}.", workspace.display()),
    }))
}

fn driver_followup_failure(
    original: Box<dyn std::error::Error>,
    followup: Box<dyn std::error::Error>,
    workspace: &Path,
) -> Box<dyn std::error::Error> {
    Box::new(ArtifactFailure {
        source: original,
        details: format!("Conflict marker handling failed: {followup}"),
        guidance: format!(
            "Preserve the inputs in {} and inspect the conflict marker destination before retrying.",
            workspace.display()
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::tests as f;
    #[test]
    fn prepare_report_failure_keeps_input_preservation_errors() {
        let root = std::env::temp_dir().join(format!(
            "axon-prepare-report-{}",
            RecordId::new(RecordKind::Store)
        ));
        fs::create_dir(&root).unwrap();
        let workspace = root.join("workspace");
        fs::create_dir(&workspace).unwrap();
        fs::create_dir(workspace.join("report.json")).unwrap();
        fs::write(root.join("ours"), b"ours").unwrap();
        fs::write(root.join("theirs"), b"theirs").unwrap();
        let error = prepare_created(
            &root.join("missing-base"),
            &root.join("ours"),
            &root.join("theirs"),
            &root.join("output"),
            &workspace,
            false,
            &mut None,
        )
        .unwrap_err();
        let diagnostic = error.to_string();
        assert!(diagnostic.contains("input preservation failed"));
        assert!(diagnostic.contains("missing-base"));
        assert!(diagnostic.contains("Error report update failed:"));
        assert!(diagnostic.contains("Not applied: file replacement"));
        assert_eq!(fs::read(workspace.join("ours.jsonl")).unwrap(), b"ours");
        assert_eq!(fs::read(workspace.join("theirs.jsonl")).unwrap(), b"theirs");
        assert!(!root.join("output").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn driver_destination_read_failure_retains_the_original_context() {
        let root = std::env::temp_dir().join(format!(
            "axon-driver-read-{}",
            RecordId::new(RecordKind::Store)
        ));
        fs::create_dir(&root).unwrap();
        let error = driver_conflict(
            &root,
            &root.join("workspace"),
            Some(b"original".to_vec()),
            "original input failure\nApplied: preserved inputs".into(),
        )
        .unwrap_err();
        let diagnostic = error.to_string();
        assert!(diagnostic.starts_with("original input failure\nApplied: preserved inputs"));
        assert!(diagnostic.contains("Conflict marker handling failed:"));
        assert!(diagnostic.contains("read destination:"));
        assert!(!diagnostic.contains("Help:"));
        assert!(
            crate::error_guidance(error.as_ref())
                .unwrap()
                .contains("workspace")
        );
        assert!(root.is_dir());
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn published_file_failure_keeps_guidance_out_of_nested_stage_details() {
        let root = std::env::temp_dir().join(format!(
            "axon-merge-guidance-{}",
            RecordId::new(RecordKind::Store)
        ));
        fs::create_dir(&root).unwrap();
        let path = root.join(".gitattributes");
        let failure = atomic_with(&path, b"saved", |_| {
            Err(std::io::Error::other("injected directory sync failure"))
        })
        .unwrap_err();
        assert_eq!(fs::read(&path).unwrap(), b"saved");
        assert!(
            failure
                .to_string()
                .contains("Result unknown: file durability")
        );
        assert!(!failure.to_string().contains("Help:"));
        assert!(
            !crate::error_guidance(failure.as_ref())
                .unwrap()
                .contains("workspace")
        );
        let failure = ArtifactFailure {
            source: failure,
            details: "Applied: destination file".into(),
            guidance: "Inspect the destination before retrying publication.".into(),
        };
        let diagnostic = format!(
            "Error: {failure}\nHelp: {}",
            crate::error_guidance(&failure).unwrap()
        );
        assert_eq!(diagnostic.matches("Help:").count(), 1);
        assert!(diagnostic.find("Applied:").unwrap() < diagnostic.find("Help:").unwrap());
        assert!(!diagnostic.contains("workspace"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn publish_faults_preserve_inputs_and_allow_reconciliation() {
        for checkpoint in ["before-replace", "after-replace"] {
            let root = std::env::temp_dir().join(format!(
                "axon-merge-fault-{}",
                RecordId::new(RecordKind::Store)
            ));
            fs::create_dir(&root).unwrap();
            let mut base = f::empty();
            base.metadata
                .insert("prefix".into(), core::MetadataValue::Text("t".into()));
            base.metadata.insert(
                "store_id".into(),
                core::MetadataValue::Text(RecordId::new(RecordKind::Store).to_string()),
            );
            let ours = f::execute(
                &base,
                core::Operation::Insert(f::entity("t-a", EntityKind::Issue, None), Vec::new()),
                1,
            )
            .unwrap()
            .state()
            .clone();
            let base = codec::encode(&base).unwrap();
            let ours = codec::encode(&ours).unwrap();
            for (name, bytes) in [
                ("base", &base),
                ("ours", &ours),
                ("theirs", &base),
                ("output", &base),
            ] {
                fs::write(root.join(name), bytes).unwrap();
            }
            let workspace = root.join("work");
            prepare(
                &root.join("base"),
                &root.join("ours"),
                &root.join("theirs"),
                &root.join("output"),
                &workspace,
                false,
                &mut None,
            )
            .unwrap();
            assert!(
                apply_with(&workspace, |stage| if stage == checkpoint {
                    Err("injected fault".into())
                } else {
                    Ok(())
                })
                .is_err()
            );
            assert_eq!(fs::read(root.join("base")).unwrap(), base);
            assert_eq!(fs::read(workspace.join("ours.jsonl")).unwrap(), ours);
            let candidate = fs::read(workspace.join("candidate.jsonl")).unwrap();
            assert_eq!(
                fs::read(root.join("output")).unwrap(),
                if checkpoint == "before-replace" {
                    base
                } else {
                    candidate.clone()
                }
            );
            apply(&workspace).unwrap();
            assert_eq!(fs::read(root.join("output")).unwrap(), candidate);
            fs::remove_dir_all(root).unwrap();
        }
    }
}
