//! One-time conversion of a store written before labels (`axon-records/v1`) to the labelled
//! format (`axon-records/v2`).
//!
//! A v1 record is the v2 record without the `label` key after `description`. Adding the key
//! changes the bytes and therefore the record ID, so every record is rebuilt in causal order:
//! its Entity's label goes into `after`, and `parents` and `chosen` point at the rebuilt IDs.
//! Notes carry no `after` and keep their bytes and IDs. The rebuilt records are written by the
//! common core's encoder, so they are the canonical v2 bytes. The conversion depends only on the
//! v1 bytes and the labels, so two stores that share records and are converted with the same
//! labels give the shared records the same IDs.
//!
//! The input store is never changed; the result is written to a new management root and
//! checked against the input before it is used.
use axon_core::lifecycle::record::{
    self, EARLIER_HEADER_FORMAT, Entry, HEADER_FORMAT, Header, RECORD_ID_LENGTH, RecordId,
    RecordKind,
};
use axon_core::lifecycle::{EntityId, Label, StoreId};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The label of an Entity without an entry in the labels file whose current value is terminal.
pub const TERMINAL_DEFAULT: Label = Label::Chore;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;
fn error(message: impl Into<String>) -> Error {
    Error(message.into())
}
fn io(path: &Path, e: std::io::Error) -> Error {
    error(format!("{}: {e}", path.display()))
}

/// The label of each Entity, read from a labels file: one `ENTITY-ID LABEL` pair per line.
/// Blank lines and lines starting with `#` are ignored. Entities the store does not hold are
/// allowed, so one file can serve every branch of a repository.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Labels(BTreeMap<EntityId, Label>);
impl Labels {
    pub fn parse(text: &str) -> Result<Self> {
        let mut labels = BTreeMap::new();
        for (index, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let at = |message: String| error(format!("labels line {}: {message}", index + 1));
            let fields: Vec<&str> = line.split_whitespace().collect();
            let [id, name] = fields[..] else {
                return Err(at(format!("expected `ENTITY-ID LABEL`, got {line:?}")));
            };
            let id = EntityId::try_from(id.to_string()).map_err(|e| at(e.0))?;
            let label = Label::from_name(name).map_err(|e| at(e.0))?;
            if labels.insert(id.clone(), label).is_some() {
                return Err(at(format!("{id} is listed more than once")));
            }
        }
        Ok(Self(labels))
    }
    pub fn read(path: &Path) -> Result<Self> {
        Self::parse(&fs::read_to_string(path).map_err(|e| io(path, e))?)
    }
    pub fn get(&self, id: &EntityId) -> Option<Label> {
        self.0.get(id).copied()
    }
    pub fn ids(&self) -> impl Iterator<Item = &EntityId> {
        self.0.keys()
    }
}

/// The files of one store as bytes: the header and every record file by the ID its name
/// states, checked against the hash of its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Files {
    pub header: Vec<u8>,
    pub gitignore: Option<Vec<u8>>,
    pub gitattributes: Option<Vec<u8>>,
    pub records: BTreeMap<RecordId, Vec<u8>>,
}

