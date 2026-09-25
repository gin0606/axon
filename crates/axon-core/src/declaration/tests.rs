use super::*;
use crate::lifecycle::{Context, Current};
use chrono::{TimeZone, Utc};
fn id(s: &str) -> EntityId {
    s.to_owned().try_into().unwrap()
}
fn context() -> Context {
    Context {
        at: Utc.timestamp_opt(1000, 0).unwrap(),
        recorder: None,
    }
}
fn current(title: &str) -> Current {
    Current {
        title: title.into(),
        description: String::new(),
        lifecycle: Lifecycle::NotStarted,
        condition: None,
        parent: None,
        dependencies: BTreeSet::new(),
    }
}
fn snapshot() -> Snapshot {
    Snapshot::empty()
}
#[test]
fn new_example_is_canonical_and_has_one_dependency() {
    let d = example();
    let text = d.serialize(&snapshot()).unwrap();
    assert_eq!(parse(&text).unwrap(), d);
    assert_eq!(parse(&text).unwrap().serialize(&snapshot()).unwrap(), text);
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
        let yaml = d.serialize(&snapshot()).unwrap();
        assert_eq!(
            parse(&yaml).unwrap_or_else(|e| panic!("{value:?}: {e}\n{yaml}")),
            d,
            "{value:?}"
        );
    }
    for value in ['\0', '\u{1}', '\u{1f}', '\u{7f}', '\u{9f}', '\u{a0}'] {
        let mut d = example();
        d.issues[0].description = format!("a{value}b");
        assert_eq!(parse(&d.serialize(&snapshot()).unwrap()).unwrap(), d);
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
    let text = example().serialize(&snapshot()).unwrap();
    let invalid = [
        text.replace(
            "schema: axon-declaration/v1",
            "schema: axon-declaration/v1\nunknown: true",
        ),
        text.replace(
            "schema: axon-declaration/v1",
            "schema: axon-declaration/v1\nschema: axon-declaration/v1",
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
}
#[test]
fn canonical_order_uses_creation_then_ids_then_keys_and_resolved_needs() {
    let mut s = snapshot();
    for (name, sec) in [("p-z", 1), ("p-b", 2), ("p-a", 2)] {
        let mut c = context();
        c.at = Utc.timestamp_opt(sec, 0).unwrap();
        s.create(id(name), Kind::Issue, current(name), c).unwrap();
    }
    let mut d = export(&s, &[id("p-a"), id("p-z"), id("p-b")]).unwrap();
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
    let yaml = d.serialize(&s).unwrap();
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
    assert_eq!(parsed.serialize(&s).unwrap(), yaml);
    d.issues
        .last_mut()
        .unwrap()
        .needs
        .push(Reference::id("p-a"));
    assert!(d.serialize(&s).is_err());
}
#[test]
fn fingerprint_tokens_and_visible_field_changes() {
    let mut s = snapshot();
    s.create(id("demo-a"), Kind::Issue, current("日本語"), context())
        .unwrap();
    let e = s.entity(&id("demo-a")).unwrap().clone();
    let mut bytes = Vec::new();
    for token in [
        SCHEMA,
        "issue",
        "demo-a",
        "not-started",
        "日本語",
        "",
        "none",
    ] {
        bytes.extend((token.len() as u64).to_be_bytes());
        bytes.extend(token.as_bytes());
    }
    bytes.extend(0u64.to_be_bytes());
    let base = fingerprint(&e);
    assert_eq!(base, format!("blake3:{}", blake3::hash(&bytes).to_hex()));
    for change in 0..7 {
        let mut other = e.clone();
        match change {
            0 => other.current.title.push('!'),
            1 => other.current.description.push('!'),
            2 => other.current.lifecycle = Lifecycle::Completed,
            3 => other.current.parent = Some(id("parent")),
            4 => {
                other.current.dependencies.insert(id("needs"));
            }
            5 => other.kind = Kind::Group,
            _ => other.id = id("other"),
        }
        assert_ne!(base, fingerprint(&other));
    }
    s.set_condition(&e.id, Some("exit 1".into())).unwrap();
    s.add_note(&e.id, "Evidence".into(), context()).unwrap();
    assert_eq!(base, fingerprint(s.entity(&e.id).unwrap()));
    let mut other = e;
    other.created_at = Utc::now();
    assert_eq!(base, fingerprint(&other));
}
#[test]
fn declaration_doc_example_has_identical_canonical_bytes() {
    let doc = include_str!("../../../../docs/reference/declaration.md");
    let text = doc
        .split("```yaml\nschema: axon-declaration/v1\n")
        .nth(1)
        .unwrap()
        .split("```\n")
        .next()
        .unwrap();
    let text = format!("schema: axon-declaration/v1\n{text}");
    let d = parse(&text).unwrap();
    let mut s = snapshot();
    s.create(id("demo-k3m7pq"), Kind::Group, current("Group"), context())
        .unwrap();
    for name in ["demo-8bxw2r", "demo-c9d4ts"] {
        s.create(id(name), Kind::Issue, current(name), context())
            .unwrap();
    }
    assert_eq!(d.serialize(&s).unwrap(), text);
}

#[test]
fn prepared_plan_checks_retries_and_collisions_without_partial_matches() {
    let original = snapshot();
    let mut d = example();
    d.prepare(&original, "demo").unwrap();
    let yaml = d.serialize(&original).unwrap();
    let checked = d.check(&yaml, &original, context()).unwrap();
    assert_eq!(checked.snapshot.entities().count(), 3);
    assert!(d.already_applied(&checked.snapshot).unwrap());
    assert!(
        d.check(&yaml, &checked.snapshot, context())
            .unwrap()
            .already_applied
    );
    let mut retry = d.clone();
    retry.prepare(&checked.snapshot, "demo").unwrap();
    assert_eq!(retry, d);
    let mut changed = checked.snapshot.clone();
    let first = id(d.issues[0].id.as_ref().unwrap());
    changed
        .write(&first, Some("Concurrent".into()), None)
        .unwrap();
    assert!(
        d.check(&yaml, &changed, context())
            .unwrap_err()
            .to_string()
            .contains("conflict:")
    );
    retry.prepare(&changed, "demo").unwrap();
    assert!(retry.records().zip(d.records()).all(|(a, b)| a.id != b.id));
    retry
        .check(&retry.serialize(&changed).unwrap(), &changed, context())
        .unwrap();
}

#[test]
fn check_core_constraints_and_references() {
    use crate::lifecycle::Operation;
    let mut s = snapshot();
    for (name, kind) in [
        ("g", Kind::Group),
        ("h", Kind::Group),
        ("a", Kind::Issue),
        ("b", Kind::Issue),
    ] {
        s.create(id(name), kind, current(name), context()).unwrap();
    }
    let reject = |d: &Declaration, snapshot: &Snapshot, field: &str| {
        let bytes = d.serialize(snapshot).unwrap();
        let error = d
            .check(&bytes, snapshot, context())
            .unwrap_err()
            .to_string();
        assert!(error.contains(field), "{field}: {error}");
    };
    let mut d = export(&s, &[id("a"), id("b")]).unwrap();
    d.issues[0].needs.push(Reference::id("b"));
    d.issues[1].needs.push(Reference::id("a"));
    reject(&d, &s, "core rejection: b: needs");
    let mut d = export(&s, &[id("g"), id("h")]).unwrap();
    d.groups[0].parent = Some(Reference::id("h"));
    d.groups[1].parent = Some(Reference::id("g"));
    reject(&d, &s, "parent");
    s.perform(&id("a"), Operation::Start, None, context())
        .unwrap();
    // InProgress work moves only under adopted Groups.
    s.perform(&id("g"), Operation::Withdraw, None, context())
        .unwrap();
    let mut d = export(&s, &[id("a")]).unwrap();
    d.issues[0].parent = Some(Reference::id("g"));
    d.refresh_references(&s).unwrap();
    reject(&d, &s, "parent");
    s.perform(&id("g"), Operation::Accept, None, context())
        .unwrap();
    s.perform(&id("a"), Operation::Complete, None, context())
        .unwrap();
    for field in ["title", "description", "needs"] {
        let mut d = export(&s, &[id("a")]).unwrap();
        match field {
            "title" => d.issues[0].title = "Changed".into(),
            "description" => d.issues[0].description = "Changed".into(),
            _ => d.issues[0].needs.push(Reference::id("b")),
        }
        d.refresh_references(&s).unwrap();
        reject(&d, &s, field);
    }
    s.set_parent(&id("a"), Some(id("g"))).unwrap();
    s.perform(&id("g"), Operation::Complete, None, context())
        .unwrap();
    let mut d = export(&s, &[id("a")]).unwrap();
    d.issues[0].parent = None;
    d.refresh_references(&s).unwrap();
    reject(&d, &s, "parent");
    let mut d = export(&s, &[id("b")]).unwrap();
    d.issues[0].parent = Some(Reference::id("g"));
    d.refresh_references(&s).unwrap();
    reject(&d, &s, "parent");
    let mut d = export(&s, &[id("b")]).unwrap();
    d.issues[0].needs.push(Reference::id("a"));
    d.refresh_references(&s).unwrap();
    d.references[0].title = "Stale title".into();
    d.references[0].lifecycle = "undecided".into();
    d.check(&d.serialize(&s).unwrap(), &s, context()).unwrap();
}

#[test]
fn prepare_mixed_records_and_check_replace_edges_before_adding() {
    let mut s = snapshot();
    for name in ["a", "b"] {
        s.create(id(name), Kind::Issue, current(name), context())
            .unwrap();
    }
    s.add_dependency(&id("a"), &id("b")).unwrap();
    let mut d = export(&s, &[id("a"), id("b")]).unwrap();
    d.issues[0].needs.clear();
    d.issues[1].needs.push(Reference::id("a"));
    let checked = d.check(&d.serialize(&s).unwrap(), &s, context()).unwrap();
    assert!(
        checked
            .snapshot
            .entity(&id("b"))
            .unwrap()
            .current
            .dependencies
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
    d.prepare(&s, "demo").unwrap();
    assert!(d.issues[2].id.is_some());
    d.check(&d.serialize(&s).unwrap(), &s, context()).unwrap();
}
