//! One record file: canonical bytes, the ID as their hash, and everything decode rejects.
use super::*;
use Operation::*;
use proptest::prelude::*;

fn store_with_every_kind() -> (Store, Vec<RecordId>) {
    let mut r = Replica::new("r0");
    let mut ids = vec![r.head("i3")];
    ids.push(r.op("i3", Start));
    let edit = r
        .store
        .write(
            &id("i3"),
            Some("edited".into()),
            Some("desc".into()),
            None,
            r.tick(),
        )
        .unwrap()
        .unwrap();
    ids.push(insert(&mut r.store, edit));
    let label = r
        .store
        .set_label(&id("i3"), Label::Docs, None, r.tick())
        .unwrap()
        .unwrap();
    ids.push(insert(&mut r.store, label));
    ids.push(r.move_to("i3", Some("g0")));
    ids.push(r.add_dep("i3", "g2"));
    let condition = r
        .store
        .set_condition(&id("i3"), Some("exit 0".into()), None, r.tick())
        .unwrap()
        .unwrap();
    ids.push(insert(&mut r.store, condition));
    let import = r
        .store
        .import(
            &id("i3"),
            Imported {
                title: "edited".into(),
                description: "desc".into(),
                label: Label::Bug,
                parent: None,
                needs: BTreeSet::new(),
            },
            r.tick(),
        )
        .unwrap()
        .unwrap();
    ids.push(insert(&mut r.store, import));
    ids.push(r.op("i3", Release));
    let convert = r
        .store
        .convert(&id("i3"), Kind::Group, None, r.tick())
        .unwrap()
        .unwrap();
    ids.push(insert(&mut r.store, convert));
    let mut r1 = Replica::from("r1", &r.store);
    let mine = r.op("i3", Withdraw);
    r1.op("i3", Cancel);
    r.sync(&r1);
    ids.push(r.resolve("i3", &mine));
    ids.push(r.note("i3", "a note"));
    (r.store, ids)
}

fn json_value() -> impl Strategy<Value = serde_json::Value> {
    let leaf = prop_oneof![
        Just(serde_json::Value::Null),
        any::<bool>().prop_map(serde_json::Value::Bool),
        any::<i64>().prop_map(|n| serde_json::json!(n)),
        any::<String>().prop_map(serde_json::Value::String),
    ];
    leaf.prop_recursive(3, 32, 4, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(serde_json::Value::Array),
            prop::collection::btree_map("[a-z]{1,6}", inner, 0..4)
                .prop_map(|items| serde_json::Value::Object(items.into_iter().collect())),
        ]
    })
}

#[test]
fn every_record_kind_round_trips_through_canonical_bytes_with_its_hash_as_id() {
    let (store, ids) = store_with_every_kind();
    let mut kinds = BTreeSet::new();
    for id in &ids {
        let entry = store.get(id).unwrap();
        let bytes = encode(entry).unwrap();
        assert_eq!(bytes.last(), Some(&b'\n'));
        assert_eq!(bytes.iter().filter(|b| **b == b'\n').count(), 1);
        assert_eq!(RecordId::of(&bytes), *id);
        assert_eq!(id.subdirectory(), &id.to_string()[..2]);
        let (decoded_id, decoded) = decode(&bytes).unwrap();
        assert_eq!(decoded_id, *id);
        assert_eq!(&decoded, entry);
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.starts_with("{\"entity\":\"i3\",\"record\":\""));
        kinds.insert(
            text.split("\"record\":\"")
                .nth(1)
                .unwrap()
                .split('"')
                .next()
                .unwrap()
                .to_string(),
        );
    }
    assert_eq!(
        kinds,
        [
            "created",
            "transition",
            "edit",
            "label",
            "parent",
            "dependency",
            "condition",
            "import",
            "convert",
            "resolve",
            "note"
        ]
        .into_iter()
        .map(String::from)
        .collect()
    );
    // The same content inserted again is the same record.
    let mut copy = Store::new();
    for (id, entry) in store.entries() {
        assert_eq!(copy.insert(entry.clone()).unwrap(), *id);
        assert_eq!(copy.insert_bytes(&encode(entry).unwrap()).unwrap(), *id);
    }
    assert_eq!(copy, store);
}

