use super::*;
use crate::lifecycle::record::{Context, Entry, Operation, RecordKind, Recorder, Store};
use chrono::{TimeZone, Utc};
use proptest::prelude::*;
fn id(s: &str) -> EntityId {
    s.to_owned().try_into().unwrap()
}
fn context() -> Context {
    Context {
        at: Utc.timestamp_opt(1000, 0).unwrap(),
        recorder: None,
    }
}
fn context_at(seconds: i64) -> Context {
    Context {
        at: Utc.timestamp_opt(seconds, 0).unwrap(),
        recorder: None,
    }
}
fn current(kind: Kind, title: &str) -> Current {
    Current {
        kind,
        lifecycle: Lifecycle::NotStarted,
        owner: None,
        title: title.into(),
        description: String::new(),
        label: crate::lifecycle::Label::Chore,
        condition: None,
        parent: None,
        needs: BTreeSet::new(),
    }
}
/// A store under a clock, so successive records of one Entity are distinct.
struct Fixture {
    store: Store,
    clock: std::cell::Cell<i64>,
}
impl Fixture {
    fn new() -> Self {
        Self {
            store: Store::new(),
            clock: std::cell::Cell::new(2000),
        }
    }
    fn tick(&self) -> Context {
        self.clock.set(self.clock.get() + 1);
        context_at(self.clock.get())
    }
    fn view(&self) -> View {
        self.store.view().unwrap()
    }
    fn create(&mut self, name: &str, kind: Kind) {
        self.create_at(name, kind, 1);
    }
    fn create_at(&mut self, name: &str, kind: Kind, seconds: i64) {
        let record = self
            .store
            .create(id(name), current(kind, name), context_at(seconds))
            .unwrap();
        self.store.insert(Entry::Record(record)).unwrap();
    }
    fn perform(&mut self, name: &str, operation: Operation) {
        let record = self
            .store
            .perform(&id(name), operation, None, self.tick())
            .unwrap();
        self.store.insert(Entry::Record(record)).unwrap();
    }
    fn write(&mut self, name: &str, title: &str) {
        let record = self
            .store
            .write(&id(name), Some(title.into()), None, self.tick())
            .unwrap()
            .unwrap();
        self.store.insert(Entry::Record(record)).unwrap();
    }
    fn set_parent(&mut self, name: &str, parent: Option<&str>) {
        let record = self
            .store
            .set_parent(&id(name), parent.map(id), self.tick())
            .unwrap()
            .unwrap();
        self.store.insert(Entry::Record(record)).unwrap();
    }
    fn add_dependency(&mut self, name: &str, target: &str) {
        let record = self
            .store
            .add_dependency(&id(name), &id(target), self.tick())
            .unwrap()
            .unwrap();
        self.store.insert(Entry::Record(record)).unwrap();
    }
    fn set_condition(&mut self, name: &str, command: &str) {
        let record = self
            .store
            .set_condition(&id(name), Some(command.into()), self.tick())
            .unwrap()
            .unwrap();
        self.store.insert(Entry::Record(record)).unwrap();
    }
    fn insert(&mut self, record: crate::lifecycle::record::Record) {
        self.store.insert(Entry::Record(record)).unwrap();
    }
    /// The declaration checked and its records added, as `axon import apply` publishes them.
    fn apply(&mut self, d: &Declaration) -> Checked {
        let input = d.serialize(&self.view()).unwrap();
        let checked = d.check(&input, &self.store, self.tick()).unwrap();
        for record in &checked.records {
            self.store.insert(Entry::Record(record.clone())).unwrap();
        }
        checked
    }
}
fn empty_view() -> View {
    Store::new().view().unwrap()
}
#[test]
fn new_example_is_canonical_and_has_one_dependency() {
    let d = example();
    let text = d.serialize(&empty_view()).unwrap();
    assert_eq!(parse(&text).unwrap(), d);
    assert_eq!(
        parse(&text).unwrap().serialize(&empty_view()).unwrap(),
        text
    );
    assert_eq!(d.groups.len(), 1);
    assert_eq!(d.issues.len(), 2);
    assert_eq!(d.records().map(|r| r.needs.len()).sum::<usize>(), 1);
    assert!(d.records().all(|r| r.id.is_none()
        && r.base.is_none()
        && r.key.is_some()
        && r.lifecycle == "not-started"));
}
#[test]
fn arbitrary_strings_round_trip_without_normalization() {
    let values = [
        "",
        "null",
        "Null",
        "~",
        "TRUE",
        "false",
        "yes",
        "no",
        "on",
        "off",
        "0",
        "01",
        "0x12",
        ".NaN",
        "1e999",
        "2026-01-01",
        "2026-01-01T01:02:03Z",
        "  leading",
        "trailing  ",
        "a:",
        "a: b",
        "a # b",
        "# header",
        "a,b",
        "a[b]",
        "a{b}",
        "a\rb",
        "a\u{0}b",
        "a\u{7}b",
        "a\u{85}b",
        "a\u{2028}b",
        "a\u{2029}b",
        "a\tb",
        "a\nb",
        "a\nb\n",
        "a\nb\n\n",
        "\n\nfirst\nsecond",
        "\n indented\nnext",
        "first\n \nnext",
        "first\nlast \n",
        "\n",
        "\n\n",
        "a\u{fffe}b",
        "a\u{ffff}b",
        "a\n\u{fffe}b",
        "a\n\u{ffff}b\n",
        "é 日本語 🚀",
        "a\\b\"c",
        "---",
        "...",
        ":",
        "%tag",
    ];
    for value in values {
        let mut d = example();
        d.groups[0].description = value.into();
        if !value.trim().is_empty() {
            d.groups[0].title = value.into();
        }
        let yaml = d.serialize(&empty_view()).unwrap();
        assert_eq!(
            parse(&yaml).unwrap_or_else(|e| panic!("{value:?}: {e}\n{yaml}")),
            d,
            "{value:?}"
        );
    }
    for value in ['\0', '\u{1}', '\u{1f}', '\u{7f}', '\u{9f}', '\u{a0}'] {
        let mut d = example();
        d.issues[0].description = format!("a{value}b");
        assert_eq!(parse(&d.serialize(&empty_view()).unwrap()).unwrap(), d);
    }
}
#[test]
fn canonical_scalar_styles_and_literal_chomping() {
    assert_eq!(scalar("normal", false), "normal");
    for value in [
        "0xray",
        "0ocean",
        "0binary",
        "2026-ab-cd",
        "2026-01-01suffix",
        "2026-1-1",
    ] {
        assert_eq!(scalar(value, false), value);
    }
    for value in [
        "0x12",
        "0o12",
        "0b10",
        "12.5",
        "2026-01-01",
        "2001-12-15T02:59:43.1Z",
        "2001-12-14t21:59:43.10-05:00",
        "2001-12-14 21:59:43.10 -5",
        "2001-12-14 21:59:43.10",
    ] {
        assert_eq!(scalar(value, false), quote(value));
    }
    assert_eq!(scalar("a,b", false), "a,b");
    assert_eq!(scalar("a,b", true), "\"a,b\"");
    assert_eq!(scalar("null", false), "\"null\"");
    for (value, expected) in [
        ("one\ntwo", "    description: |-\n      one\n      two\n"),
        ("one\ntwo\n", "    description: |\n      one\n      two\n"),
        (
            "one\n\ntwo",
            "    description: |-\n      one\n\n      two\n",
        ),
    ] {
        let mut out = String::new();
        field(&mut out, "description", value, 4);
        assert_eq!(out, expected);
    }
}
#[test]
fn strict_yaml_rejects_unsupported_constructs_and_wrong_types() {
    let text = example().serialize(&empty_view()).unwrap();
    let invalid = [
        text.replace(
            "schema: axon-declaration/v2",
            "schema: axon-declaration/v2\nunknown: true",
        ),
        text.replace(
            "schema: axon-declaration/v2",
            "schema: axon-declaration/v2\nschema: axon-declaration/v2",
        ),
        text.replace("title: Deliver the plan", "title: &title Deliver the plan"),
        text.replace("title: Deliver the plan", "title: *title"),
        text.replace("title: Deliver the plan", "title: !custom Deliver the plan"),
        text.replace("title: Deliver the plan", "title: !!str Deliver the plan"),
        text.replace("title: Deliver the plan", "title: 123"),
        text.replace("title: Deliver the plan", "title: true"),
        text.replace("title: Deliver the plan", "title: null"),
        text.replace("description: \"\"", "description: []"),
        text.replace("    key: plan\n", ""),
        text.replace("    needs: []", "    needs: null"),
        text.replace("    needs: []", "    needs: []\n    <<: {}"),
        text.replace(
            "parent: { key: plan }",
            "parent: { key: plan, id: some-id }",
        ),
        text.replace("parent: { key: plan }", "parent: {}"),
        text.replace("parent: { key: plan }", "parent: plan"),
        text.replace("parent: { key: plan }", "parent: { key: missing }"),
        text.replace("parent: { key: plan }", "parent: { key: first }"),
        text.replace("      - { key: first }", "      - { key: second }"),
        text.replace(
            "      - { key: first }",
            "      - { key: first }\n      - { key: first }",
        ),
        text.replace("key: second", "key: first"),
        format!("{text}---\n{text}"),
        "[]\n".into(),
    ];
    for bad in invalid {
        assert!(parse(&bad).is_err(), "accepted {bad}");
    }
    for schema in ["other/v1", "future/v2"] {
        let e = parse(&format!("schema: {schema}\n"))
            .unwrap_err()
            .to_string();
        assert!(e.contains(schema) && e.contains(SCHEMA), "{e}");
    }
    // A v1 file, with or without labels, is not converted: its bases no longer match.
    for v1 in [
        text.replace("schema: axon-declaration/v2", "schema: axon-declaration/v1"),
        text.replace("schema: axon-declaration/v2", "schema: axon-declaration/v1")
            .replace("    label: feat\n", ""),
    ] {
        let e = parse(&v1).unwrap_err().to_string();
        assert!(
            e.contains("axon-declaration/v1")
                && e.contains("axon export")
                && e.contains(&format!("add a label to each and declare {SCHEMA}")),
            "{e}"
        );
    }
}
#[test]
fn label_is_required_and_limited_to_the_fixed_set() {
    let text = example().serialize(&empty_view()).unwrap();
    assert_eq!(text.matches("    label: feat\n").count(), 3);
    let group = "    title: Deliver the plan\n    description: \"\"\n    label: feat\n";
    assert!(text.contains(group), "{text}");
    let with = |line: &str| {
        text.replacen(
            group,
            &format!("    title: Deliver the plan\n    description: \"\"\n{line}"),
            1,
        )
    };
    let missing = with("");
    assert!(parse(&missing).is_err(), "accepted {missing}");
    for value in [
        "null", "~", "\"\"", "Bug", "fix", "\" bug\"", "[]", "{}", "1", "bug feat",
    ] {
        let bad = with(&format!("    label: {value}\n"));
        let e = parse(&bad).expect_err(&bad).to_string();
        assert!(e.starts_with("Declaration: schema:"), "{value}: {e}");
    }
    let unknown = with("    label: fix\n");
    let e = parse(&unknown).unwrap_err().to_string();
    assert!(
        e.contains("plan") && e.contains("label") && e.contains("spike"),
        "{e}"
    );
    for label in crate::lifecycle::Label::ALL {
        let mut d = example();
        d.groups[0].label = label.name().into();
        let yaml = d.serialize(&empty_view()).unwrap();
        assert!(yaml.contains(&format!("    label: {}\n", label.name())));
        assert_eq!(parse(&yaml).unwrap(), d);
    }
    // A declaration built in code is held to the same set.
    let mut d = example();
    d.issues[1].label = "fix".into();
    assert!(d.serialize(&empty_view()).is_err());
}
#[test]
fn canonical_order_uses_creation_then_ids_then_keys_and_resolved_needs() {
    let mut f = Fixture::new();
    for (name, sec) in [("p-z", 1), ("p-b", 2), ("p-a", 2)] {
        f.create_at(name, Kind::Issue, sec);
    }
    let view = f.view();
    let mut d = export(&f.store, &view, &[id("p-a"), id("p-z"), id("p-b")]).unwrap();
    d.issues[0].key = Some("alias".into());
    d.issues.extend(example().issues.into_iter().map(|mut r| {
        r.parent = None;
        r.needs = vec![];
        r
    }));
    d.issues.last_mut().unwrap().needs = vec![
        Reference::key("first"),
        Reference::id("p-z"),
        Reference::key("alias"),
    ];
    let yaml = d.serialize(&view).unwrap();
    let parsed = parse(&yaml).unwrap();
    assert_eq!(
        parsed
            .issues
            .iter()
            .map(|r| r.id.as_deref().unwrap_or(r.key.as_deref().unwrap_or("")))
            .collect::<Vec<_>>(),
        ["p-z", "p-a", "p-b", "first", "second"]
    );
    assert_eq!(
        parsed.issues.last().unwrap().needs,
        vec![
            Reference::key("alias"),
            Reference::id("p-z"),
            Reference::key("first")
        ]
    );
    assert_eq!(parsed.serialize(&view).unwrap(), yaml);
    d.issues
        .last_mut()
        .unwrap()
        .needs
        .push(Reference::id("p-a"));
    assert!(d.serialize(&view).is_err());
}
#[test]
fn fingerprint_tokens_and_visible_field_changes() {
    let mut f = Fixture::new();
    f.create("demo-a", Kind::Issue);
    f.write("demo-a", "日本語");
    let view = f.view();
    let e = view.current(&id("demo-a")).unwrap().clone();
    let mut bytes = Vec::new();
    for token in [
        "axon-declaration/v2",
        "issue",
        "demo-a",
        "not-started",
        "日本語",
        "",
        "chore",
        "none",
    ] {
        bytes.extend((token.len() as u64).to_be_bytes());
        bytes.extend(token.as_bytes());
    }
    bytes.extend(0u64.to_be_bytes());
    let base = fingerprint(&id("demo-a"), &e);
    assert_eq!(base, format!("blake3:{}", blake3::hash(&bytes).to_hex()));
    for change in 0..8 {
        let mut other = e.clone();
        let mut other_id = id("demo-a");
        match change {
            0 => other.title.push('!'),
            1 => other.description.push('!'),
            2 => other.lifecycle = Lifecycle::Completed,
            3 => other.parent = Some(id("parent")),
            4 => {
                other.needs.insert(id("needs"));
            }
            5 => other.kind = Kind::Group,
            6 => other.label = crate::lifecycle::Label::Bug,
            _ => other_id = id("other"),
        }
        assert_ne!(base, fingerprint(&other_id, &other));
    }
    // Conditions are not part of the fingerprint.
    f.set_condition("demo-a", "exit 1");
    let view = f.view();
    assert_eq!(
        base,
        fingerprint(&id("demo-a"), view.current(&id("demo-a")).unwrap())
    );
}
#[test]
fn declaration_doc_example_has_identical_canonical_bytes() {
    let doc = include_str!("../../../../docs/reference/declaration.md");
    let text = doc
        .split("```yaml\nschema: axon-declaration/v2\n")
        .nth(1)
        .unwrap()
        .split("```\n")
        .next()
        .unwrap();
    let text = format!("schema: axon-declaration/v2\n{text}");
    let d = parse(&text).unwrap();
    let mut f = Fixture::new();
    f.create("demo-k3m7pq", Kind::Group);
    for name in ["demo-8bxw2r", "demo-c9d4ts"] {
        f.create(name, Kind::Issue);
    }
    assert_eq!(d.serialize(&f.view()).unwrap(), text);
}