/// The bytes of a regular file, None if it does not exist. A symlink or another kind of file
/// is refused, as the store adapter refuses it.
fn regular(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::symlink_metadata(path) {
        Ok(kind) if kind.is_file() => fs::read(path).map(Some).map_err(|e| io(path, e)),
        Ok(_) => Err(error(format!(
            "{}: not a regular file; the store is not converted",
            path.display()
        ))),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io(path, e)),
    }
}
/// Whether a directory exists; a symlink or a file in its place is refused.
fn directory(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(kind) if kind.is_dir() => Ok(true),
        Ok(_) => Err(error(format!(
            "{}: not a directory; the store is not converted",
            path.display()
        ))),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(false),
        Err(e) => Err(io(path, e)),
    }
}
fn is_hex(name: &str, length: usize) -> bool {
    name.len() == length
        && name
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn is_temporary(name: &str) -> bool {
    name.ends_with(".tmp")
}

/// Reads the store under `root/.axon` without interpreting the records.
pub fn read_files(root: &Path) -> Result<Files> {
    read_store(&root.join(".axon"))
}

/// Reads the store in the directory `axon` without interpreting the records. A damaged record
/// directory (an unexpected file, a name that is not a record ID, bytes whose hash is not the
/// name) is refused with every damaged path listed. Temporary files are ignored, as readers do.
fn read_store(axon: &Path) -> Result<Files> {
    if !directory(axon)? {
        return Err(error(format!("{}: no store", axon.display())));
    }
    let header_path = axon.join("header.json");
    let header = regular(&header_path)?
        .ok_or_else(|| error(format!("{}: no store header", header_path.display())))?;
    let base = axon.join("records");
    let mut records = BTreeMap::new();
    let mut damaged = Vec::new();
    let entries = if directory(&base)? {
        Some(fs::read_dir(&base).map_err(|e| io(&base, e))?)
    } else {
        None
    };
    for entry in entries.into_iter().flatten() {
        let entry = entry.map_err(|e| io(&base, e))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if is_temporary(&name) {
            continue;
        }
        let kind = fs::symlink_metadata(&path).map_err(|e| io(&path, e))?;
        if !kind.is_dir() || !is_hex(&name, 2) {
            damaged.push(format!("{}: not a record subdirectory", path.display()));
            continue;
        }
        for file in fs::read_dir(&path).map_err(|e| io(&path, e))? {
            let file = file.map_err(|e| io(&path, e))?;
            let file_name = file.file_name().to_string_lossy().into_owned();
            let file_path = file.path();
            if is_temporary(&file_name) {
                continue;
            }
            let kind = fs::symlink_metadata(&file_path).map_err(|e| io(&file_path, e))?;
            if !kind.is_file() || !is_hex(&file_name, RECORD_ID_LENGTH) || file_name[..2] != name {
                damaged.push(format!("{}: not a record file", file_path.display()));
                continue;
            }
            let bytes = fs::read(&file_path).map_err(|e| io(&file_path, e))?;
            let id = RecordId::of(&bytes);
            if id.as_ref() != file_name {
                damaged.push(format!(
                    "{}: the hash of the bytes is not the name",
                    file_path.display()
                ));
                continue;
            }
            records.insert(id, bytes);
        }
    }
    if !damaged.is_empty() {
        damaged.sort();
        return Err(error(format!(
            "the store is corrupt and is not converted:\n  {}",
            damaged.join("\n  ")
        )));
    }
    Ok(Files {
        header,
        gitignore: regular(&axon.join(".gitignore"))?,
        gitattributes: regular(&axon.join(".gitattributes"))?,
        records,
    })
}

fn git(root: &Path, args: &[&str]) -> Result<std::process::Output> {
    let mut command = Command::new("git");
    command.current_dir(root).args(args);
    // The repository is the one `root` is in, not a calling Git hook's.
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("GIT_") {
            command.env_remove(name);
        }
    }
    let output = command.output().map_err(|e| {
        error(format!(
            "cannot run git to check the Git index of {}: {e}",
            root.display()
        ))
    })?;
    if !output.status.success() {
        return Err(error(format!(
            "cannot check the Git index of {}: {}",
            root.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(output)
}

/// Refuses a store whose `.axon` has unmerged paths in the Git index: the files in the work
/// tree are then not the store Git will commit. Only a root with no `.git` or `HEAD` (the mark
/// of a Git directory) in itself or an ancestor is outside Git, as in the store discovery;
/// inside Git, a Git that cannot answer stops the conversion.
pub fn check_index(root: &Path) -> Result<()> {
    let root = fs::canonicalize(root).map_err(|e| io(root, e))?;
    if !root.ancestors().any(|directory| {
        [".git", "HEAD"]
            .iter()
            .any(|mark| fs::symlink_metadata(directory.join(mark)).is_ok())
    }) {
        return Ok(());
    }
    let inside = git(&root, &["rev-parse", "--is-inside-work-tree"])?;
    if inside.stdout.trim_ascii() != b"true" {
        return Err(error(format!(
            "{} is inside a Git directory, not a work tree",
            root.display()
        )));
    }
    let listed = git(
        &root,
        &["ls-files", "--unmerged", "--full-name", "-z", "--", ".axon"],
    )?;
    let paths = listed
        .stdout
        .split(|&b| b == 0)
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            let tab = entry
                .iter()
                .position(|&b| b == b'\t')
                .ok_or_else(|| error("cannot check the Git index: unexpected ls-files output"))?;
            Ok(String::from_utf8_lossy(&entry[tab + 1..]).into_owned())
        })
        .collect::<Result<BTreeSet<String>>>()?;
    if paths.is_empty() {
        return Ok(());
    }
    Err(error(format!(
        "the Git index has unmerged paths under .axon; finish or abort the merge first:\n  {}",
        paths.into_iter().collect::<Vec<_>>().join("\n  ")
    )))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HeaderRow {
    format: String,
    store: StoreId,
    prefix: String,
}

/// The v1 header's store and prefix, read as the earlier reader read it: one JSON line with
/// exactly these keys. Refuses any other format. The output header is encoded anew.
pub fn read_header(bytes: &[u8]) -> Result<Header> {
    let corrupt = |message: String| error(format!("corrupt header: {message}"));
    let line = bytes
        .strip_suffix(b"\n")
        .ok_or_else(|| corrupt("it does not end with a line feed".into()))?;
    if line.contains(&b'\n') {
        return Err(corrupt("it spans more than one line".into()));
    }
    let row: HeaderRow = serde_json::from_slice(line).map_err(|e| corrupt(e.to_string()))?;
    if row.format == HEADER_FORMAT {
        return Err(error(format!(
            "the store is already {HEADER_FORMAT:?} and needs no conversion"
        )));
    }
    if row.format != EARLIER_HEADER_FORMAT {
        return Err(error(format!(
            "unsupported store format {:?}: only {EARLIER_HEADER_FORMAT:?} is converted",
            row.format
        )));
    }
    let header = Header {
        store: row.store,
        prefix: row.prefix,
    };
    record::encode_header(&header).map_err(|e| corrupt(e.0))?;
    Ok(header)
}

/// One v1 record file, read just far enough to rebuild it.
struct Earlier<'a> {
    bytes: &'a [u8],
    entity: EntityId,
    /// None for a Note.
    record: Option<EarlierRecord>,
}
struct EarlierRecord {
    parents: BTreeSet<RecordId>,
    description: String,
    terminal: bool,
    /// The lifecycle and title, shown when the Entity needs a label.
    summary: String,
}

fn object<'a>(value: &'a Value, what: &str) -> Result<&'a Map<String, Value>> {
    value
        .as_object()
        .ok_or_else(|| error(format!("{what} is not an object")))
}
fn string<'a>(map: &'a Map<String, Value>, key: &str) -> Result<&'a str> {
    map.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| error(format!("missing string {key:?}")))
}
fn record_ids(map: &Map<String, Value>) -> Result<BTreeSet<RecordId>> {
    map.get("parents")
        .and_then(Value::as_array)
        .ok_or_else(|| error("missing list \"parents\""))?
        .iter()
        .map(|parent| {
            let parent = parent
                .as_str()
                .ok_or_else(|| error("a parent is not a string"))?;
            RecordId::try_from(parent).map_err(|e| error(e.0))
        })
        .collect()
}