#[test]
fn key_order_and_optional_keys_follow_the_contract() {
    let (store, ids) = store_with_every_kind();
    let text = |i: usize| String::from_utf8(encode(store.get(&ids[i]).unwrap()).unwrap()).unwrap();
    let start = text(1);
    assert!(start.contains("\"record\":\"transition\",\"operation\":\"start\",\"parents\":[\""));
    assert!(start.contains("\"],\"at\":\"1970-01-01T00:0"));
    assert!(start.contains("\"recorder\":{\"actor\":\"r0\",\"data\":{\"session_id\":\"session-1\"}},\"reason\":null,\"after\":{\"kind\":\"issue\",\"lifecycle\":\"in-progress\",\"owner\":\"r0\",\"title\":\"task\",\"description\":\"body\\n日本語\",\"label\":\"feat\",\"condition\":null,\"parent\":null,\"needs\":[]}}\n"));
    let created = text(0);
    assert!(created.contains("\"record\":\"created\",\"parents\":[],\"at\":"));
    let resolve = text(10);
    assert!(resolve.contains("\"record\":\"resolve\",\"parents\":[\""));
    assert!(resolve.contains("\"],\"chosen\":\""));
    let note = text(11);
    assert!(note.contains("\"record\":\"note\",\"parents\":[],\"nonce\":\""));
    assert!(note.ends_with("\"reason\":null,\"body\":\"a note\"}\n"));
    assert!(!note.contains("\"after\""));
}

/// Record files written by an earlier encoder, one per record kind. Stored records stay readable
/// only while decoding and re-encoding reproduces these bytes exactly.
const WRITTEN_RECORDS: [&str; 12] = [
    r#"{"entity":"i3","record":"created","parents":[],"at":"1970-01-01T00:00:03.500Z","recorder":{"actor":"setup","data":{"session_id":"session-1"}},"reason":null,"after":{"kind":"issue","lifecycle":"not-started","owner":null,"title":"task","description":"body\n日本語","label":"feat","condition":null,"parent":null,"needs":[]}}"#,
    r#"{"entity":"i3","record":"transition","operation":"start","parents":["95e349ab47d8d1cd4f42275d434470347c48efdeb43d4085aeb13338580feec6"],"at":"1970-01-01T00:01:41.500Z","recorder":{"actor":"r0","data":{"session_id":"session-1"}},"reason":null,"after":{"kind":"issue","lifecycle":"in-progress","owner":"r0","title":"task","description":"body\n日本語","label":"feat","condition":null,"parent":null,"needs":[]}}"#,
    r#"{"entity":"i3","record":"edit","parents":["8b9bfb377789bac516817020a2ae94088a3717679873a39e490295cac2a64a6b"],"at":"1970-01-01T00:01:42.500Z","recorder":{"actor":"r0","data":{"session_id":"session-1"}},"reason":null,"after":{"kind":"issue","lifecycle":"in-progress","owner":"r0","title":"edited","description":"desc","label":"feat","condition":null,"parent":null,"needs":[]}}"#,
    r#"{"entity":"i3","record":"label","parents":["701ecf85fe32041e2b4336bb42b37af71d6f8adbf42779c228254f7432306751"],"at":"1970-01-01T00:01:43.500Z","recorder":{"actor":"r0","data":{"session_id":"session-1"}},"reason":null,"after":{"kind":"issue","lifecycle":"in-progress","owner":"r0","title":"edited","description":"desc","label":"docs","condition":null,"parent":null,"needs":[]}}"#,
    r#"{"entity":"i3","record":"parent","parents":["b714d5c2e4e729661cd599712044a62f0506a156b985944e5086ba5520acdf6c"],"at":"1970-01-01T00:01:44.500Z","recorder":{"actor":"r0","data":{"session_id":"session-1"}},"reason":null,"after":{"kind":"issue","lifecycle":"in-progress","owner":"r0","title":"edited","description":"desc","label":"docs","condition":null,"parent":"g0","needs":[]}}"#,
    r#"{"entity":"i3","record":"dependency","parents":["e5f30d64565d67fa22a2620200a76fec14b9188b8f028d927d74b38a2f47fc1d"],"at":"1970-01-01T00:01:45.500Z","recorder":{"actor":"r0","data":{"session_id":"session-1"}},"reason":null,"after":{"kind":"issue","lifecycle":"in-progress","owner":"r0","title":"edited","description":"desc","label":"docs","condition":null,"parent":"g0","needs":["g2"]}}"#,
    r#"{"entity":"i3","record":"condition","parents":["57f165a404da7a7ea25829ac54a0c4847e43eb757444052c79e4152b4deef63b"],"at":"1970-01-01T00:01:46.500Z","recorder":{"actor":"r0","data":{"session_id":"session-1"}},"reason":null,"after":{"kind":"issue","lifecycle":"in-progress","owner":"r0","title":"edited","description":"desc","label":"docs","condition":"exit 0","parent":"g0","needs":["g2"]}}"#,
    r#"{"entity":"i3","record":"import","parents":["88fe9a32d4947df4f0d83a6f7344233dd79236e33b30e978bb91efa7c0829fc1"],"at":"1970-01-01T00:01:47.500Z","recorder":{"actor":"r0","data":{"session_id":"session-1"}},"reason":null,"after":{"kind":"issue","lifecycle":"in-progress","owner":"r0","title":"edited","description":"desc","label":"bug","condition":"exit 0","parent":null,"needs":[]}}"#,
    r#"{"entity":"i3","record":"transition","operation":"release","parents":["7e446f06c056822660b822144afc121812f24a7c2b20012ae413ae7132ce9d24"],"at":"1970-01-01T00:01:48.500Z","recorder":{"actor":"r0","data":{"session_id":"session-1"}},"reason":null,"after":{"kind":"issue","lifecycle":"not-started","owner":null,"title":"edited","description":"desc","label":"bug","condition":"exit 0","parent":null,"needs":[]}}"#,
    r#"{"entity":"i3","record":"convert","parents":["5bddd081183c035566bd693431653db9e61cb96d511cc384e4a376d5ee35b3ea"],"at":"1970-01-01T00:01:49.500Z","recorder":{"actor":"r0","data":{"session_id":"session-1"}},"reason":null,"after":{"kind":"group","lifecycle":"not-started","owner":null,"title":"edited","description":"desc","label":"bug","condition":"exit 0","parent":null,"needs":[]}}"#,
    r#"{"entity":"i3","record":"resolve","parents":["446215bc8e9180b0903bd7ee129d1838c8d75d30ab2a21d931b2155d9ba6dfe9","70385063e2cb933ce5638e10200fb5a71b5849d81ab9df2f73a9007a17ca7f34"],"chosen":"70385063e2cb933ce5638e10200fb5a71b5849d81ab9df2f73a9007a17ca7f34","at":"1970-01-01T00:01:51.500Z","recorder":{"actor":"r0","data":{"session_id":"session-1"}},"reason":"pick","after":{"kind":"group","lifecycle":"undecided","owner":null,"title":"edited","description":"desc","label":"bug","condition":"exit 0","parent":null,"needs":[]}}"#,
    r#"{"entity":"i3","record":"note","parents":[],"nonce":"282559de5da0c189ec16e95dad6cfce4","at":"1970-01-01T00:01:52.500Z","recorder":{"actor":"r0","data":{"session_id":"session-1"}},"reason":null,"body":"a note"}"#,
];