fn declaration_text() -> impl Strategy<Value = String> {
    proptest::string::string_regex("[a-zA-Z0-9 #,:\\[\\]{}\\t\\n]{0,60}").unwrap()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn generated_declarations_parse_and_rewrite_canonically(
        title in "[a-zA-Z][a-zA-Z0-9]{0,30}",
        description in declaration_text(),
        key in "k[a-z0-9-]{0,20}",
        reverse in any::<bool>(),
    ) {
        for value in [description.as_str(), "null", "TRUE", "2026-01-01", "0x12", "é 日本語 🚀", "line\r\nnext", "line\nnext\n", "\u{2028}\u{2029}\0"] {
            let mut d = example();
            d.groups[0].title = title.clone();
            d.groups[0].description = value.into();
            d.issues[0].key = Some(key.clone());
            d.issues[1].needs = vec![Reference::key(&key)];
            if reverse { d.issues.reverse(); }
            let yaml = d.serialize(&empty_view()).unwrap();
            let parsed = parse(&yaml).unwrap();
            prop_assert_eq!(parsed.serialize(&empty_view()).unwrap(), yaml);
            prop_assert_eq!(parsed.groups[0].description.as_str(), value);
            prop_assert_eq!(parsed.issues.iter().map(|issue| issue.key.as_deref()).collect::<Vec<_>>(), [Some(key.as_str()), Some("second")]);
            prop_assert_eq!(parsed.issues.iter().map(|issue| issue.needs.len()).sum::<usize>(), 1);
        }
    }

    #[test]
    fn generated_yaml_damage_is_rejected(
        title in "[a-zA-Z]{1,12}",
    ) {
        let mut d = example();
        d.groups[0].title = title;
        let yaml = d.serialize(&empty_view()).unwrap();
        for field in ["schema", "groups", "issues", "references"] {
            let line = yaml.lines().find(|line| line.starts_with(&format!("{field}:"))).unwrap();
            // The whole value is replaced, so a block sequence under the key does not turn
            // the damage into a syntax error.
            let block: String = yaml
                .lines()
                .skip_while(|l| *l != line)
                .enumerate()
                .take_while(|(i, l)| *i == 0 || l.is_empty() || l.starts_with(' '))
                .map(|(_, l)| format!("{l}\n"))
                .collect();
            for replacement in ["null", "true", "123", "{}"] {
                let bad = yaml.replacen(&block, &format!("{field}: {replacement}\n"), 1);
                let result = parse(&bad);
                prop_assert!(result.is_err(), "{bad}");
                let error = result.unwrap_err().to_string();
                // The lists are rejected by their type, not by the text left around them.
                prop_assert!(
                    field == "schema" || error.contains("expected a sequence"),
                    "{error}\n{bad}"
                );
            }
            let duplicate = yaml.replacen(line, &format!("{line}\n{line}"), 1);
            prop_assert!(parse(&duplicate).is_err(), "{duplicate}");
        }
    }

    #[test]
    fn generated_record_field_damage_is_rejected(
        title in "[a-zA-Z]{1,12}",
    ) {
        let mut d = example();
        d.groups[0].title = title;
        let yaml = d.serialize(&empty_view()).unwrap();
        for field in ["key", "lifecycle", "title", "description", "label", "parent", "needs"] {
            let line = yaml.lines().find(|line| line.starts_with(&format!("    {field}:"))).unwrap();
            for damage in ["missing", "duplicate", "wrong-type"] {
                let bad = match damage {
                    "missing" => yaml.replacen(&format!("{line}\n"), "", 1),
                    "duplicate" => yaml.replacen(line, &format!("{line}\n{line}"), 1),
                    _ => yaml.replacen(line, &format!("    {field}: {{ bad: value }}"), 1),
                };
                prop_assert!(parse(&bad).is_err(), "{field}/{damage}: {bad}");
            }
        }
    }

    #[test]
    fn generated_visible_fingerprint_fields_change_it(
        title in "[a-zA-Z]{1,20}",
        description in declaration_text(),
    ) {
        for selector in 0..8 {
            let mut value = current(Kind::Issue, &title);
            value.description = description.clone();
            let entity = id("demo-a");
            let base = fingerprint(&entity, &value);
            let mut other_id = entity.clone();
            match selector {
                0 => value.title.push('!'),
                1 => value.description.push('!'),
                2 => value.lifecycle = Lifecycle::Completed,
                3 => value.parent = Some(id("parent")),
                4 => { value.needs.insert(id("needs")); },
                5 => value.kind = Kind::Group,
                6 => value.label = crate::lifecycle::Label::Spike,
                _ => other_id = id("other"),
            }
            prop_assert_ne!(base, fingerprint(&other_id, &value), "selector {}", selector);
            let unchanged = fingerprint(&other_id, &value);
            value.condition = Some("exit 1".into());
            prop_assert_eq!(unchanged, fingerprint(&other_id, &value), "selector {}", selector);
        }
    }

    #[test]
    fn generated_notes_and_recorders_do_not_change_export(
        actor in "[a-z]{1,12}",
        note in "[a-zA-Z0-9]{1,30}",
    ) {
        let entity = id("demo-a");
        let value = current(Kind::Issue, "title");
        let mut store = Store::new();
        let plain = store.create(entity.clone(), value.clone(), context()).unwrap();
        let mut baseline = Store::new();
        baseline.insert(Entry::Record(plain)).unwrap();
        let expected = export(&baseline, &baseline.view().unwrap(), std::slice::from_ref(&entity)).unwrap();
        let record = store.create(entity.clone(), value, Context {
            at: context().at,
            recorder: Some(Recorder { actor, data: Default::default() }),
        }).unwrap();
        store.insert(Entry::Record(record)).unwrap();
        prop_assert_eq!(export(&store, &store.view().unwrap(), std::slice::from_ref(&entity)).unwrap(), expected.clone());
        let note = store.add_note(&entity, note, None, context_at(1001)).unwrap();
        store.insert(Entry::Note(note)).unwrap();
        prop_assert_eq!(export(&store, &store.view().unwrap(), &[entity]).unwrap(), expected);
    }
}