fn read_earlier(bytes: &[u8]) -> Result<Earlier<'_>> {
    let line = bytes
        .strip_suffix(b"\n")
        .ok_or_else(|| error("the record does not end with a line feed"))?;
    let value: Value = serde_json::from_slice(line).map_err(|e| error(e.to_string()))?;
    let row = object(&value, "the record")?;
    let entity = EntityId::try_from(string(row, "entity")?.to_string()).map_err(|e| error(e.0))?;
    match string(row, "record")? {
        "note" => {
            // A Note has the same bytes in both formats.
            match record::decode(bytes).map_err(|e| error(e.0))?.1 {
                Entry::Note(_) => Ok(Earlier {
                    bytes,
                    entity,
                    record: None,
                }),
                Entry::Record(_) => Err(error("not a Note")),
            }
        }
        "label" => Err(error("a label record cannot precede labels")),
        _ => {
            let after = object(
                row.get("after")
                    .ok_or_else(|| error("missing key \"after\""))?,
                "after",
            )?;
            if after.contains_key("label") {
                return Err(error("a record written before labels carries a label"));
            }
            let lifecycle = string(after, "lifecycle")?;
            let description = string(after, "description")?;
            // Decoding with any label checks the record before labels are chosen, so a
            // corrupt store is reported before the Entities that need a label.
            record::decode(&with_label(bytes, description, TERMINAL_DEFAULT)?)
                .map_err(|e| error(e.0))?;
            Ok(Earlier {
                bytes,
                entity,
                record: Some(EarlierRecord {
                    parents: record_ids(row)?,
                    description: description.to_string(),
                    terminal: matches!(lifecycle, "completed" | "cancelled"),
                    summary: format!("{lifecycle}  {}", string(after, "title")?),
                }),
            })
        }
    }
}