#[test]
fn records_written_earlier_decode_and_re_encode_to_the_same_bytes() {
    for line in WRITTEN_RECORDS {
        let bytes = format!("{line}\n").into_bytes();
        let (id, entry) = decode(&bytes).unwrap();
        assert_eq!(id, RecordId::of(&bytes));
        assert_eq!(encode(&entry).unwrap(), bytes, "{line}");
    }
}

#[test]
fn decode_rejects_a_group_storing_in_progress() {
    let text =
        format!("{}\n", WRITTEN_RECORDS[1]).replace("\"kind\":\"issue\"", "\"kind\":\"group\"");
    assert_eq!(
        error(decode(text.as_bytes())),
        "a Group never stores InProgress"
    );
}

#[test]
fn decode_rejects_empty_truncated_and_unterminated_records() {
    let (store, ids) = store_with_every_kind();
    let bytes = encode(store.get(&ids[1]).unwrap()).unwrap();
    assert!(error(decode(b"")).contains("empty"));
    assert!(error(decode(&bytes[..bytes.len() - 1])).contains("line feed"));
    assert!(error(decode(&bytes[..bytes.len() / 2])).contains("line feed"));
    let mut truncated = bytes[..bytes.len() / 2].to_vec();
    truncated.push(b'\n');
    assert!(error(decode(&truncated)).contains("EOF"));
}

#[test]
fn decode_rejects_unknown_missing_and_incompatible_record_fields() {
    let (store, ids) = store_with_every_kind();
    let bytes = encode(store.get(&ids[1]).unwrap()).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    let with = |from: &str, to: &str| decode(text.replacen(from, to, 1).as_bytes());
    assert!(
        error(with("\"reason\":null", "\"reason\":null,\"extra\":1")).contains("unknown field")
    );
    assert!(error(with("\"reason\":null,", "")).contains("missing key \"reason\""));
    assert!(error(with("\"owner\":\"r0\",", "")).contains("missing key \"owner\""));
    assert!(error(with("\"operation\":\"start\",", "")).contains("missing key \"operation\""));
    assert!(
        error(with(
            "\"record\":\"transition\",\"operation\":\"start\"",
            "\"record\":\"edit\",\"operation\":\"start\""
        ))
        .contains("only a transition")
    );
    assert!(
        error(with("\"record\":\"transition\"", "\"record\":\"note\"")).contains("note carries no")
    );
    assert!(
        error(with("\"record\":\"transition\"", "\"record\":\"merge\""))
            .contains("unknown record kind")
    );
    assert!(
        error(with("\"operation\":\"start\"", "\"operation\":\"merge\""))
            .contains("unknown operation")
    );
    assert!(error(with("\"kind\":\"issue\"", "\"kind\":\"epic\"")).contains("unknown kind"));
}