#[test]
fn prepared_plan_checks_retries_and_collisions_without_partial_matches() {
    let mut f = Fixture::new();
    let mut d = example();
    d.prepare(&f.store, "demo").unwrap();
    let yaml = d.serialize(&f.view()).unwrap();
    let checked = d.check(&yaml, &f.store, context()).unwrap();
    assert_eq!(checked.records.len(), 3);
    assert!(
        checked
            .records
            .iter()
            .all(|r| r.kind == RecordKind::Created)
    );
    // Records are published parents and dependencies first.
    let order: Vec<_> = checked
        .records
        .iter()
        .map(|r| r.entity.to_string())
        .collect();
    let position = |key: &str| {
        order
            .iter()
            .position(|e| {
                d.records()
                    .find(|r| r.key.as_deref() == Some(key))
                    .unwrap()
                    .id
                    .as_deref()
                    == Some(e.as_str())
            })
            .unwrap()
    };
    assert!(position("plan") < position("first") && position("first") < position("second"));
    let applied = f.apply(&d);
    assert_eq!(applied.records.len(), 3);
    assert_eq!(applied.after.view().unwrap().known().count(), 3);
    let retried = d.check(&yaml, &f.store, context()).unwrap();
    assert!(retried.already_applied && retried.records.is_empty());
    // A conflict elsewhere in the store rejects even a completed retry.
    let mut other = Fixture {
        store: f.store.clone(),
        clock: std::cell::Cell::new(7000),
    };
    other.create("elsewhere", Kind::Issue);
    other.perform("elsewhere", Operation::Withdraw);
    f.create("elsewhere", Kind::Issue);
    f.perform("elsewhere", Operation::Cancel);
    let mut conflicted = f.store.clone();
    conflicted.absorb(&other.store);
    assert!(conflicted.view().unwrap().is_conflicted(&id("elsewhere")));
    let error = d
        .check(&yaml, &conflicted, context())
        .unwrap_err()
        .to_string();
    assert!(error.contains("conflicted"), "{error}");
    // An export that was never edited is not "already applied": nothing was applied.
    let unedited = export(&f.store, &f.view(), &[applied.records[0].entity.clone()]).unwrap();
    let checked = unedited
        .check(&unedited.serialize(&f.view()).unwrap(), &f.store, context())
        .unwrap();
    assert!(!checked.already_applied && checked.records.is_empty());
    // An ID that only Notes refer to is taken as well.
    let note = f
        .store
        .add_note(&id("demo-noted"), "orphan".into(), None, context())
        .map(Entry::Note);
    assert!(note.is_err(), "a Note needs a record; insert one directly");
    let orphan = crate::lifecycle::record::Note {
        entity: id("demo-noted"),
        nonce: crate::lifecycle::record::Nonce::generate(),
        at: context().at,
        recorder: None,
        reason: None,
        body: "orphan".into(),
    };
    f.store.insert(Entry::Note(orphan)).unwrap();
    let mut with_noted = example();
    with_noted.groups[0].id = Some("demo-noted".into());
    with_noted.prepare(&f.store, "demo").unwrap();
    assert_ne!(with_noted.groups[0].id.as_deref(), Some("demo-noted"));
    let mut retry = d.clone();
    retry.prepare(&f.store, "demo").unwrap();
    assert_eq!(retry, d);
    let first = d.issues[0].id.as_ref().unwrap().clone();
    f.write(&first, "Concurrent");
    assert!(
        d.check(&yaml, &f.store, context())
            .unwrap_err()
            .to_string()
            .contains("conflict:")
    );
    // The edited Issue no longer holds the declared value, so its ID is taken by another
    // writer as far as the declaration knows; the Issue that depends on it follows, and the
    // Group that still holds its value keeps its ID.
    retry.prepare(&f.store, "demo").unwrap();
    assert_eq!(retry.groups[0].id, d.groups[0].id);
    assert!(
        retry
            .issues
            .iter()
            .zip(&d.issues)
            .all(|(a, b)| a.id != b.id)
    );
    let checked = retry
        .check(&retry.serialize(&f.view()).unwrap(), &f.store, context())
        .unwrap();
    assert_eq!(checked.records.len(), 2);
    f.apply(&retry);
    assert_eq!(f.view().known().count(), 6);
}