/// The v2 bytes of a v1 record with the old parent IDs: `label` inserted after `description`.
/// The description's canonical literal is followed by `,"condition":` only inside `after`,
/// the last object of a record; a string cannot hold an unescaped quote, and the recorder data
/// orders its keys, so `condition` never follows `description` there. The common core decodes
/// the result, so an insertion anywhere else is refused rather than converted.
fn with_label(bytes: &[u8], description: &str, label: Label) -> Result<Vec<u8>> {
    let literal = serde_json::to_string(description).map_err(|e| error(e.to_string()))?;
    let marker = format!("\"description\":{literal},\"condition\":");
    let text = std::str::from_utf8(bytes).map_err(|e| error(e.to_string()))?;
    let at = text
        .rfind(&marker)
        .ok_or_else(|| error("the record bytes are not canonical"))?
        + marker.len()
        - ",\"condition\":".len();
    let mut out = String::with_capacity(text.len() + 24);
    out.push_str(&text[..at]);
    out.push_str(&format!(",\"label\":\"{}\"", label.name()));
    out.push_str(&text[at..]);
    Ok(out.into_bytes())
}

/// The converted store: header and record files as bytes, and each input record's new ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Converted {
    pub files: Files,
    pub ids: BTreeMap<RecordId, RecordId>,
    pub labels: BTreeMap<EntityId, Label>,
    pub notes: usize,
}

fn listed(problem: &str, items: impl IntoIterator<Item = String>) -> Error {
    error(format!(
        "{problem}:\n  {}",
        items.into_iter().collect::<Vec<_>>().join("\n  ")
    ))
}
const CORRUPT: &str = "the store is corrupt and is not converted";
const GAP: &str = "records are missing (a gap) and the store is not converted";