#[test]
fn record_labels_use_the_fixed_spellings_and_canonical_field_position() {
    let (store, ids) = store_with_every_kind();
    let bytes = encode(store.get(&ids[1]).unwrap()).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    let with = |from: &str, to: &str| decode(text.replacen(from, to, 1).as_bytes());
    // The label is always present, spelled from the fixed set, and placed after description.
    let names: Vec<_> = Label::ALL.iter().map(|label| label.name()).collect();
    assert_eq!(
        names,
        ["bug", "feat", "chore", "docs", "test", "refactor", "spike"]
    );
    // Decoding reads the spellings from `ALL`, so a variant missing there would be written
    // but never read back. The exhaustive match makes a new variant name its place in `ALL`.
    let position = |label: Label| match label {
        Label::Bug => 0,
        Label::Feat => 1,
        Label::Chore => 2,
        Label::Docs => 3,
        Label::Test => 4,
        Label::Refactor => 5,
        Label::Spike => 6,
    };
    assert_eq!(Label::ALL.map(position), std::array::from_fn(|index| index));
    for label in Label::ALL {
        assert_eq!(Label::from_name(label.name()).unwrap(), label);
        let spelled = with("\"label\":\"feat\"", &format!("\"label\":\"{label}\""));
        assert_eq!(spelled.unwrap().1.as_record().unwrap().after.label, label);
    }
    assert!(error(with("\"label\":\"feat\",", "")).contains("missing key \"label\""));
    for outside in ["\"fix\"", "\"Feat\"", "\"\"", "null", "[\"feat\"]"] {
        let error = error(with("\"label\":\"feat\"", &format!("\"label\":{outside}")));
        assert!(
            error.contains("unknown label") || error.contains("invalid type"),
            "{outside}: {error}"
        );
    }
    assert!(
        error(with(
            "\"description\":\"body\\n日本語\",\"label\":\"feat\"",
            "\"label\":\"feat\",\"description\":\"body\\n日本語\""
        ))
        .contains("not canonical")
    );
}

#[test]
fn decode_rejects_invalid_lifecycle_and_ownership() {
    let (store, ids) = store_with_every_kind();
    let bytes = encode(store.get(&ids[1]).unwrap()).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    let with = |from: &str, to: &str| decode(text.replacen(from, to, 1).as_bytes());
    assert!(
        error(with(
            "\"lifecycle\":\"in-progress\"",
            "\"lifecycle\":\"InProgress\""
        ))
        .contains("unknown lifecycle")
    );
    // Values outside the rules.
    assert!(
        error(with("\"owner\":\"r0\"", "\"owner\":\"someone-else\"")).contains("recorder's actor")
    );
    assert!(
        error(with(
            "\"lifecycle\":\"in-progress\",\"owner\":\"r0\"",
            "\"lifecycle\":\"not-started\",\"owner\":\"r0\""
        ))
        .contains("owner")
    );
}

#[test]
fn decode_rejects_empty_titles_and_line_breaks_in_titles_and_reasons() {
    let (store, ids) = store_with_every_kind();
    let bytes = encode(store.get(&ids[1]).unwrap()).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    let with = |from: &str, to: &str| decode(text.replacen(from, to, 1).as_bytes());
    assert!(error(with("\"title\":\"task\"", "\"title\":\"\"")).contains("empty title"));
    assert!(
        error(with("\"title\":\"task\"", "\"title\":\"two\\nlines\""))
            .contains("title contains a line break")
    );
    assert!(
        error(with("\"reason\":null", "\"reason\":\"two\\nlines\""))
            .contains("reason contains a line break")
    );
}

#[test]
fn decode_rejects_invalid_entity_and_record_ids() {
    let (store, ids) = store_with_every_kind();
    let bytes = encode(store.get(&ids[1]).unwrap()).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    let with = |from: &str, to: &str| decode(text.replacen(from, to, 1).as_bytes());
    assert!(error(with("\"entity\":\"i3\"", "\"entity\":\"I3\"")).contains("EntityId"));
    assert!(error(with("\"parents\":[\"", "\"parents\":[\"zz")).contains("record ID"));
}

#[test]
fn decode_rejects_unknown_fields_among_parent_and_dependency_fields() {
    let (store, ids) = store_with_every_kind();
    let bytes = encode(store.get(&ids[1]).unwrap()).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    let with = |from: &str, to: &str| decode(text.replacen(from, to, 1).as_bytes());
    assert!(error(with("\"parents\":[\"", "\"parents\":[],\"x\":[\"")).contains("unknown field"));
    assert!(error(with("\"needs\":[]", "\"needs\":[],\"extra\":1")).contains("unknown field"));
}