#[test]
fn applying_a_declaration_keeps_the_label_of_an_existing_entity() {
    let mut f = Fixture::new();
    f.create("a", Kind::Issue);
    let record = f
        .store
        .set_label(&id("a"), crate::lifecycle::Label::Bug, f.tick())
        .unwrap()
        .unwrap();
    f.insert(record);
    let mut d = export(&f.store, &f.view(), &[id("a")]).unwrap();
    let input = d.serialize(&f.view()).unwrap();
    let unchanged = d.check(&input, &f.store, f.tick()).unwrap();
    assert!(unchanged.records.is_empty());
    d.issues[0].title = "renamed".into();
    let checked = f.apply(&d);
    assert_eq!(checked.records.len(), 1);
    assert_eq!(checked.records[0].kind, RecordKind::Import);
    assert_eq!(checked.records[0].after.label, crate::lifecycle::Label::Bug);
}

#[test]
fn an_edited_label_is_applied_as_one_import_record_and_a_new_entity_takes_its_label() {
    use crate::lifecycle::Label;
    let mut f = Fixture::new();
    f.create("g", Kind::Group);
    f.create("a", Kind::Issue);
    f.set_parent("a", Some("g"));
    let mut d = export(&f.store, &f.view(), &[id("g")]).unwrap();
    assert!(d.records().all(|r| r.label == "chore"));
    let before = d.clone();
    // Only the label differs: the fingerprint still matches and the change is one record.
    d.issues[0].label = "bug".into();
    let input = d.serialize(&f.view()).unwrap();
    assert_eq!(
        input,
        before.serialize(&f.view()).unwrap().replacen(
            "label: chore\n    parent: { id: g }",
            "label: bug\n    parent: { id: g }",
            1
        )
    );
    let checked = f.apply(&d);
    assert_eq!(checked.records.len(), 1);
    let record = &checked.records[0];
    assert_eq!(
        (
            record.entity.clone(),
            record.kind.clone(),
            record.after.label
        ),
        (id("a"), RecordKind::Import, Label::Bug)
    );
    let mut stored = f.view().current(&id("a")).unwrap().clone();
    assert_eq!(stored.label, Label::Bug);
    stored.label = Label::Chore;
    assert_eq!(
        before.issues[0].base.as_deref(),
        Some(fingerprint(&id("a"), &stored).as_str()),
        "the base of the export is the value before the label changed"
    );
    // The applied file is fully applied; the pre-edit export now conflicts on the label.
    let done = d.check(&input, &f.store, f.tick()).unwrap();
    assert!(done.already_applied && done.records.is_empty());
    let stale = before.serialize(&f.view()).unwrap();
    let e = before
        .check(&stale, &f.store, f.tick())
        .unwrap_err()
        .to_string();
    assert!(e.contains("conflict: a"), "{e}");
    // A label and a title change together are still one record, and a new Entity is created
    // with its declared label.
    let mut d = export(&f.store, &f.view(), &[id("g")]).unwrap();
    d.groups[0].label = "spike".into();
    d.groups[0].title = "renamed".into();
    let mut new = example().issues.remove(0);
    new.key = Some("added".into());
    new.label = "docs".into();
    new.parent = Some(Reference::id("g"));
    d.issues.push(new);
    d.prepare(&f.store, "demo").unwrap();
    let checked = f.apply(&d);
    assert_eq!(checked.records.len(), 2);
    let created = checked
        .records
        .iter()
        .find(|r| r.kind == RecordKind::Created)
        .unwrap();
    assert_eq!(created.after.label, Label::Docs);
    let imported = checked
        .records
        .iter()
        .find(|r| r.kind == RecordKind::Import)
        .unwrap();
    assert_eq!(
        (
            imported.entity.clone(),
            imported.after.label,
            imported.after.title.as_str()
        ),
        (id("g"), Label::Spike, "renamed")
    );
}

