//! Strict declaration YAML and canonical export over an immutable core snapshot.
use crate::lifecycle::{Entity, EntityId, Kind, Lifecycle, Snapshot};
use serde::{Deserialize, Deserializer};
use std::collections::BTreeSet;
use std::fmt::Write;

mod import;
pub use import::Checked;

pub const SCHEMA: &str = "axon-declaration/v1";
#[derive(Debug, thiserror::Error)]
#[error("Declaration: {0}")]
pub struct Error(pub String);
pub type Result<T> = std::result::Result<T, Error>;
fn invalid(message: impl Into<String>) -> Error {
    Error(message.into())
}
fn nullable<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> std::result::Result<Option<T>, D::Error> {
    Option::deserialize(d)
}

fn sequence<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> std::result::Result<Vec<T>, D::Error> {
    struct Sequence<T>(std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for Sequence<T> {
        type Value = Vec<T>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a sequence")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            let mut values = Vec::new();
            while let Some(value) = seq.next_element()? {
                values.push(value);
            }
            Ok(values)
        }
    }
    d.deserialize_any(Sequence(std::marker::PhantomData))
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Declaration {
    pub schema: String,
    #[serde(deserialize_with = "sequence")]
    pub groups: Vec<Record>,
    #[serde(deserialize_with = "sequence")]
    pub issues: Vec<Record>,
    #[serde(deserialize_with = "sequence")]
    pub references: Vec<External>,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    #[serde(deserialize_with = "nullable")]
    pub id: Option<String>,
    #[serde(deserialize_with = "nullable")]
    pub key: Option<String>,
    #[serde(deserialize_with = "nullable")]
    pub base: Option<String>,
    pub lifecycle: String,
    pub title: String,
    pub description: String,
    #[serde(deserialize_with = "nullable")]
    pub parent: Option<Reference>,
    #[serde(deserialize_with = "sequence")]
    pub needs: Vec<Reference>,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum Reference {
    Id(IdReference),
    Key(KeyReference),
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdReference {
    pub id: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyReference {
    pub key: String,
}
impl Reference {
    pub fn id(id: impl Into<String>) -> Self {
        Self::Id(IdReference { id: id.into() })
    }
    pub fn key(key: impl Into<String>) -> Self {
        Self::Key(KeyReference { key: key.into() })
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct External {
    pub id: String,
    pub kind: String,
    pub lifecycle: String,
    pub title: String,
}
pub fn lifecycle(value: Lifecycle) -> &'static str {
    match value {
        Lifecycle::Undecided => "undecided",
        Lifecycle::NotStarted => "not-started",
        Lifecycle::InProgress => "in-progress",
        Lifecycle::Completed => "completed",
        Lifecycle::Cancelled => "cancelled",
    }
}
pub fn kind(value: Kind) -> &'static str {
    match value {
        Kind::Issue => "issue",
        Kind::Group => "group",
    }
}
fn valid_lifecycle(value: &str) -> bool {
    matches!(
        value,
        "undecided" | "not-started" | "in-progress" | "completed" | "cancelled"
    )
}

/// Parse syntax and file-local constraints. Storage identities are checked against a snapshot separately.
pub fn parse(input: &str) -> Result<Declaration> {
    use granit_parser::{BufferedInput, Scanner, TokenType};
    for token in Scanner::new(BufferedInput::new(input.chars())) {
        match token
            .map_err(|e| invalid(format!("schema: {e}")))?
            .into_parts()
            .1
        {
            TokenType::Anchor(_)
            | TokenType::Alias(_)
            | TokenType::Tag(..)
            | TokenType::TagDirective(..) => {
                return Err(invalid("schema: anchors, aliases and tags are not allowed"));
            }
            _ => {}
        }
    }
    let options = serde_saphyr::options! {
        duplicate_keys: serde_saphyr::DuplicateKeyPolicy::Error,
        merge_keys: serde_saphyr::MergeKeyPolicy::Error,
        no_schema: true,
        reject_unsupported_tags: true,
    };
    #[derive(Deserialize)]
    struct SchemaProbe {
        schema: String,
    }
    let probe: SchemaProbe = serde_saphyr::from_str_with_options(input, options.clone())
        .map_err(|e| invalid(format!("schema: {e}")))?;
    if probe.schema != SCHEMA {
        return Err(invalid(format!(
            "schema: unsupported schema {}; expected {SCHEMA}; legacy axon-plan/v3 is an old-model format and is not converted",
            probe.schema
        )));
    }
    let value: Declaration = serde_saphyr::from_str_with_options(input, options)
        .map_err(|e| invalid(format!("schema: {e}")))?;
    value.validate()?;
    Ok(value)
}
impl Declaration {
    /// Assigned identities for records whose base has not yet been refreshed by apply.
    pub fn assigned_new_ids(&self) -> Vec<(String, String)> {
        self.records()
            .filter(|record| record.base.is_none())
            .filter_map(|record| record.key.clone().zip(record.id.clone()))
            .collect()
    }

    pub fn records(&self) -> impl Iterator<Item = &Record> {
        self.groups.iter().chain(&self.issues)
    }
    pub fn validate(&self) -> Result<()> {
        self.validate_local()
            .map_err(|e| invalid(format!("identity/reference: {}", e.0)))
    }
    fn validate_local(&self) -> Result<()> {
        if self.schema != SCHEMA {
            return Err(invalid(format!(
                "schema: unsupported schema {}; expected {SCHEMA}; legacy axon-plan/v3 is an old-model format and is not converted",
                self.schema
            )));
        }
        let mut ids = BTreeSet::new();
        let mut keys = BTreeSet::new();
        for record in self.records() {
            let label = record
                .id
                .as_deref()
                .or(record.key.as_deref())
                .unwrap_or("unassigned");
            if !valid_lifecycle(&record.lifecycle) || record.title.trim().is_empty() {
                return Err(invalid(format!(
                    "{label}: invalid lifecycle or empty title"
                )));
            }
            if let Some(id) = &record.id
                && (id.is_empty() || !ids.insert(id))
            {
                return Err(invalid(format!("{label}: empty or duplicate Entity ID")));
            }
            if let Some(key) = &record.key
                && (!valid_key(key) || !keys.insert(key))
            {
                return Err(invalid(format!("{label}: invalid or duplicate key")));
            }
            if let Some(base) = &record.base {
                if record.id.is_none() || !valid_base(base) {
                    return Err(invalid(format!(
                        "{label}: existing records require an ID and a blake3 fingerprint"
                    )));
                }
            } else if record.key.is_none()
                || !matches!(record.lifecycle.as_str(), "undecided" | "not-started")
            {
                return Err(invalid(
                    "new records require a key and undecided or not-started lifecycle",
                ));
            }
        }
        let mut external = BTreeSet::new();
        for r in &self.references {
            if r.id.is_empty()
                || !external.insert(&r.id)
                || ids.contains(&r.id)
                || !matches!(r.kind.as_str(), "group" | "issue")
                || !valid_lifecycle(&r.lifecycle)
                || r.title.trim().is_empty()
            {
                return Err(invalid("invalid or duplicate external reference"));
            }
        }
        for record in self.records() {
            let label = record
                .id
                .as_deref()
                .or(record.key.as_deref())
                .unwrap_or("unassigned");
            let own = record
                .id
                .as_ref()
                .map(|id| (0, id.clone()))
                .unwrap_or_else(|| (1, record.key.clone().unwrap()));
            let mut needs = BTreeSet::new();
            for reference in &record.needs {
                let target = self.target(reference)?;
                if target == own || !needs.insert(target) {
                    return Err(invalid(format!(
                        "{label}: self dependency or duplicate resolved dependency"
                    )));
                }
            }
            if let Some(parent) = &record.parent {
                let target = self.target(parent)?;
                if self.issues.iter().any(|r| self.record_target(r) == target)
                    || self
                        .references
                        .iter()
                        .any(|r| r.kind == "issue" && target == (0, r.id.clone()))
                {
                    return Err(invalid(format!("{label}: parent must be a Group")));
                }
            }
        }
        Ok(())
    }
    fn record_target(&self, r: &Record) -> (u8, String) {
        r.id.as_ref()
            .map(|id| (0, id.clone()))
            .unwrap_or_else(|| (1, r.key.clone().unwrap()))
    }
    fn target(&self, reference: &Reference) -> Result<(u8, String)> {
        match reference {
            Reference::Id(r) => {
                if r.id.is_empty() {
                    return Err(invalid("empty reference ID"));
                }
                Ok((0, r.id.clone()))
            }
            Reference::Key(r) => self
                .records()
                .find(|record| record.key.as_ref() == Some(&r.key))
                .map(|record| self.record_target(record))
                .ok_or_else(|| invalid(format!("unresolved key {}", r.key))),
        }
    }
    fn canonical_reference(&self, r: &Reference) -> Result<String> {
        let target = self.target(r)?;
        let key = self
            .records()
            .find(|record| self.record_target(record) == target)
            .and_then(|r| r.key.as_ref());
        Ok(if let Some(key) = key {
            format!("{{ key: {} }}", scalar(key, true))
        } else {
            format!("{{ id: {} }}", scalar(&target.1, true))
        })
    }
    /// Canonical order uses creation times from the same snapshot used for export or validation.
    pub fn serialize(&self, snapshot: &Snapshot) -> Result<String> {
        self.validate()?;
        let mut out = format!("schema: {SCHEMA}\n");
        for (label, records) in [("groups", &self.groups), ("issues", &self.issues)] {
            let mut ordered = Vec::new();
            for r in records {
                let created = if r.base.is_some() {
                    let id: EntityId =
                        r.id.clone()
                            .unwrap()
                            .try_into()
                            .map_err(|e: crate::lifecycle::Error| invalid(e.to_string()))?;
                    Some(
                        snapshot
                            .entity(&id)
                            .map_err(|e| invalid(e.to_string()))?
                            .created_at,
                    )
                } else {
                    None
                };
                ordered.push((r, created));
            }
            ordered.sort_by(|(a, ac), (b, bc)| match (ac, bc) {
                (Some(ac), Some(bc)) => ac.cmp(bc).then_with(|| a.id.cmp(&b.id)),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => a.key.cmp(&b.key),
            });
            writeln!(
                out,
                "{label}:{}",
                if ordered.is_empty() { " []" } else { "" }
            )
            .unwrap();
            for (r, _) in ordered {
                list_id(&mut out, r.id.as_deref());
                writeln!(out, "    key: {}", optional(&r.key)).unwrap();
                writeln!(
                    out,
                    "    base: {}",
                    r.base
                        .as_ref()
                        .map(|s| quote(s))
                        .unwrap_or_else(|| "null".into())
                )
                .unwrap();
                writeln!(out, "    lifecycle: {}", r.lifecycle).unwrap();
                field(&mut out, "title", &r.title, 4);
                field(&mut out, "description", &r.description, 4);
                writeln!(
                    out,
                    "    parent: {}",
                    r.parent
                        .as_ref()
                        .map(|p| self.canonical_reference(p))
                        .transpose()?
                        .unwrap_or_else(|| "null".into())
                )
                .unwrap();
                let mut needs = r
                    .needs
                    .iter()
                    .map(|n| Ok((self.target(n)?, n)))
                    .collect::<Result<Vec<_>>>()?;
                needs.sort_by(|(a, _), (b, _)| a.cmp(b));
                writeln!(
                    out,
                    "    needs:{}",
                    if needs.is_empty() { " []" } else { "" }
                )
                .unwrap();
                for (_, n) in needs {
                    writeln!(out, "      - {}", self.canonical_reference(n)?).unwrap();
                }
            }
        }
        let mut external: Vec<_> = self.references.iter().collect();
        external.sort_by(|a, b| a.id.cmp(&b.id));
        writeln!(
            out,
            "references:{}",
            if external.is_empty() { " []" } else { "" }
        )
        .unwrap();
        for r in external {
            list_id(&mut out, Some(&r.id));
            writeln!(out, "    kind: {}\n    lifecycle: {}", r.kind, r.lifecycle).unwrap();
            field(&mut out, "title", &r.title, 4);
        }
        Ok(out)
    }
}
fn valid_key(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && s.as_bytes()[0].is_ascii_lowercase()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}
fn valid_base(s: &str) -> bool {
    s.strip_prefix("blake3:").is_some_and(|s| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}
fn optional(s: &Option<String>) -> String {
    s.as_ref()
        .map(|s| scalar(s, false))
        .unwrap_or_else(|| "null".into())
}
fn list_id(out: &mut String, id: Option<&str>) {
    if let Some(id) = id {
        let mut rendered = String::new();
        field(&mut rendered, "id", id, 4);
        out.push_str("  - ");
        out.push_str(&rendered[4..]);
    } else {
        out.push_str("  - id: null\n");
    }
}
fn quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control()
                || matches!(
                    c,
                    '\u{85}' | '\u{2028}' | '\u{2029}' | '\u{fffe}' | '\u{ffff}'
                ) =>
            {
                write!(out, "\\u{:04x}", c as u32).unwrap();
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
fn scalar(s: &str, flow: bool) -> String {
    let forbidden = s.is_empty()
        || s.trim() != s
        || s.contains(['\n', '\r'])
        || s.chars().any(|c| {
            c.is_control()
                || matches!(
                    c,
                    '\u{85}' | '\u{2028}' | '\u{2029}' | '\u{fffe}' | '\u{ffff}'
                )
        })
        || s.starts_with([
            '-', '?', ':', ',', '[', ']', '{', '}', '#', '&', '*', '!', '|', '>', '\'', '"', '%',
            '@', '`',
        ])
        || s.contains(": ")
        || s.contains(" #")
        || (flow && s.contains([',', '[', ']', '{', '}']))
        || looks_typed(s);
    let plain = !forbidden
        && serde_saphyr::from_str_with_options::<serde_json::Value>(
            s,
            serde_saphyr::options! { no_schema: true },
        )
        .is_ok_and(|value| value.as_str() == Some(s));
    if plain { s.into() } else { quote(s) }
}
fn looks_typed(s: &str) -> bool {
    if matches!(
        s.to_ascii_lowercase().as_str(),
        "null"
            | "~"
            | "true"
            | "false"
            | "yes"
            | "no"
            | "on"
            | "off"
            | ".nan"
            | ".inf"
            | "+.inf"
            | "-.inf"
    ) {
        return true;
    }
    timestamp(s)
}
fn timestamp(s: &str) -> bool {
    fn digits<'a>(s: &mut &'a str, min: usize, max: usize) -> Option<&'a str> {
        let count = s.bytes().take_while(u8::is_ascii_digit).count();
        if !(min..=max).contains(&count) {
            return None;
        }
        let (value, rest) = s.split_at(count);
        *s = rest;
        Some(value)
    }
    fn parse(mut s: &str) -> Option<()> {
        digits(&mut s, 4, 4)?;
        s = s.strip_prefix('-')?;
        let month = digits(&mut s, 1, 2)?;
        s = s.strip_prefix('-')?;
        let day = digits(&mut s, 1, 2)?;
        if s.is_empty() {
            return (month.len() == 2 && day.len() == 2).then_some(());
        }
        if let Some(rest) = s.strip_prefix(['T', 't']) {
            s = rest;
        } else {
            let rest = s.trim_start_matches([' ', '\t']);
            if rest.len() == s.len() {
                return None;
            }
            s = rest;
        }
        digits(&mut s, 1, 2)?;
        s = s.strip_prefix(':')?;
        digits(&mut s, 2, 2)?;
        s = s.strip_prefix(':')?;
        digits(&mut s, 2, 2)?;
        if let Some(rest) = s.strip_prefix('.') {
            s = rest.trim_start_matches(|c: char| c.is_ascii_digit());
        }
        s = s.trim_start_matches([' ', '\t']);
        if s.is_empty() || s == "Z" {
            return Some(());
        }
        s = s.strip_prefix(['+', '-'])?;
        digits(&mut s, 1, 2)?;
        if let Some(rest) = s.strip_prefix(':') {
            s = rest;
            digits(&mut s, 2, 2)?;
        }
        s.is_empty().then_some(())
    }
    parse(s).is_some()
}

fn field(out: &mut String, name: &str, value: &str, indent: usize) {
    let lines: Vec<_> = value.split_terminator('\n').collect();
    let literal = value.contains('\n')
        && !value.contains('\r')
        && !value.ends_with("\n\n")
        && !value.chars().any(|c| {
            (c.is_control() && c != '\n')
                || matches!(
                    c,
                    '\u{85}' | '\u{2028}' | '\u{2029}' | '\u{fffe}' | '\u{ffff}'
                )
        })
        && lines
            .iter()
            .find(|l| !l.is_empty())
            .is_some_and(|l| !l.starts_with(char::is_whitespace))
        && lines.iter().all(|l| l.is_empty() || l.trim_end() == *l);
    if literal {
        writeln!(
            out,
            "{:indent$}{name}: {}",
            "",
            if value.ends_with('\n') { "|" } else { "|-" }
        )
        .unwrap();
        for line in lines {
            if line.is_empty() {
                out.push('\n');
            } else {
                writeln!(out, "{:width$}{line}", "", width = indent + 2).unwrap();
            }
        }
    } else {
        writeln!(out, "{:indent$}{name}: {}", "", scalar(value, false)).unwrap();
    }
}

pub fn fingerprint(entity: &Entity) -> String {
    fn token(hash: &mut blake3::Hasher, s: &str) {
        hash.update(&(s.len() as u64).to_be_bytes());
        hash.update(s.as_bytes());
    }
    let mut hash = blake3::Hasher::new();
    let c = &entity.current;
    for s in [
        SCHEMA,
        kind(entity.kind),
        &entity.id.to_string(),
        lifecycle(c.lifecycle),
        &c.title,
        &c.description,
    ] {
        token(&mut hash, s);
    }
    token(&mut hash, if c.parent.is_some() { "some" } else { "none" });
    if let Some(parent) = &c.parent {
        token(&mut hash, &parent.to_string());
    }
    hash.update(&(c.dependencies.len() as u64).to_be_bytes());
    for id in &c.dependencies {
        token(&mut hash, &id.to_string());
    }
    format!("blake3:{}", hash.finalize().to_hex())
}
/// Select the union of Issues and complete Group subtrees, including terminal descendants.
pub fn export(snapshot: &Snapshot, selectors: &[EntityId]) -> Result<Declaration> {
    if selectors.is_empty() {
        return Err(invalid("at least one selector is required"));
    }
    let mut selected = BTreeSet::new();
    let mut pending = selectors.to_vec();
    while let Some(id) = pending.pop() {
        if !selected.insert(id.clone()) {
            continue;
        }
        let entity = snapshot.entity(&id).map_err(|e| invalid(e.to_string()))?;
        if entity.kind == Kind::Group {
            pending.extend(
                snapshot
                    .entities()
                    .filter(|e| e.current.parent.as_ref() == Some(&id))
                    .map(|e| e.id.clone()),
            );
        }
    }
    let mut declaration = Declaration {
        schema: SCHEMA.into(),
        groups: vec![],
        issues: vec![],
        references: vec![],
    };
    let mut references = BTreeSet::new();
    for id in &selected {
        let entity = snapshot.entity(id).map_err(|e| invalid(e.to_string()))?;
        let c = &entity.current;
        references.extend(
            c.parent
                .iter()
                .chain(&c.dependencies)
                .filter(|id| !selected.contains(*id))
                .cloned(),
        );
        let record = Record {
            id: Some(id.to_string()),
            key: None,
            base: Some(fingerprint(entity)),
            lifecycle: lifecycle(c.lifecycle).into(),
            title: c.title.clone(),
            description: c.description.clone(),
            parent: c.parent.as_ref().map(|id| Reference::id(id.to_string())),
            needs: c
                .dependencies
                .iter()
                .map(|id| Reference::id(id.to_string()))
                .collect(),
        };
        match entity.kind {
            Kind::Group => declaration.groups.push(record),
            Kind::Issue => declaration.issues.push(record),
        }
    }
    for id in references {
        let e = snapshot.entity(&id).map_err(|e| invalid(e.to_string()))?;
        declaration.references.push(External {
            id: id.to_string(),
            kind: kind(e.kind).into(),
            lifecycle: lifecycle(e.current.lifecycle).into(),
            title: e.current.title.clone(),
        });
    }
    Ok(declaration)
}
pub fn example() -> Declaration {
    let record = |key: &str, title: &str, parent, needs| Record {
        id: None,
        key: Some(key.into()),
        base: None,
        lifecycle: "not-started".into(),
        title: title.into(),
        description: String::new(),
        parent,
        needs,
    };
    Declaration {
        schema: SCHEMA.into(),
        groups: vec![record("plan", "Deliver the plan", None, vec![])],
        issues: vec![
            record(
                "first",
                "Build the foundation",
                Some(Reference::key("plan")),
                vec![],
            ),
            record(
                "second",
                "Complete the work",
                Some(Reference::key("plan")),
                vec![Reference::key("first")],
            ),
        ],
        references: vec![],
    }
}

#[cfg(test)]
mod tests;