#[test]
fn decode_rejects_non_canonical_record_bytes() {
    let (store, ids) = store_with_every_kind();
    let bytes = encode(store.get(&ids[1]).unwrap()).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    let with = |from: &str, to: &str| decode(text.replacen(from, to, 1).as_bytes());
    // Canonical bytes only: whitespace, key order, offsets and duplicate list items differ.
    assert!(error(with("\"reason\":null,", "\"reason\": null,")).contains("not canonical"));
    assert!(error(with("\"recorder\":{\"actor\":\"r0\",\"data\":{\"session_id\":\"session-1\"}},\"reason\":null", "\"reason\":null,\"recorder\":{\"actor\":\"r0\",\"data\":{\"session_id\":\"session-1\"}}")).contains("not canonical"));
    assert!(error(with("Z\",\"recorder\"", "+00:00\",\"recorder\"")).contains("not canonical"));
    assert!(error(decode(format!("{}\n", text).as_bytes())).contains("more than one line"));
}

#[test]
fn decode_requires_no_parents_for_created_records_and_one_parent_for_edits() {
    let (store, ids) = store_with_every_kind();
    let created = String::from_utf8(encode(store.get(&ids[0]).unwrap()).unwrap()).unwrap();
    let parent = ids[0].to_string();
    assert!(
        error(decode(
            created
                .replacen("\"parents\":[]", &format!("\"parents\":[\"{parent}\"]"), 1)
                .as_bytes()
        ))
        .contains("no parents")
    );
    let edit = String::from_utf8(encode(store.get(&ids[2]).unwrap()).unwrap()).unwrap();
    assert!(
        error(decode(
            edit.replacen(
                &format!("\"parents\":[\"{}\"]", ids[1]),
                "\"parents\":[]",
                1
            )
            .as_bytes()
        ))
        .contains("exactly one parent")
    );
}

#[test]
fn decode_requires_valid_note_nonces_and_nonempty_bodies() {
    let (store, ids) = store_with_every_kind();
    let note = String::from_utf8(encode(store.get(&ids[11]).unwrap()).unwrap()).unwrap();
    assert!(
        error(decode(
            note.replacen("\"nonce\":\"", "\"nonce\":\"g", 1).as_bytes()
        ))
        .contains("nonce")
    );
    assert!(
        error(decode(
            note.replacen("\"body\":\"a note\"", "\"body\":\" \"", 1)
                .as_bytes()
        ))
        .contains("empty Note")
    );
}