#[test]
fn a_terminal_entity_keeps_its_label() {
    let mut f = Fixture::new();
    f.create("a", Kind::Issue);
    f.perform("a", Operation::Cancel);
    let mut d = export(&f.store, &f.view(), &[id("a")]).unwrap();
    d.issues[0].label = "bug".into();
    let input = d.serialize(&f.view()).unwrap();
    let e = d.check(&input, &f.store, f.tick()).unwrap_err().to_string();
    assert!(e.contains("core rejection: a: label"), "{e}");
}

#[test]
fn partial_publication_is_completed_by_a_retry_without_duplicate_records() {
    let mut f = Fixture::new();
    f.create("a", Kind::Issue);
    f.create("b", Kind::Issue);
    f.add_dependency("a", "b");
    let mut d = export(&f.store, &f.view(), &[id("a"), id("b")]).unwrap();
    d.issues[0].needs.clear();
    d.issues[1].needs.push(Reference::id("a"));
    let input = d.serialize(&f.view()).unwrap();
    let checked = d.check(&input, &f.store, context()).unwrap();
    assert_eq!(checked.records.len(), 2);
    assert!(checked.records.iter().all(|r| r.kind == RecordKind::Import));
    // Only the first record was published: the store is in between, valid but not final.
    f.store
        .insert(Entry::Record(checked.records[0].clone()))
        .unwrap();
    let retried = d.check(&input, &f.store, context()).unwrap();
    assert!(!retried.already_applied);
    assert_eq!(retried.records.len(), 1);
    assert_eq!(retried.records[0].entity, checked.records[1].entity);
    f.store
        .insert(Entry::Record(retried.records[0].clone()))
        .unwrap();
    let done = d.check(&input, &f.store, context()).unwrap();
    assert!(done.already_applied && done.records.is_empty());
    assert!(f.view().is_valid());
    assert_eq!(f.store.len(), 5);
    // The same file with a fresh context adds no record either.
    let again = d.check(&input, &f.store, context_at(9000)).unwrap();
    assert!(again.already_applied && again.records.is_empty());
    // A violation another writer adds afterwards does not stop the retry the interrupted
    // apply asked for: the declaration adds nothing to the store, so it is still applied.
    f.create("p", Kind::Group);
    f.create("q", Kind::Group);
    let mut other = Fixture {
        store: f.store.clone(),
        clock: std::cell::Cell::new(7000),
    };
    other.set_parent("p", Some("q"));
    f.set_parent("q", Some("p"));
    f.store.absorb(&other.store);
    assert!(!f.view().violations().is_empty());
    let again = d.check(&input, &f.store, context_at(9500)).unwrap();
    assert!(again.already_applied && again.records.is_empty());
    d.clone().prepare(&f.store, "demo").unwrap();
}

#[test]
fn check_core_constraints_and_references() {
    let mut f = Fixture::new();
    for (name, kind) in [
        ("g", Kind::Group),
        ("h", Kind::Group),
        ("a", Kind::Issue),
        ("b", Kind::Issue),
    ] {
        f.create(name, kind);
    }
    let reject = |d: &Declaration, f: &Fixture, field: &str| {
        let bytes = d.serialize(&f.view()).unwrap();
        let error = d
            .check(&bytes, &f.store, context())
            .unwrap_err()
            .to_string();
        assert!(error.contains(field), "{field}: {error}");
    };
    let mut d = export(&f.store, &f.view(), &[id("a"), id("b")]).unwrap();
    d.issues[0].needs.push(Reference::id("b"));
    d.issues[1].needs.push(Reference::id("a"));
    reject(&d, &f, "core rejection: b: needs");
    let mut d = export(&f.store, &f.view(), &[id("g"), id("h")]).unwrap();
    d.groups[0].parent = Some(Reference::id("h"));
    d.groups[1].parent = Some(Reference::id("g"));
    reject(&d, &f, "parent");
    f.perform("a", Operation::Start);
    // InProgress work moves only under adopted Groups.
    f.perform("g", Operation::Withdraw);
    let mut d = export(&f.store, &f.view(), &[id("a")]).unwrap();
    d.issues[0].parent = Some(Reference::id("g"));
    d.refresh_references(&f.view()).unwrap();
    reject(&d, &f, "parent");
    f.perform("g", Operation::Accept);
    f.perform("a", Operation::Complete);
    for field in ["title", "description", "needs"] {
        let mut d = export(&f.store, &f.view(), &[id("a")]).unwrap();
        match field {
            "title" => d.issues[0].title = "Changed".into(),
            "description" => d.issues[0].description = "Changed".into(),
            _ => d.issues[0].needs.push(Reference::id("b")),
        }
        d.refresh_references(&f.view()).unwrap();
        reject(&d, &f, field);
    }
    // A terminal Entity exported and applied as it is passes: its text is not edited.
    let d = export(&f.store, &f.view(), &[id("a")]).unwrap();
    let checked = d
        .check(&d.serialize(&f.view()).unwrap(), &f.store, context())
        .unwrap();
    assert!(checked.records.is_empty() && !checked.already_applied);
    f.set_parent("a", Some("g"));
    f.perform("g", Operation::Complete);
    let mut d = export(&f.store, &f.view(), &[id("a")]).unwrap();
    d.issues[0].parent = None;
    d.refresh_references(&f.view()).unwrap();
    reject(&d, &f, "parent");
    let mut d = export(&f.store, &f.view(), &[id("b")]).unwrap();
    d.issues[0].parent = Some(Reference::id("g"));
    d.refresh_references(&f.view()).unwrap();
    reject(&d, &f, "parent");
    let mut d = export(&f.store, &f.view(), &[id("b")]).unwrap();
    d.issues[0].needs.push(Reference::id("a"));
    d.refresh_references(&f.view()).unwrap();
    d.references[0].title = "Stale title".into();
    d.references[0].lifecycle = "undecided".into();
    d.check(&d.serialize(&f.view()).unwrap(), &f.store, context())
        .unwrap();
}