/// Converts the files of a v1 store. Refuses a corrupt store, one with missing records (a
/// parent record missing, or Notes of an Entity without records), and an Entity without a
/// label whose current value is not terminal; every such record or Entity is listed. Conflicts
/// and violations are converted as they are: a conflicted Entity takes one label on every
/// head, `chore` without an entry only when every head is terminal.
pub fn convert(files: &Files, labels: &Labels) -> Result<Converted> {
    let header = read_header(&files.header)?;
    let mut earlier = BTreeMap::new();
    let mut damaged = Vec::new();
    for (id, bytes) in &files.records {
        match read_earlier(bytes) {
            Ok(entry) => {
                earlier.insert(id.clone(), entry);
            }
            Err(e) => damaged.push(format!("{id}: {e}")),
        }
    }
    let mut missing = BTreeSet::new();
    let mut children: BTreeMap<&RecordId, Vec<&RecordId>> = BTreeMap::new();
    let mut recorded: BTreeSet<&EntityId> = BTreeSet::new();
    for (id, entry) in &earlier {
        let Some(record) = &entry.record else {
            continue;
        };
        recorded.insert(&entry.entity);
        for parent in &record.parents {
            match earlier.get(parent) {
                None => {
                    missing.insert(format!("{parent} (a parent of {id})"));
                }
                Some(Earlier { record: None, .. }) => {
                    damaged.push(format!("{id}: the parent {parent} is a Note"));
                }
                Some(other) if other.entity != entry.entity => {
                    damaged.push(format!("{id}: the parent {parent} is another Entity's"));
                }
                Some(_) => children.entry(parent).or_default().push(id),
            }
        }
    }
    for entry in earlier.values() {
        if entry.record.is_none() && !recorded.contains(&entry.entity) {
            missing.insert(format!("every record of {} (it has Notes)", entry.entity));
        }
    }
    if !damaged.is_empty() {
        return Err(listed(CORRUPT, damaged));
    }
    if !missing.is_empty() {
        return Err(listed(GAP, missing));
    }

    let chosen = choose_labels(
        earlier.iter().filter_map(|(id, entry)| {
            let record = entry.record.as_ref()?;
            Some((
                &entry.entity,
                record.terminal,
                children.contains_key(id),
                record.summary.as_str(),
            ))
        }),
        labels,
    )?;

    // Causal order: a record is rebuilt once its parents are. A record that fails leaves its
    // descendants unconverted; every failure is listed.
    let mut ids: BTreeMap<RecordId, RecordId> = BTreeMap::new();
    let mut records = BTreeMap::new();
    let mut failures = Vec::new();
    let mut waiting: BTreeMap<&RecordId, usize> = BTreeMap::new();
    let mut ready: Vec<&RecordId> = Vec::new();
    for (id, entry) in &earlier {
        match &entry.record {
            Some(record) if !record.parents.is_empty() => {
                waiting.insert(id, record.parents.len());
            }
            _ => ready.push(id),
        }
    }
    while let Some(id) = ready.pop() {
        let entry = &earlier[id];
        let bytes = match &entry.record {
            None => entry.bytes.to_vec(),
            Some(record) => match rebuild(entry, record, chosen[&entry.entity], &ids) {
                Ok(bytes) => bytes,
                Err(e) => {
                    failures.push(format!("{id}: {e}"));
                    continue;
                }
            },
        };
        let new = RecordId::of(&bytes);
        ids.insert(id.clone(), new.clone());
        records.insert(new, bytes);
        for child in children.get(id).into_iter().flatten() {
            let count = waiting
                .get_mut(child)
                .expect("a child waits for its parents");
            *count -= 1;
            if *count == 0 {
                ready.push(child);
            }
        }
    }
    if !failures.is_empty() {
        return Err(listed(CORRUPT, failures));
    }
    if ids.len() != earlier.len() {
        return Err(error(format!("{CORRUPT}: its records form a causal cycle")));
    }

    // The derivation of the whole set finds records that do not continue their parent.
    let mut store = record::Store::new();
    for bytes in records.values() {
        store.insert_bytes(bytes).map_err(|e| error(e.0))?;
    }
    let problems = store.problems();
    if !problems.is_empty() {
        let inverse: BTreeMap<&RecordId, &RecordId> = ids.iter().map(|(o, n)| (n, o)).collect();
        return Err(listed(
            CORRUPT,
            problems
                .iter()
                .map(|(id, e)| format!("{}: {e}", inverse[id])),
        ));
    }
    let view = store.view().map_err(|e| error(format!("{CORRUPT}: {e}")))?;
    if !view.gaps().is_empty() || !view.noted_only().is_empty() {
        return Err(listed(
            GAP,
            view.gaps()
                .keys()
                .chain(view.noted_only())
                .map(ToString::to_string),
        ));
    }

    Ok(Converted {
        files: Files {
            header: record::encode_header(&header).map_err(|e| error(e.0))?,
            gitignore: files.gitignore.clone(),
            gitattributes: files.gitattributes.clone(),
            records,
        },
        ids,
        labels: chosen,
        notes: earlier
            .values()
            .filter(|entry| entry.record.is_none())
            .count(),
    })
}

/// The label of every Entity with records, from `(Entity, terminal, continued, summary)` for
/// each record other than a Note. Heads are the records no other record continues. An Entity
/// without a line gets `chore` only when every head is terminal; otherwise every such Entity
/// is listed with the lifecycle and title of its heads, and nothing is converted.
fn choose_labels<'a>(
    records: impl Iterator<Item = (&'a EntityId, bool, bool, &'a str)>,
    labels: &Labels,
) -> Result<BTreeMap<EntityId, Label>> {
    let mut terminal: BTreeMap<&EntityId, (bool, Vec<&str>)> = BTreeMap::new();
    for (entity, is_terminal, continued, summary) in records {
        let (all, heads) = terminal.entry(entity).or_insert((true, Vec::new()));
        if !continued {
            *all &= is_terminal;
            heads.push(summary);
        }
    }
    let mut chosen = BTreeMap::new();
    let mut unlabelled = Vec::new();
    for (entity, (terminal, heads)) in terminal {
        match labels.get(entity) {
            Some(label) => {
                chosen.insert(entity.clone(), label);
            }
            None if terminal => {
                chosen.insert(entity.clone(), TERMINAL_DEFAULT);
            }
            None => unlabelled.push(format!("{entity}  {}", heads.join(" | "))),
        }
    }
    if !unlabelled.is_empty() {
        return Err(listed(
            "these Entities are not terminal and have no label in the labels file",
            unlabelled,
        ));
    }
    Ok(chosen)
}