#[test]
fn decode_requires_a_resolve_choice_among_its_parents() {
    let (store, ids) = store_with_every_kind();
    let resolve = String::from_utf8(encode(store.get(&ids[10]).unwrap()).unwrap()).unwrap();
    let chosen = resolve
        .split("\"chosen\":\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .to_string();
    assert!(
        error(decode(
            resolve
                .replacen(
                    &format!("\"chosen\":\"{chosen}\""),
                    &format!("\"chosen\":\"{}\"", ids[0]),
                    1
                )
                .as_bytes()
        ))
        .contains("chooses one of its parents")
    );
}

#[test]
fn the_id_is_the_hash_of_the_bytes_so_a_changed_byte_is_a_different_record() {
    let (store, ids) = store_with_every_kind();
    let bytes = encode(store.get(&ids[11]).unwrap()).unwrap();
    let altered = String::from_utf8(bytes.clone())
        .unwrap()
        .replacen("a note", "a mote", 1);
    let (altered_id, _) = decode(altered.as_bytes()).unwrap();
    assert_ne!(altered_id, ids[11]);
    assert_eq!(RecordId::of(altered.as_bytes()), altered_id);
    assert!(error(RecordId::try_from("ABC")).contains("record ID"));
    assert!(RecordId::try_from(ids[0].to_string()).is_ok());
}

/// Built with serde_json's `preserve_order` (as in a build that includes the GUI), `json!` keeps
/// these keys in insertion order; without it the map sorts them anyway. Either way the bytes must
/// list every nested key in sorted order, including objects inside arrays.
#[test]
fn recorder_metadata_keys_are_sorted_at_every_depth() {
    let r = Replica::new("r0");
    let context = Context {
        at: r.tick().at,
        recorder: Some(Recorder {
            actor: "codex".into(),
            data: BTreeMap::from([(
                "nested".into(),
                serde_json::json!({"z": [{"y": 1, "b": {"d": 0, "c": 0}}], "a": 0}),
            )]),
        }),
    };
    let note = r
        .store
        .add_note(&id("i3"), "n".into(), None, context)
        .unwrap();
    let bytes = encode(&Entry::Note(note)).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(
        text.contains(r#""data":{"nested":{"a":0,"z":[{"b":{"c":0,"d":0},"y":1}]}}"#),
        "{text}"
    );
    let (_, decoded) = decode(&bytes).unwrap();
    assert_eq!(encode(&decoded).unwrap(), bytes);
}

#[test]
fn recorder_numbers_and_nested_data_survive_byte_for_byte() {
    let r = Replica::new("r0");
    let context = Context {
        at: r.tick().at,
        recorder: Some(Recorder {
            actor: "codex".into(),
            data: BTreeMap::from([
                ("float".into(), serde_json::json!(1.0)),
                (
                    "big".into(),
                    serde_json::from_str("12345678901234567890123456789").unwrap(),
                ),
                ("exp".into(), serde_json::from_str("1e5").unwrap()),
                (
                    "nested".into(),
                    serde_json::json!({"z": [true, null, "日本語"], "a": {}}),
                ),
            ]),
        }),
    };
    let note = r
        .store
        .add_note(&id("i3"), "n".into(), None, context)
        .unwrap();
    let bytes = encode(&Entry::Note(note)).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(
        text.contains("\"data\":{\"big\":12345678901234567890123456789,\"exp\":1e+5,\"float\":1.0,\"nested\":{\"a\":{},\"z\":[true,null,\"日本語\"]}}"),
        "{text}"
    );
    let (_, decoded) = decode(&bytes).unwrap();
    assert_eq!(encode(&decoded).unwrap(), bytes);
    // Digits beyond f64, tiny exponents and trailing zeros are kept as spelled: the bytes,
    // not a parsed value, are the record. A spelling the encoder would rewrite is rejected
    // instead of silently changing the record's identity.
    for spelled in [
        "1.00",
        "1e-1000",
        "1.23456789012345678901",
        "-18446744073709551617",
    ] {
        let file = text.replacen("1.0,", &format!("{spelled},"), 1);
        let (spelled_id, decoded) = decode(file.as_bytes()).unwrap();
        assert_eq!(encode(&decoded).unwrap(), file.as_bytes());
        assert_eq!(spelled_id, RecordId::of(file.as_bytes()));
    }
    assert!(error(decode(text.replacen("1e+5", "1E5", 1).as_bytes())).contains("not canonical"));
}

#[test]
fn header_round_trips_and_rejects_unknown_formats_and_bad_prefixes() {
    let header = Header::new("demo-1").unwrap();
    let bytes = encode_header(&header).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(text.starts_with("{\"format\":\"axon-records/v2\",\"store\":\"store-"));
    assert!(text.ends_with("\",\"prefix\":\"demo-1\"}\n"));
    assert_eq!(decode_header(&bytes).unwrap(), header);
    for unknown in ["axon-lifecycle/v1", "axon-records/v1", "axon-records/v3"] {
        assert!(
            error(decode_header(
                text.replacen("axon-records/v2", unknown, 1).as_bytes()
            ))
            .contains("unsupported store format"),
            "{unknown}"
        );
    }
    assert!(
        error(decode_header(
            text.replacen("\"prefix\"", "\"name\"", 1).as_bytes()
        ))
        .contains("header")
    );
    assert!(error(decode_header(b"")).contains("empty"));
    for bad in ["", "-a", "a-", "Demo", "a b"] {
        assert!(error(Header::new(bad)).contains("prefix"));
        assert!(
            error(decode_header(text.replacen("demo-1", bad, 1).as_bytes())).contains("prefix")
        );
    }
}

#[test]
fn impossible_transitions_are_rejected_without_their_parent() {
    let (store, ids) = store_with_every_kind();
    let start = store.record(&ids[1]).unwrap().clone();
    let mut reopen = start.clone();
    reopen.kind = RecordKind::Transition(Reopen);
    assert!(error(encode(&Entry::Record(reopen))).contains("never leads"));
    let mut complete = start.clone();
    complete.kind = RecordKind::Transition(Complete);
    complete.after.lifecycle = Lifecycle::NotStarted;
    complete.after.owner = None;
    assert!(error(encode(&Entry::Record(complete))).contains("never leads"));
    let text = String::from_utf8(encode(&Entry::Record(start.clone())).unwrap()).unwrap();
    let edited = text.replacen("\"operation\":\"start\"", "\"operation\":\"reopen\"", 1);
    assert!(error(decode(edited.as_bytes())).contains("never leads"));
}

#[test]
fn started_and_terminal_conversions_are_rejected_without_their_parent() {
    let (store, ids) = store_with_every_kind();
    let start = store.record(&ids[1]).unwrap().clone();
    let mut convert = start.clone();
    convert.kind = RecordKind::Convert;
    convert.after.kind = Kind::Group;
    convert.after.lifecycle = Lifecycle::Completed;
    convert.after.owner = None;
    assert!(error(encode(&Entry::Record(convert))).contains("started or terminal"));
    // A gap store holding only such a record never gets a value from it.
    let mut orphan = Store::new();
    let mut convert = start;
    convert.kind = RecordKind::Convert;
    convert.after.kind = Kind::Group;
    convert.after.lifecycle = Lifecycle::Cancelled;
    convert.after.owner = None;
    assert!(orphan.insert(Entry::Record(convert)).is_err());
    assert!(orphan.is_empty());
}

#[test]
fn creation_records_require_an_undecided_or_not_started_lifecycle() {
    let (store, ids) = store_with_every_kind();
    // A creation record starts only at Undecided or NotStarted.
    let created = String::from_utf8(encode(store.get(&ids[0]).unwrap()).unwrap()).unwrap();
    let completed = created.replacen(
        "\"lifecycle\":\"not-started\"",
        "\"lifecycle\":\"completed\"",
        1,
    );
    assert!(error(decode(completed.as_bytes())).contains("Undecided or NotStarted"));
    let mut record = store.record(&ids[0]).unwrap().clone();
    record.after.lifecycle = Lifecycle::InProgress;
    record.after.owner = Some("setup".into());
    assert!(error(encode(&Entry::Record(record))).contains("Undecided or NotStarted"));
}

#[test]
fn decode_rejects_duplicate_record_parents() {
    let (store, ids) = store_with_every_kind();
    // Duplicate parents are not a canonical list.
    let resolve = String::from_utf8(encode(store.get(&ids[10]).unwrap()).unwrap()).unwrap();
    let first = resolve
        .split("\"parents\":[\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .to_string();
    let doubled = resolve.replacen(
        &format!("\"parents\":[\"{first}\""),
        &format!("\"parents\":[\"{first}\",\"{first}\""),
        1,
    );
    assert!(error(decode(doubled.as_bytes())).contains("duplicate parent"));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn generated_entries_keep_canonical_bytes_and_hash(
        body in any::<String>(),
        json in json_value(),
    ) {
        let (store, ids) = store_with_every_kind();
        for (body, number) in [
            (body.as_str(), "0"), ("日本語\n🚀", "-1"), (body.as_str(), "1.0"),
            ("日本語\n🚀", "1e+5"), (body.as_str(), "12345678901234567890123456789"),
        ] {
        for id in &ids {
            let mut entry = store.get(id).unwrap().clone();
            match &mut entry {
                Entry::Record(record) => record.after.description = body.into(),
                Entry::Note(note) => note.body = if body.trim().is_empty() { "note".into() } else { body.into() },
            }
            if let Entry::Note(note) = &mut entry {
                note.recorder = Some(Recorder {
                    actor: "generated".into(),
                    data: BTreeMap::from([
                        ("number".into(), serde_json::from_str(number).unwrap()),
                        ("value".into(), json.clone()),
                    ]),
                });
            }
            let bytes = encode(&entry).unwrap();
            let expected_id = RecordId::of(&bytes);
            prop_assert_eq!(&expected_id.to_string()[..2], expected_id.subdirectory());
            let (decoded_id, decoded) = decode(&bytes).unwrap();
            prop_assert_eq!(decoded_id, expected_id);
            prop_assert_eq!(&decoded, &entry);
            prop_assert_eq!(encode(&decoded).unwrap(), bytes);
        }
        }
    }



    #[test]
    fn generated_headers_round_trip_and_reject_damage(
        prefix in "[a-z][a-z0-9-]{0,15}[a-z0-9]",
    ) {
        let header = Header::new(&prefix).unwrap();
        let bytes = encode_header(&header).unwrap();
        prop_assert_eq!(decode_header(&bytes).unwrap(), header);
        let text = String::from_utf8(bytes).unwrap();
        for damage in 0..4 {
        let bad = match damage {
            0 => text.replacen(HEADER_FORMAT, "unknown/v2", 1),
            1 => text.replacen("\"prefix\":", "\"other\":", 1),
            2 => text.replacen(&format!("\"prefix\":\"{prefix}\""), "\"prefix\":\"-bad\"", 1),
            _ => text.replacen("\"format\":", "\"format\":null,\"format\":", 1),
        };
        prop_assert!(decode_header(bad.as_bytes()).is_err(), "{bad}");
        }
        for bad_prefix in ["", "-bad", "bad-", "Upper", "space here", "under_score", "日本語"] {
            let bad = text.replacen(&format!("\"prefix\":\"{prefix}\""), &format!("\"prefix\":\"{bad_prefix}\""), 1);
            prop_assert!(decode_header(bad.as_bytes()).is_err(), "{bad}");
        }
    }




}

#[test]
fn record_damage_is_rejected() {
    let body = "description".to_owned();

    let (store, ids) = store_with_every_kind();
    for id in &ids {
        let mut entry = store.get(id).unwrap().clone();
        if let Entry::Record(record) = &mut entry {
            record.after.description = body.clone();
        }
        let bytes = encode(&entry).unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        for damage in 0..5 {
            let bad = match damage {
                0 => text.replacen("\"entity\":\"i3\",", "", 1),
                1 => text.replacen("\"entity\":\"i3\"", "\"entity\":123", 1),
                2 => text.replacen(
                    "\"entity\":\"i3\"",
                    "\"entity\":\"i3\",\"entity\":\"i3\"",
                    1,
                ),
                3 => text.replacen("\"entity\":\"i3\"", "\"entity\":\"i3\",\"extra\":true", 1),
                _ => text.replacen("\"entity\":\"i3\",", "\"entity\": \"i3\",", 1),
            };
            assert!(decode(bad.as_bytes()).is_err(), "{bad}");
        }
    }
}

#[test]
fn parent_field_matrix_rejects_forbidden_changes() {
    let suffix = "changed".to_owned();

    // Columns: kind, lifecycle, owner, text, label, parent, needs, condition.
    const ALLOWED: [[bool; 8]; 10] = [
        [false, true, true, false, false, false, false, false], // transition
        [false, false, false, true, false, false, false, false], // edit
        [false, false, false, false, true, false, false, false], // label
        [false, false, false, false, false, true, false, false], // parent
        [false, false, false, false, false, false, true, false], // dependency
        [false, false, false, false, false, false, false, true], // condition
        [false, false, false, true, true, true, true, false],   // import
        [false, true, true, false, false, false, false, false], // release transition
        [true, false, false, false, false, false, false, false], // convert
        [false; 8],                                             // resolve repeats its chosen parent
    ];
    const REJECTED_WITHOUT_PARENT: [[bool; 8]; 10] = [
        [true, true, true, false, false, false, false, false],
        [true, true, false, false, false, false, false, false],
        [true, true, false, false, false, false, false, false],
        [true, true, false, false, false, false, false, false],
        [true, true, false, false, false, false, false, false],
        [true, true, false, false, false, false, false, false],
        [true, true, false, false, false, false, false, false],
        [true, true, true, false, false, false, false, false],
        [false, false, true, false, false, false, false, false],
        [false, false, true, false, false, false, false, false],
    ];
    let (source, ids) = store_with_every_kind();
    let mut checked = 0;
    for (row, record_id) in ids[1..11].iter().enumerate() {
        for (field, allowed) in ALLOWED[row].iter().enumerate() {
            if *allowed {
                continue;
            }
            let mut store = Store::new();
            let record = source.record(record_id).unwrap();
            let mut ancestors = record.parents.iter().cloned().collect::<Vec<_>>();
            while let Some(parent_id) = ancestors.pop() {
                if store.get(&parent_id).is_some() {
                    continue;
                }
                let parent = source.record(&parent_id).unwrap();
                ancestors.extend(parent.parents.iter().cloned());
                store.insert(Entry::Record(parent.clone())).unwrap();
            }
            let mut damaged = record.clone();
            match field {
                0 => {
                    damaged.after.kind = if damaged.after.kind == Kind::Issue {
                        Kind::Group
                    } else {
                        Kind::Issue
                    }
                }
                1 => {
                    damaged.after.lifecycle = if damaged.after.lifecycle == Lifecycle::NotStarted {
                        Lifecycle::Undecided
                    } else {
                        Lifecycle::NotStarted
                    }
                }
                2 => damaged.after.owner = Some(format!("owner-{suffix}")),
                3 => damaged.after.title.push_str(&suffix),
                4 => {
                    damaged.after.label = if damaged.after.label == Label::Bug {
                        Label::Chore
                    } else {
                        Label::Bug
                    }
                }
                5 => {
                    damaged.after.parent = if damaged.after.parent.is_some() {
                        None
                    } else {
                        Some(id("g0"))
                    }
                }
                6 => {
                    damaged.after.needs.insert(id(&format!("need-{suffix}")));
                }
                _ => damaged.after.condition = Some(format!("exit 0 # {suffix}")),
            }
            let entry = Entry::Record(damaged);
            let encoded = encode(&entry);
            if REJECTED_WITHOUT_PARENT[row][field] {
                assert!(
                    encoded.is_err(),
                    "record {} field {field} unexpectedly encoded",
                    record.kind.name()
                );
                continue;
            }
            let bytes = encoded.unwrap();
            assert!(decode(&bytes).is_ok());
            store.insert(entry).unwrap();
            assert!(
                store.view().is_err(),
                "record {} changed field {field}",
                record.kind.name()
            );
            checked += 1;
        }
    }
    assert!(checked >= 20, "only {checked} encodable forbidden changes");
}

#[test]
fn wrong_entity_or_kind_parent_is_rejected() {
    let suffix = "changed".to_owned();

    for wrong_entity in [false, true] {
        let mut replica = Replica::new("r0");
        let parent = if wrong_entity {
            replica.head("g0")
        } else {
            let conversion = replica
                .store
                .convert(&id("i3"), Kind::Group, None, replica.tick())
                .unwrap()
                .unwrap();
            insert(&mut replica.store, conversion)
        };
        let mut record = replica.store.record(&replica.head("i3")).unwrap().clone();
        record.kind = RecordKind::Edit;
        record.parents = BTreeSet::from([parent]);
        record.at = replica.tick().at;
        record.after.kind = Kind::Issue;
        record.after.title.push_str(&suffix);
        let bytes = encode(&Entry::Record(record.clone())).unwrap();
        assert!(decode(&bytes).is_ok());
        replica.store.insert(Entry::Record(record)).unwrap();
        assert!(replica.store.view().is_err());
    }
}