#[test]
fn prepare_mixed_records_and_check_replace_edges_before_adding() {
    let mut f = Fixture::new();
    for name in ["a", "b"] {
        f.create(name, Kind::Issue);
    }
    f.add_dependency("a", "b");
    let mut d = export(&f.store, &f.view(), &[id("a"), id("b")]).unwrap();
    d.issues[0].needs.clear();
    d.issues[1].needs.push(Reference::id("a"));
    let checked = d
        .check(&d.serialize(&f.view()).unwrap(), &f.store, context())
        .unwrap();
    assert!(
        checked
            .after
            .view()
            .unwrap()
            .current(&id("b"))
            .unwrap()
            .needs
            .contains(&id("a"))
    );
    let new = example().issues.remove(0);
    let mut new = Record {
        parent: None,
        ..new
    };
    new.needs.clear();
    d.issues.push(new);
    d.issues[0].needs.push(Reference::key("first"));
    d.prepare(&f.store, "demo").unwrap();
    assert!(d.issues[2].id.is_some());
    let checked = d
        .check(&d.serialize(&f.view()).unwrap(), &f.store, context())
        .unwrap();
    // The new Issue is published before the existing one that depends on it.
    assert_eq!(checked.records.len(), 3);
    assert_eq!(checked.records[0].kind, RecordKind::Created);
    assert!(
        checked.records[1..]
            .iter()
            .all(|r| r.kind == RecordKind::Import)
    );
}

#[test]
fn conflicted_and_violating_stores_reject_declarations_except_a_completing_retry() {
    let mut f = Fixture::new();
    f.create("a", Kind::Issue);
    f.create("b", Kind::Issue);
    let base = f.store.clone();
    // A conflict: the same Issue started on two sides.
    let mut other = Fixture {
        store: base.clone(),
        clock: std::cell::Cell::new(5000),
    };
    other.perform("a", Operation::Start);
    f.perform("a", Operation::Start);
    f.store.absorb(&other.store);
    assert!(f.view().is_conflicted(&id("a")));
    assert!(
        export(&f.store, &f.view(), &[id("a")])
            .unwrap_err()
            .to_string()
            .contains("conflicted")
    );
    assert!(export(&f.store, &f.view(), &[id("b")]).is_ok());
    let mut d = example();
    assert!(
        d.prepare(&f.store, "demo")
            .unwrap_err()
            .to_string()
            .contains("conflicted")
    );
    // A violation: a Completed dependent whose dependency was reopened elsewhere.
    let mut f = Fixture::new();
    f.create("dep", Kind::Issue);
    f.create("user", Kind::Issue);
    f.add_dependency("user", "dep");
    f.perform("dep", Operation::Start);
    f.perform("dep", Operation::Complete);
    let mut other = Fixture {
        store: f.store.clone(),
        clock: std::cell::Cell::new(5000),
    };
    other.perform("dep", Operation::Reopen);
    f.perform("user", Operation::Start);
    f.perform("user", Operation::Complete);
    f.store.absorb(&other.store);
    let view = f.view();
    assert!(view.conflicted().is_empty() && !view.violations().is_empty());
    // Export still works; a fresh plan is rejected until the violation is repaired.
    let exported = export(&f.store, &view, &[id("user")]).unwrap();
    assert_eq!(exported.issues.len(), 1);
    let error = d.prepare(&f.store, "demo").unwrap_err().to_string();
    assert!(error.contains("structural violations"), "{error}");
    let mut prepared = example();
    let mut clean = Fixture::new();
    prepared.prepare(&clean.store, "demo").unwrap();
    let input = prepared.serialize(&clean.view()).unwrap();
    let error = prepared
        .check(&input, &f.store, context())
        .unwrap_err()
        .to_string();
    assert!(error.contains("structural violations"), "{error}");
    // A retry whose remaining records remove the violation is allowed.
    let mut f = Fixture::new();
    f.create("x", Kind::Issue);
    f.create("y", Kind::Issue);
    f.add_dependency("x", "y");
    let mut swap = export(&f.store, &f.view(), &[id("x"), id("y")]).unwrap();
    swap.issues[0].needs.clear();
    swap.issues[1].needs.push(Reference::id("x"));
    let input = swap.serialize(&f.view()).unwrap();
    let checked = swap.check(&input, &f.store, context()).unwrap();
    // Publish the second record first: a completion cycle until the first one lands.
    f.store
        .insert(Entry::Record(checked.records[1].clone()))
        .unwrap();
    assert!(!f.view().violations().is_empty());
    let retried = swap.check(&input, &f.store, context()).unwrap();
    assert_eq!(retried.records.len(), 1);
    let mut again = swap.clone();
    again.prepare(&f.store, "demo").unwrap();
    f.store
        .insert(Entry::Record(retried.records[0].clone()))
        .unwrap();
    assert!(f.view().is_valid());
    let _ = clean.apply(&prepared);
    // A retry whose remaining records leave an unrelated violation in place is rejected.
    let mut f = Fixture::new();
    f.create("x", Kind::Issue);
    f.create("y", Kind::Issue);
    f.add_dependency("x", "y");
    f.create("a", Kind::Group);
    f.create("b", Kind::Group);
    let mut swap = export(&f.store, &f.view(), &[id("x"), id("y")]).unwrap();
    swap.issues[0].needs.clear();
    swap.issues[1].needs.push(Reference::id("x"));
    let input = swap.serialize(&f.view()).unwrap();
    let checked = swap.check(&input, &f.store, context()).unwrap();
    f.store
        .insert(Entry::Record(checked.records[1].clone()))
        .unwrap();
    let mut other = Fixture {
        store: f.store.clone(),
        clock: std::cell::Cell::new(6000),
    };
    other.set_parent("a", Some("b"));
    f.set_parent("b", Some("a"));
    f.store.absorb(&other.store);
    for error in [
        swap.check(&input, &f.store, context()).unwrap_err(),
        swap.clone().prepare(&f.store, "demo").unwrap_err(),
    ] {
        let error = error.to_string();
        assert!(error.contains("structural violations"), "{error}");
    }
}