/// The canonical v2 bytes of one v1 record: decoded by the common core with the label
/// inserted, which also proves the v1 bytes canonical, then encoded with the new parents.
fn rebuild(
    entry: &Earlier<'_>,
    earlier: &EarlierRecord,
    label: Label,
    ids: &BTreeMap<RecordId, RecordId>,
) -> Result<Vec<u8>> {
    let labelled = with_label(entry.bytes, &earlier.description, label)?;
    let Entry::Record(mut record) = record::decode(&labelled).map_err(|e| error(e.0))?.1 else {
        return Err(error("not a record"));
    };
    let map = |id: &RecordId| {
        ids.get(id)
            .cloned()
            .ok_or_else(|| error(format!("{id} is not among the parents")))
    };
    record.parents = record.parents.iter().map(map).collect::<Result<_>>()?;
    if let RecordKind::Resolve { chosen } = &record.kind {
        record.kind = RecordKind::Resolve {
            chosen: map(chosen)?,
        };
    }
    record::encode(&Entry::Record(record)).map_err(|e| error(e.0))
}

/// Writes the store into the empty directory `axon`.
fn write(files: &Files, axon: &Path) -> Result<()> {
    let records = axon.join("records");
    fs::create_dir(&records).map_err(|e| io(&records, e))?;
    let put = |path: PathBuf, bytes: &[u8]| fs::write(&path, bytes).map_err(|e| io(&path, e));
    for (id, bytes) in &files.records {
        let directory = records.join(id.subdirectory());
        if !directory.exists() {
            fs::create_dir(&directory).map_err(|e| io(&directory, e))?;
        }
        put(directory.join(id.as_ref()), bytes)?;
    }
    if let Some(bytes) = &files.gitignore {
        put(axon.join(".gitignore"), bytes)?;
    }
    if let Some(bytes) = &files.gitattributes {
        put(axon.join(".gitattributes"), bytes)?;
    }
    put(axon.join("header.json"), &files.header)
}

fn parse(bytes: &[u8]) -> Result<Map<String, Value>> {
    let value: Value = serde_json::from_slice(bytes).map_err(|e| error(e.to_string()))?;
    match value {
        Value::Object(map) => Ok(map),
        _ => Err(error("not an object")),
    }
}
fn ids_in(value: Option<Value>) -> Result<BTreeSet<String>> {
    match value {
        None => Ok(BTreeSet::new()),
        Some(Value::String(id)) => Ok(BTreeSet::from([id])),
        Some(Value::Array(items)) => items
            .into_iter()
            .map(|item| match item {
                Value::String(id) => Ok(id),
                _ => Err(error("a record ID is not a string")),
            })
            .collect(),
        Some(_) => Err(error("a record ID is not a string")),
    }
}
fn entity_of(row: &Map<String, Value>) -> Result<EntityId> {
    let entity = row
        .get("entity")
        .and_then(Value::as_str)
        .ok_or_else(|| error("no entity"))?;
    EntityId::try_from(entity.to_string()).map_err(|e| error(e.0))
}

