//! Reviewable merge workspaces bind inputs, resolution, candidate and destination.
use crate::{
    file::{self, optional_bytes, read_regular},
    lifecycle::*,
    location::Location,
    sqlite::{Result, invalid},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

fn json<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    serde_json::to_vec_pretty(value).map_err(|e| invalid(e.to_string()))
}
fn parse<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T> {
    serde_json::from_slice(bytes).map_err(|e| invalid(e.to_string()))
}
fn digest(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    path: PathBuf,
    digest: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
    inputs: [Input; 3],
    output: PathBuf,
    preimage: Option<String>,
    root: PathBuf,
    context: Context,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resolution {
    pub choices: BTreeMap<EntityId, Side>,
    pub reason: Option<String>,
    #[serde(default)]
    pub repairs: Vec<Repair>,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Repair {
    Write {
        id: EntityId,
        title: Option<String>,
        description: Option<String>,
    },
    Parent {
        id: EntityId,
        parent: Option<EntityId>,
    },
    Dependency {
        id: EntityId,
        needs: EntityId,
        present: bool,
    },
    Condition {
        id: EntityId,
        command: Option<String>,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Checked {
    manifest: String,
    resolution: String,
    candidate: String,
}
fn absolute_destination(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut component_path = PathBuf::new();
    for component in absolute.components() {
        component_path.push(component);
        if component.as_os_str() == ".axon"
            && fs::symlink_metadata(&component_path)?
                .file_type()
                .is_symlink()
        {
            return Err(invalid("destination management directory is a symlink"));
        }
    }
    let original_parent = absolute
        .parent()
        .ok_or_else(|| invalid("destination needs parent"))?;
    if !fs::symlink_metadata(original_parent)?.file_type().is_dir() {
        return Err(invalid("destination directory is not a regular directory"));
    }
    let parent = fs::canonicalize(
        absolute
            .parent()
            .ok_or_else(|| invalid("destination needs parent"))?,
    )?;
    let name = absolute
        .file_name()
        .ok_or_else(|| invalid("destination needs filename"))?;
    let result = parent.join(name);
    optional_bytes(&result)?;
    Ok(result)
}
fn workspace_lock(workspace: &Path) -> Result<fs::File> {
    file::lock(&workspace.join("workspace.lock"))
}
fn save(path: &Path, bytes: &[u8]) -> Result<()> {
    let before = optional_bytes(path)?;
    file::publish(path, before.as_deref(), bytes, || Ok(()))
}
fn load(workspace: &Path) -> Result<(Vec<u8>, Manifest, [Vec<u8>; 3])> {
    let workspace = fs::canonicalize(workspace)?;
    let bytes = read_regular(&workspace.join("manifest.json"))?;
    let manifest: Manifest = parse(&bytes)?;
    if manifest.version != 1 {
        return Err(invalid("unknown merge workspace version"));
    }
    if absolute_destination(&manifest.output)? != manifest.output
        || manifest.output.starts_with(&workspace)
    {
        return Err(invalid("destination aliases workspace or changed path"));
    }
    let mut snapshots = Vec::new();
    for (input, name) in manifest
        .inputs
        .iter()
        .zip(["base.jsonl", "ours.jsonl", "theirs.jsonl"])
    {
        if fs::canonicalize(&input.path)? != input.path {
            return Err(invalid("input path changed"));
        }
        let original = read_regular(&input.path)?;
        let preserved = read_regular(&workspace.join(name))?;
        if digest(&original) != input.digest || original != preserved {
            return Err(invalid(format!("input drift: {}", input.path.display())));
        }
        snapshots.push(preserved);
    }
    let preimage = optional_bytes(&manifest.output)?;
    if preimage.as_ref().map(|bytes| digest(bytes)) != manifest.preimage {
        return Err(invalid("destination changed since prepare"));
    }
    let retained = optional_bytes(&workspace.join("preimage"))?;
    if retained != preimage {
        return Err(invalid("preserved destination changed"));
    }
    // A valid live snapshot must be one of the reviewed branches. Otherwise
    // replacing it could discard a different store or unreviewed work/records.
    if let Some(preimage) = &preimage
        && let Ok(destination) = file::decode(preimage)
    {
        let ours = file::decode(&snapshots[1])?;
        let theirs = file::decode(&snapshots[2])?;
        if destination != ours && destination != theirs {
            return Err(invalid(
                "destination is not either reviewed branch; preserve its changes as a merge input",
            ));
        }
    }
    Ok((bytes, manifest, snapshots.try_into().unwrap()))
}
fn plan(inputs: &[Vec<u8>; 3]) -> Result<(String, MergePlan)> {
    let (prefix, base) = file::decode(&inputs[0])?;
    let (ours_prefix, ours) = file::decode(&inputs[1])?;
    let (theirs_prefix, theirs) = file::decode(&inputs[2])?;
    if prefix != ours_prefix || prefix != theirs_prefix {
        return Err(invalid("different store prefixes"));
    }
    Ok((prefix, MergePlan::prepare(&base, &ours, &theirs)?))
}
pub fn prepare(
    base: &Path,
    ours: &Path,
    theirs: &Path,
    output: &Path,
    workspace: &Path,
    context: Context,
) -> Result<()> {
    let output = absolute_destination(output)?;
    let workspace = absolute_destination(workspace)?;
    if output.starts_with(&workspace) {
        return Err(invalid("destination must be outside workspace"));
    }
    let root = Location::discover(output.parent().unwrap(), false)?.root;
    if output != root.join(".axon/state.jsonl") {
        return Err(invalid(
            "merge destination must be the active .axon/state.jsonl",
        ));
    }
    let paths = [
        fs::canonicalize(base)?,
        fs::canonicalize(ours)?,
        fs::canonicalize(theirs)?,
    ];
    let inputs = [
        read_regular(&paths[0])?,
        read_regular(&paths[1])?,
        read_regular(&paths[2])?,
    ];
    let preimage = optional_bytes(&output)?;
    fs::create_dir(&workspace)?;
    let _lock = workspace_lock(&workspace)?;
    let manifest = Manifest {
        version: 1,
        inputs: std::array::from_fn(|i| Input {
            path: paths[i].clone(),
            digest: digest(&inputs[i]),
        }),
        output,
        preimage: preimage.as_ref().map(|bytes| digest(bytes)),
        root,
        context,
    };
    for (bytes, name) in inputs
        .iter()
        .zip(["base.jsonl", "ours.jsonl", "theirs.jsonl"])
    {
        save(&workspace.join(name), bytes)?;
    }
    if let Some(preimage) = preimage {
        save(&workspace.join("preimage"), &preimage)?;
    }
    save(&workspace.join("manifest.json"), &json(&manifest)?)?;
    save(
        &workspace.join("resolution.json"),
        &json(&Resolution::default())?,
    )?;
    let result = (|| -> Result<()> {
        let (_, plan) = plan(&inputs)?;
        save(
            &workspace.join("choices.json"),
            &json(
                &serde_json::json!({"automatic": plan.automatic(), "conflicts": plan.conflicts()}),
            )?,
        )?;
        check_inner(&workspace)
    })();
    if let Err(error) = &result {
        save(
            &workspace.join("report.json"),
            &json(
                &serde_json::json!({"status": "invalid_or_unresolved", "error": error.to_string()}),
            )?,
        )?;
    }
    result
}
pub fn check(workspace: &Path) -> Result<()> {
    let _lock = workspace_lock(workspace)?;
    // An unsuccessful recheck invalidates any earlier certificate.
    if optional_bytes(&workspace.join("checked.json"))?.is_some() {
        fs::remove_file(workspace.join("checked.json"))?;
    }
    let result = check_inner(workspace);
    if let Err(error) = &result {
        save(
            &workspace.join("report.json"),
            &json(
                &serde_json::json!({"status": "invalid_or_unresolved", "error": error.to_string()}),
            )?,
        )?;
    }
    result
}
fn check_inner(workspace: &Path) -> Result<()> {
    let (manifest_bytes, manifest, inputs) = load(workspace)?;
    let resolution_bytes = read_regular(&workspace.join("resolution.json"))?;
    let resolution: Resolution = parse(&resolution_bytes)?;
    let (prefix, plan) = plan(&inputs)?;
    let mut candidate = plan.resolve(&resolution.choices, resolution.reason, manifest.context)?;
    for repair in resolution.repairs {
        match repair {
            Repair::Write {
                id,
                title,
                description,
            } => candidate.write(&id, title, description)?,
            Repair::Parent { id, parent } => candidate.set_parent(&id, parent)?,
            Repair::Dependency { id, needs, present } => {
                if present {
                    candidate.add_dependency(&id, &needs)?;
                } else {
                    candidate.remove_dependency(&id, &needs)?;
                }
            }
            Repair::Condition { id, command } => candidate.set_condition(&id, command)?,
        }
    }
    let bytes = file::encode(&prefix, &candidate)?;
    let checked = Checked {
        manifest: digest(&manifest_bytes),
        resolution: digest(&resolution_bytes),
        candidate: digest(&bytes),
    };
    save(&workspace.join("candidate.jsonl"), &bytes)?;
    save(&workspace.join("checked.json"), &json(&checked)?)?;
    save(
        &workspace.join("report.json"),
        &json(&serde_json::json!({"status": "valid"}))?,
    )?;
    Ok(())
}
pub fn apply(workspace: &Path) -> Result<()> {
    let workspace = fs::canonicalize(workspace)?;
    let _workspace_lock = workspace_lock(&workspace)?;
    let (manifest_bytes, manifest, _) = load(&workspace)?;
    let checked_bytes = read_regular(&workspace.join("checked.json"))?;
    let checked: Checked = parse(&checked_bytes)?;
    let resolution = read_regular(&workspace.join("resolution.json"))?;
    let bytes = read_regular(&workspace.join("candidate.jsonl"))?;
    if checked.manifest != digest(&manifest_bytes)
        || checked.resolution != digest(&resolution)
        || checked.candidate != digest(&bytes)
    {
        return Err(invalid("checked artifacts changed; run merge check again"));
    }
    let (prefix, candidate) = file::decode(&bytes)?;
    let location = Location::discover(&manifest.root, false)?;
    if location.root != manifest.root || !location.is_file()? {
        return Err(invalid("merge destination backend changed"));
    }
    let canonical = location.root.join(".axon/state.jsonl");
    if manifest.output != canonical {
        return Err(invalid(
            "merge apply destination must be the active .axon/state.jsonl",
        ));
    }
    let _lock = file::lock(&location.root.join(".axon/state.lock"))?;
    let before = optional_bytes(&canonical)?;
    // A conflict-marker preimage is intentionally allowed: apply resolves it,
    // but the index stays unmerged until the user stages the verified snapshot.
    let (_, _, inputs) = load(&workspace)?;
    let (expected_prefix, source) = file::decode(&inputs[1])?;
    if prefix != expected_prefix || candidate.store() != source.store() {
        return Err(invalid("candidate store identity changed"));
    }
    file::publish(&canonical, before.as_deref(), &bytes, || {
        if load(&workspace)?.0 != manifest_bytes {
            return Err(invalid("manifest changed during apply"));
        }
        let current = Location::discover(&manifest.root, false)?;
        if !current.is_file()? || current.root != manifest.root {
            return Err(invalid("backend changed"));
        }
        if read_regular(&workspace.join("checked.json"))? != checked_bytes
            || read_regular(&workspace.join("candidate.jsonl"))? != bytes
            || read_regular(&workspace.join("resolution.json"))? != resolution
        {
            return Err(invalid("checked artifacts changed during apply"));
        }
        Ok(())
    })
}
pub fn driver(base: &Path, ours: &Path, theirs: &Path, context: Context) -> Result<()> {
    let paths = [
        fs::canonicalize(base)?,
        fs::canonicalize(ours)?,
        fs::canonicalize(theirs)?,
    ];
    if paths[0] == paths[1] || paths[1] == paths[2] {
        return Err(invalid("Git driver input paths must differ"));
    }
    let inputs = [
        read_regular(&paths[0])?,
        read_regular(&paths[1])?,
        read_regular(&paths[2])?,
    ];
    let (prefix, plan) = plan(&inputs)?;
    let result = plan.resolve(&BTreeMap::new(), None, context)?;
    let bytes = file::encode(&prefix, &result)?;
    file::publish(&paths[1], Some(&inputs[1]), &bytes, || {
        for i in 0..3 {
            if read_regular(&paths[i])? != inputs[i] {
                return Err(invalid("Git driver input changed"));
            }
        }
        Ok(())
    })
}