#[test]
fn prepare_keeps_the_ids_an_interrupted_apply_published_and_replaces_only_taken_ones() {
    let mut f = Fixture::new();
    let mut d = example();
    d.prepare(&f.store, "demo").unwrap();
    let input = d.serialize(&f.view()).unwrap();
    let checked = d.check(&input, &f.store, context()).unwrap();
    // Only the Group was published before the process died.
    f.insert(checked.records[0].clone());
    let mut retry = d.clone();
    retry.prepare(&f.store, "demo").unwrap();
    assert_eq!(
        retry, d,
        "a published Entity keeps its ID on the next prepare"
    );
    let outcome = f.apply(&retry);
    assert_eq!(outcome.records.len(), 2);
    assert_eq!(f.view().known().count(), 3);
    // An ID another writer took for a different Entity is replaced; key references follow.
    let mut fresh = example();
    fresh.prepare(&f.store, "demo").unwrap();
    let taken = fresh.groups[0].id.clone().unwrap();
    f.create(&taken, Kind::Issue);
    let mut prepared = fresh.clone();
    prepared.prepare(&f.store, "demo").unwrap();
    assert_ne!(prepared.groups[0].id, fresh.groups[0].id);
    // The Issues' IDs are not in the store and are kept.
    assert!(
        prepared
            .issues
            .iter()
            .zip(&fresh.issues)
            .all(|(a, b)| a.id == b.id)
    );
    assert!(
        prepared
            .issues
            .iter()
            .all(|r| r.parent == Some(Reference::key("plan")))
    );
    prepared
        .check(&prepared.serialize(&f.view()).unwrap(), &f.store, context())
        .unwrap();
}

#[test]
fn export_tolerates_violations_and_missing_references_but_not_a_conflicted_child() {
    // A child whose parent's registration is missing (cherry-picked alone) exports; the
    // missing parent is not among the references, so a later check names it as missing.
    let mut f = Fixture::new();
    f.create("g", Kind::Group);
    let record = f
        .store
        .create(
            id("child"),
            Current {
                parent: Some(id("g")),
                ..current(Kind::Issue, "child")
            },
            context(),
        )
        .unwrap();
    let mut alone = Fixture::new();
    alone.insert(record);
    let view = alone.view();
    assert!(!view.violations().is_empty());
    let mut d = export(&alone.store, &view, &[id("child")]).unwrap();
    assert_eq!(d.issues.len(), 1);
    assert!(d.references.is_empty());
    let error = d
        .clone()
        .prepare(&alone.store, "demo")
        .unwrap_err()
        .to_string();
    assert!(error.contains("does not exist"), "{error}");
    // Unchanged or changed, the declaration is refused: the missing parent is named as a
    // missing reference before the store's violation, which no application would remove.
    let input = d.serialize(&view).unwrap();
    let error = d
        .check(&input, &alone.store, context())
        .unwrap_err()
        .to_string();
    assert!(error.contains("does not exist"), "{error}");
    d.issues[0].title = "renamed".into();
    let input = d.serialize(&view).unwrap();
    let error = d
        .check(&input, &alone.store, context())
        .unwrap_err()
        .to_string();
    assert!(error.contains("does not exist"), "{error}");
    // A conflicted child of a selected Group is rejected, whichever head names the parent.
    let mut f = Fixture::new();
    f.create("g", Kind::Group);
    f.create("c", Kind::Issue);
    f.set_parent("c", Some("g"));
    let mut other = Fixture {
        store: f.store.clone(),
        clock: std::cell::Cell::new(5000),
    };
    other.set_parent("c", None);
    f.perform("c", Operation::Start);
    f.store.absorb(&other.store);
    let view = f.view();
    assert!(view.is_conflicted(&id("c")));
    let error = export(&f.store, &view, &[id("g")]).unwrap_err().to_string();
    assert!(error.contains("conflicted child"), "{error}");
}