/// Checks a converted store read back from disk against the v1 files and the labels file,
/// independently of how the conversion built it: the header keeps its store and prefix; every
/// output record decodes; each input record has exactly one output record that equals it in
/// everything but the label (`entity`, `record`, `operation`, `at`, `recorder`, `reason`,
/// `after`), with `parents` and `chosen` mapped through the new IDs; every record carries the
/// label the labels file gives its Entity, or `chore` when it has no line and every head of
/// the input is terminal; and every Note keeps its bytes and ID.
pub fn verify(input: &Files, labels: &Labels, converted: &Converted, output: &Files) -> Result<()> {
    let fail = |message: String| error(format!("the converted store does not match: {message}"));
    let header = read_header(&input.header)?;
    let written = record::decode_header(&output.header).map_err(|e| fail(e.0))?;
    if written != header {
        return Err(fail("the header's store or prefix changed".into()));
    }
    if output.gitignore != input.gitignore || output.gitattributes != input.gitattributes {
        return Err(fail(".gitignore or .gitattributes changed".into()));
    }
    let new: BTreeSet<&RecordId> = converted.ids.values().collect();
    let old: BTreeSet<&RecordId> = converted.ids.keys().collect();
    if old != input.records.keys().collect()
        || new != output.records.keys().collect()
        || new.len() != old.len()
    {
        return Err(fail("the records do not correspond one to one".into()));
    }

    // The labels each Entity must carry, from the input rows alone.
    let mut rows = BTreeMap::new();
    let mut continued = BTreeSet::new();
    for (id, bytes) in &input.records {
        let row = parse(bytes).map_err(|e| fail(format!("{id}: {e}")))?;
        if row.get("record").and_then(Value::as_str) != Some("note") {
            continued.extend(ids_in(row.get("parents").cloned()).map_err(|e| fail(e.0))?);
        }
        rows.insert(id.to_string(), row);
    }
    let heads = rows.iter().filter_map(|(id, row)| {
        if row.get("record").and_then(Value::as_str) == Some("note") {
            return None;
        }
        let terminal = matches!(
            row.get("after")
                .and_then(|after| after.get("lifecycle"))
                .and_then(Value::as_str),
            Some("completed" | "cancelled")
        );
        Some((entity_of(row), terminal, continued.contains(id)))
    });
    let mut entities = Vec::new();
    for (entity, terminal, is_continued) in heads {
        entities.push((entity.map_err(|e| fail(e.0))?, terminal, is_continued));
    }
    let expected = choose_labels(
        entities
            .iter()
            .map(|(entity, terminal, continued)| (entity, *terminal, *continued, "")),
        labels,
    )
    .map_err(|e| fail(e.0))?;

    let inverse: BTreeMap<String, String> = converted
        .ids
        .iter()
        .map(|(o, n)| (n.to_string(), o.to_string()))
        .collect();
    for (old_id, new_id) in &converted.ids {
        let before = &input.records[old_id];
        let after = &output.records[new_id];
        let at = |message: &str| fail(format!("{old_id} -> {new_id}: {message}"));
        let (decoded_id, _) = record::decode(after).map_err(|e| at(&e.0))?;
        if &decoded_id != new_id {
            return Err(at("the file name is not the hash of the bytes"));
        }
        let mut was = rows[old_id.as_ref()].clone();
        let mut is = parse(after).map_err(|e| at(&e.0))?;
        if was.get("record").and_then(Value::as_str) == Some("note") {
            if before != after || old_id != new_id {
                return Err(at("a Note changed"));
            }
            continue;
        }
        let entity = entity_of(&was).map_err(|e| at(&e.0))?;
        let label = is
            .get_mut("after")
            .and_then(Value::as_object_mut)
            .and_then(|after| after.remove("label"));
        let wanted = expected[&entity];
        if label.as_ref().and_then(Value::as_str) != Some(wanted.name()) {
            return Err(at(&format!("the label is not {wanted}")));
        }
        for key in ["parents", "chosen"] {
            let was_ids = ids_in(was.remove(key)).map_err(|e| at(&e.0))?;
            let is_ids = ids_in(is.remove(key)).map_err(|e| at(&e.0))?;
            let mapped: BTreeSet<String> = is_ids
                .iter()
                .map(|id| inverse.get(id).cloned().unwrap_or_default())
                .collect();
            if mapped != was_ids {
                return Err(at(&format!("{key} do not map to the earlier IDs")));
            }
        }
        if was != is {
            return Err(at("the content other than the label changed"));
        }
    }
    Ok(())
}

/// What `run` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub output: PathBuf,
    pub records: usize,
    pub notes: usize,
    pub entities: usize,
    /// Lines of the labels file for Entities the store does not hold: expected for another
    /// branch's Entities, or a mistyped ID.
    pub unused: Vec<EntityId>,
}

/// The absolute path with symlinks of its existing part resolved, so that two spellings of
/// one location compare equal before the rest exists.
fn resolved(path: &Path) -> Result<PathBuf> {
    let absolute = std::path::absolute(path).map_err(|e| io(path, e))?;
    let mut existing = absolute.as_path();
    let mut rest = Vec::new();
    loop {
        match fs::canonicalize(existing) {
            Ok(mut found) => {
                found.extend(rest.iter().rev());
                return Ok(found);
            }
            Err(e) if e.kind() == ErrorKind::NotFound => {
                let (Some(name), Some(parent)) = (existing.file_name(), existing.parent()) else {
                    return Err(io(path, e));
                };
                rest.push(name);
                existing = parent;
            }
            Err(e) => return Err(io(existing, e)),
        }
    }
}

/// The directory a conversion writes into before it is checked and renamed to `.axon`.
pub const UNCHECKED: &str = ".axon.unchecked";