#[test]
fn a_fresh_declaration_that_would_repair_a_violation_is_still_rejected() {
    // Two sides each move a Group under the other; the merge is a containment cycle.
    let mut f = Fixture::new();
    f.create("a", Kind::Group);
    f.create("b", Kind::Group);
    let mut other = Fixture {
        store: f.store.clone(),
        clock: std::cell::Cell::new(5000),
    };
    other.set_parent("a", Some("b"));
    f.set_parent("b", Some("a"));
    f.store.absorb(&other.store);
    let view = f.view();
    assert!(view.conflicted().is_empty() && !view.violations().is_empty());
    let mut d = export(&f.store, &view, &[id("a"), id("b")]).unwrap();
    d.groups[0].parent = None;
    let input = d.serialize(&view).unwrap();
    // Every Entity matches its base, so this is not the retry of an interrupted apply.
    let error = d
        .check(&input, &f.store, context())
        .unwrap_err()
        .to_string();
    assert!(error.contains("structural violations"), "{error}");
    let error = d.prepare(&f.store, "demo").unwrap_err().to_string();
    assert!(error.contains("structural violations"), "{error}");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn generated_export_selectors_include_descendants_and_external_references(
        title in "[A-Za-z][A-Za-z0-9]{0,12}",
        reverse in any::<bool>(),
    ) {
        let mut f = Fixture::new();
        for (name, kind) in [("h", Kind::Group), ("g", Kind::Group), ("a", Kind::Issue), ("b", Kind::Issue), ("x", Kind::Issue)] {
            f.create(name, kind);
        }
        f.set_parent("g", Some("h"));
        f.set_parent("a", Some("g"));
        f.set_parent("b", Some("h"));
        f.add_dependency("a", "x");
        f.add_dependency("b", "a");
        let edited_title = format!("Edited {title}");
        f.write("a", &edited_title);
        for mask in 1u8..32 {
            let mut selectors: Vec<_> = ["h", "g", "a", "b", "x"]
                .into_iter().enumerate().filter(|(bit, _)| mask & (1 << bit) != 0)
                .map(|(_, name)| id(name)).collect();
            if reverse { selectors.reverse(); }
            let mut expected: BTreeSet<_> = selectors.iter().map(ToString::to_string).collect();
            if expected.contains("h") { expected.extend(["g", "a", "b"].map(str::to_owned)); }
            if expected.contains("g") { expected.insert("a".into()); }
            let declaration = export(&f.store, &f.view(), &selectors).unwrap();
            let actual: BTreeSet<_> = declaration.records().map(|record| record.id.clone().unwrap()).collect();
            prop_assert_eq!(&actual, &expected);
            let external: BTreeSet<_> = ["h", "g", "a", "x"].into_iter()
                .filter(|name| !expected.contains(*name) && match *name {
                    "h" => expected.contains("g") || expected.contains("b"),
                    "g" => expected.contains("a"),
                    "a" => expected.contains("b"),
                    _ => expected.contains("a"),
                }).map(str::to_owned).collect();
            prop_assert_eq!(declaration.references.iter().map(|reference| reference.id.clone()).collect::<BTreeSet<_>>(), external);
            prop_assert!(declaration.records().all(|record| record.key.is_none() && record.base.is_some()));
            if expected.contains("a") {
                prop_assert_eq!(declaration.issues.iter().find(|record| record.id.as_deref() == Some("a")).unwrap().title.as_str(), edited_title.as_str());
            }
        }
    }

    #[test]
    fn generated_plan_edits_survive_partial_publication_and_retry(
        title in "[A-Za-z][A-Za-z0-9]{0,12}",
        reverse in any::<bool>(),
    ) {
        for relationship in 0..5 {
          for published in 0..=3 {
            let mut f = Fixture::new();
            for (name, kind) in [("g", Kind::Group), ("x", Kind::Issue), ("a", Kind::Issue), ("b", Kind::Issue)] {
                f.create(name, kind);
            }
            f.set_parent("a", Some("g"));
            f.add_dependency("a", "b");
            let original = f.store.clone();
            let mut d = export(&f.store, &f.view(), &[id("g"), id("a"), id("b")]).unwrap();
            let a = d.issues.iter_mut().find(|r| r.id.as_deref() == Some("a")).unwrap();
            a.title = format!("Edited {title}");
            match relationship {
                0 => a.parent = None,
                2 | 4 => a.needs.clear(),
                _ => {},
            }
            let b = d.issues.iter_mut().find(|r| r.id.as_deref() == Some("b")).unwrap();
            match relationship {
                1 => b.parent = Some(Reference::id("g")),
                3 => b.needs = vec![Reference::id("x")],
                4 => b.needs = vec![Reference::id("a")],
                _ => {},
            }
            let mut new = example().issues.remove(0);
            new.key = Some("new".into());
            new.title = format!("New {title}");
            new.parent = Some(Reference::id("g"));
            new.needs = vec![Reference::id("x")];
            d.issues.push(new);
            if reverse { d.issues.reverse(); }
            d.prepare(&f.store, "demo").unwrap();
            let new_id = d.issues.iter().find(|r| r.key.as_deref() == Some("new")).unwrap().id.clone().unwrap();
            let input = d.serialize(&f.view()).unwrap();
            let checked = d.check(&input, &f.store, context()).unwrap();
            prop_assert_eq!(checked.records.len(), if relationship == 0 || relationship == 2 { 2 } else { 3 });
            let prefix = published.min(checked.records.len());
            for record in checked.records.iter().take(prefix) {
                f.insert(record.clone());
            }
            let retry = d.check(&input, &f.store, context()).unwrap();
            prop_assert_eq!(retry.records.len(), checked.records.len() - prefix);
            for record in retry.records { f.insert(record); }
            let settled = f.view();
            prop_assert!(settled.is_valid());
            prop_assert_eq!(settled.known().count(), 5);
            let a = settled.current(&id("a")).unwrap();
            let b = settled.current(&id("b")).unwrap();
            let new = settled.current(&id(&new_id)).unwrap();
            prop_assert_eq!(a.title.as_str(), format!("Edited {title}"));
            prop_assert_eq!(a.parent.as_ref(), if relationship == 0 { None } else { Some(&id("g")) });
            prop_assert_eq!(&a.needs, if relationship == 2 || relationship == 4 { &BTreeSet::new() } else { &BTreeSet::from([id("b")]) });
            prop_assert_eq!(b.parent.as_ref(), if relationship == 1 { Some(&id("g")) } else { None });
            prop_assert_eq!(&b.needs, match relationship { 3 => &BTreeSet::from([id("x")]), 4 => &BTreeSet::from([id("a")]), _ => &BTreeSet::new() });
            prop_assert_eq!(new.parent.as_ref(), Some(&id("g")));
            prop_assert_eq!(&new.needs, &BTreeSet::from([id("x")]));
            let original_view = original.view().unwrap();
            prop_assert_eq!(settled.current(&id("x")), original_view.current(&id("x")));
            prop_assert!(d.check(&input, &f.store, context_at(9000)).unwrap().records.is_empty());
          }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    #[test]
    fn generated_invalid_stores_keep_fresh_plans_out_but_allow_repairing_retries(
        title in "[A-Za-z][A-Za-z0-9]{0,12}",
    ) {
      for conflicted_child in [false, true] {
        let mut f = Fixture::new();
        f.create("g", Kind::Group);
        f.create("a", Kind::Issue);
        f.set_parent("a", Some("g"));
        f.create("unrelated", Kind::Issue);
        let mut other = Fixture { store: f.store.clone(), clock: std::cell::Cell::new(6000) };
        let conflicted = if conflicted_child { "a" } else { "unrelated" };
        f.write(conflicted, &format!("Left {title}"));
        other.write(conflicted, &format!("Right {title}"));
        f.store.absorb(&other.store);
        let exported = export(&f.store, &f.view(), &[id("g")]);
        if conflicted_child {
            prop_assert!(exported.unwrap_err().to_string().contains("conflicted"));
        } else {
            prop_assert_eq!(exported.unwrap().issues.len(), 1);
        }
        let mut fresh = example();
        fresh.groups[0].title = title.clone();
        let input = fresh.serialize(&f.view()).unwrap();
        prop_assert!(fresh.prepare(&f.store, "demo").unwrap_err().to_string().contains("conflicted"));
        prop_assert_eq!(input, fresh.serialize(&f.view()).unwrap());
      }

        for missing_parent in [false, true] {
            let mut source = Fixture::new();
            source.create("missing", if missing_parent { Kind::Group } else { Kind::Issue });
            let mut value = current(Kind::Issue, &title);
            if missing_parent {
                value.parent = Some(id("missing"));
            } else {
                value.needs.insert(id("missing"));
            }
            let record = source.store.create(id("child"), value, context()).unwrap();
            let mut alone = Fixture::new();
            alone.insert(record);
            let view = alone.view();
            let declaration = export(&alone.store, &view, &[id("child")]).unwrap();
            prop_assert!(declaration.references.is_empty());
            let input = declaration.serialize(&view).unwrap();
            prop_assert!(declaration.clone().prepare(&alone.store, "demo").unwrap_err().to_string().contains("does not exist"));
            prop_assert!(declaration.check(&input, &alone.store, context()).unwrap_err().to_string().contains("does not exist"));
        }

        let mut f = Fixture::new();
        f.create("x", Kind::Issue);
        f.create("y", Kind::Issue);
        f.add_dependency("x", "y");
        let mut swap = export(&f.store, &f.view(), &[id("x"), id("y")]).unwrap();
        swap.issues[0].needs.clear();
        swap.issues[1].needs = vec![Reference::id("x")];
        swap.issues[1].title = title;
        let input = swap.serialize(&f.view()).unwrap();
        let checked = swap.check(&input, &f.store, context()).unwrap();
        prop_assert_eq!(checked.records.len(), 2);
        f.insert(checked.records[1].clone());
        prop_assert!(!f.view().is_valid());
        let mut fresh = example();
        prop_assert!(fresh.prepare(&f.store, "demo").unwrap_err().to_string().contains("structural violations"));
        let retry = swap.check(&input, &f.store, context()).unwrap();
        prop_assert_eq!(retry.records.len(), 1);
        f.insert(retry.records[0].clone());
        prop_assert!(f.view().is_valid());
        prop_assert!(swap.check(&input, &f.store, context()).unwrap().already_applied);
    }
}