/// Reads the v1 store of `source`, converts it with `labels`, and writes it to the new store
/// `output/.axon`. The store is written to `output/.axon.unchecked`, read back and checked
/// against the input, and renamed only when the check passes; otherwise it is removed.
/// `source` is not changed, and `output` may not lie inside its `.axon`.
pub fn run(source: &Path, labels: &Labels, output: &Path) -> Result<Summary> {
    let source_store = resolved(&source.join(".axon"))?;
    let target = resolved(&output.join(".axon"))?;
    let unchecked = resolved(&output.join(UNCHECKED))?;
    if target.starts_with(&source_store) || unchecked.starts_with(&source_store) {
        return Err(error(format!(
            "{} is inside the source store {}; write the output elsewhere",
            output.display(),
            source_store.display()
        )));
    }
    if fs::symlink_metadata(&target).is_ok() {
        return Err(error(format!(
            "{} already exists; the output is written only to a new store",
            target.display()
        )));
    }
    if fs::symlink_metadata(&unchecked).is_ok() {
        return Err(error(format!(
            "{} already exists: an interrupted or concurrent conversion left an unchecked store; delete it and do not use it",
            unchecked.display()
        )));
    }
    check_index(source)?;
    let input = read_files(source)?;
    let converted = convert(&input, labels)?;
    fs::create_dir_all(output).map_err(|e| io(output, e))?;
    // Only a directory this run created is removed on failure.
    fs::create_dir(&unchecked).map_err(|e| io(&unchecked, e))?;
    let checked = write(&converted.files, &unchecked)
        .and_then(|()| read_store(&unchecked))
        .and_then(|written| {
            if written != converted.files {
                return Err(error("the files read back differ from those written"));
            }
            verify(&input, labels, &converted, &written)
        })
        .and_then(|()| fs::rename(&unchecked, &target).map_err(|e| io(&target, e)));
    if let Err(e) = checked {
        return Err(match fs::remove_dir_all(&unchecked) {
            Ok(()) => e,
            Err(removal) if removal.kind() == ErrorKind::NotFound => e,
            Err(removal) => error(format!(
                "{e}\n{} holds an unchecked store that could not be removed ({removal}); delete it and do not use it",
                unchecked.display()
            )),
        });
    }
    Ok(Summary {
        output: target,
        records: converted.ids.len() - converted.notes,
        notes: converted.notes,
        entities: converted.labels.len(),
        unused: labels
            .ids()
            .filter(|id| !converted.labels.contains_key(*id))
            .cloned()
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_read_one_pair_per_line() {
        let labels = Labels::parse("# comment\n\n  a-1   bug \na-2 spike\n").unwrap();
        assert_eq!(
            labels.get(&EntityId::try_from("a-1".to_string()).unwrap()),
            Some(Label::Bug)
        );
        assert_eq!(
            labels.get(&EntityId::try_from("a-2".to_string()).unwrap()),
            Some(Label::Spike)
        );
        assert_eq!(
            labels.get(&EntityId::try_from("a-3".to_string()).unwrap()),
            None
        );
    }

    #[test]
    fn labels_refuse_unknown_values_duplicates_and_malformed_lines() {
        for (text, expected) in [
            ("a-1 fix", "line 1"),
            ("a-1 bug\na-1 bug", "more than once"),
            ("a-1", "ENTITY-ID LABEL"),
            ("a-1 bug extra", "ENTITY-ID LABEL"),
            ("\nA-1 bug", "line 2"),
        ] {
            let error = Labels::parse(text).unwrap_err().0;
            assert!(error.contains(expected), "{text:?}: {error}");
        }
    }

    #[test]
    fn the_label_goes_after_the_description_even_when_it_looks_like_json() {
        let description = "x\",\"condition\":null";
        let row = serde_json::json!({ "description": description }).to_string();
        let bytes = format!(
            "{{\"recorder\":{{\"data\":{{\"description\":\"d\"}}}},\"after\":{{\"description\":{},\"condition\":null}}}}\n",
            &row["{\"description\":".len()..row.len() - 1]
        );
        let labelled =
            String::from_utf8(with_label(bytes.as_bytes(), description, Label::Docs).unwrap())
                .unwrap();
        assert!(
            labelled.ends_with(",\"label\":\"docs\",\"condition\":null}}\n"),
            "{labelled}"
        );
        assert_eq!(labelled.matches("label").count(), 1);
    }
}
