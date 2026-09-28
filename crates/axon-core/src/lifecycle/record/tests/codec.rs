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
            r.tick(),
        )
        .unwrap()
        .unwrap();
    ids.push(insert(&mut r.store, edit));
    ids.push(r.move_to("i3", Some("g0")));
    ids.push(r.add_dep("i3", "g2"));
    let condition = r
        .store
        .set_condition(&id("i3"), Some("exit 0".into()), r.tick())
        .unwrap()
        .unwrap();
    ids.push(insert(&mut r.store, condition));
    let import = r
        .store
        .import(
            &id("i3"),
            "edited".into(),
            "desc".into(),
            None,
            BTreeSet::new(),
            r.tick(),
        )
        .unwrap()
        .unwrap();
    ids.push(insert(&mut r.store, import));
    ids.push(r.op("i3", Release));
    let convert = r
        .store
        .convert(&id("i3"), Kind::Group, r.tick())
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
    assert!(start.contains("\"recorder\":{\"actor\":\"r0\",\"data\":{\"session_id\":\"session-1\"}},\"reason\":null,\"after\":{\"kind\":\"issue\",\"lifecycle\":\"in-progress\",\"owner\":\"r0\",\"title\":\"task\",\"description\":\"body\\n日本語\",\"condition\":null,\"parent\":null,\"needs\":[]}}\n"));
    let created = text(0);
    assert!(created.contains("\"record\":\"created\",\"parents\":[],\"at\":"));
    let resolve = text(9);
    assert!(resolve.contains("\"record\":\"resolve\",\"parents\":[\""));
    assert!(resolve.contains("\"],\"chosen\":\""));
    let note = text(10);
    assert!(note.contains("\"record\":\"note\",\"parents\":[],\"nonce\":\""));
    assert!(note.ends_with("\"reason\":null,\"body\":\"a note\"}\n"));
    assert!(!note.contains("\"after\""));
}

#[test]
fn decode_rejects_truncated_empty_unknown_missing_and_non_canonical_input() {
    let (store, ids) = store_with_every_kind();
    let bytes = encode(store.get(&ids[1]).unwrap()).unwrap();
    assert!(error(decode(b"")).contains("empty"));
    assert!(error(decode(&bytes[..bytes.len() - 1])).contains("line feed"));
    assert!(error(decode(&bytes[..bytes.len() / 2])).contains("line feed"));
    let mut truncated = bytes[..bytes.len() / 2].to_vec();
    truncated.push(b'\n');
    assert!(error(decode(&truncated)).contains("EOF"));
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
    assert!(
        error(with(
            "\"lifecycle\":\"in-progress\"",
            "\"lifecycle\":\"InProgress\""
        ))
        .contains("unknown lifecycle")
    );
    // Values outside the rules.
    assert!(error(with("\"kind\":\"issue\"", "\"kind\":\"group\"")).contains("Group"));
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
    assert!(error(with("\"title\":\"task\"", "\"title\":\"\"")).contains("empty title"));
    assert!(
        error(with("\"title\":\"task\"", "\"title\":\"two\\nlines\""))
            .contains("title contains a line break")
    );
    assert!(
        error(with("\"reason\":null", "\"reason\":\"two\\nlines\""))
            .contains("reason contains a line break")
    );
    assert!(error(with("\"entity\":\"i3\"", "\"entity\":\"I3\"")).contains("EntityId"));
    assert!(error(with("\"parents\":[\"", "\"parents\":[\"zz")).contains("record ID"));
    assert!(error(with("\"parents\":[\"", "\"parents\":[],\"x\":[\"")).contains("unknown field"));
    assert!(error(with("\"needs\":[]", "\"needs\":[],\"extra\":1")).contains("unknown field"));
    // Canonical bytes only: whitespace, key order, offsets and duplicate list items differ.
    assert!(error(with("\"reason\":null,", "\"reason\": null,")).contains("not canonical"));
    assert!(error(with("\"recorder\":{\"actor\":\"r0\",\"data\":{\"session_id\":\"session-1\"}},\"reason\":null", "\"reason\":null,\"recorder\":{\"actor\":\"r0\",\"data\":{\"session_id\":\"session-1\"}}")).contains("not canonical"));
    assert!(error(with("Z\",\"recorder\"", "+00:00\",\"recorder\"")).contains("not canonical"));
    assert!(error(decode(format!("{}\n", text).as_bytes())).contains("more than one line"));
    // Created records have no parents; other records exactly one; notes need a nonce.
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
    let note = String::from_utf8(encode(store.get(&ids[10]).unwrap()).unwrap()).unwrap();
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
    let resolve = String::from_utf8(encode(store.get(&ids[9]).unwrap()).unwrap()).unwrap();
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
    let bytes = encode(store.get(&ids[10]).unwrap()).unwrap();
    let altered = String::from_utf8(bytes.clone())
        .unwrap()
        .replacen("a note", "a mote", 1);
    let (altered_id, _) = decode(altered.as_bytes()).unwrap();
    assert_ne!(altered_id, ids[10]);
    assert_eq!(RecordId::of(altered.as_bytes()), altered_id);
    assert!(error(RecordId::try_from("ABC")).contains("record ID"));
    assert!(RecordId::try_from(ids[0].to_string()).is_ok());
    // A file whose name is not the hash of its bytes is rejected before decoding.
    assert_eq!(
        decode_as(&altered_id, altered.as_bytes()).unwrap(),
        decode(altered.as_bytes()).unwrap().1
    );
    let mismatch = error(decode_as(&ids[10], altered.as_bytes()));
    assert!(mismatch.contains(&ids[10].to_string()) && mismatch.contains(&altered_id.to_string()));
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
    assert!(text.starts_with("{\"format\":\"axon-records/v1\",\"store\":\"store-"));
    assert!(text.ends_with("\",\"prefix\":\"demo-1\"}\n"));
    assert_eq!(decode_header(&bytes).unwrap(), header);
    assert!(
        error(decode_header(
            text.replacen("axon-records/v1", "axon-lifecycle/v1", 1)
                .as_bytes()
        ))
        .contains("unsupported store format")
    );
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

/// A transition that no source lifecycle of its kind leads to is rejected on its own, so a
/// record whose parent is missing cannot smuggle in an impossible state; a conversion of a
/// started or terminal Entity likewise.
#[test]
fn impossible_transitions_and_conversions_are_rejected_without_their_parent() {
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
    // Duplicate parents are not a canonical list.
    let resolve = String::from_utf8(encode(store.get(&ids[9]).unwrap()).unwrap()).unwrap();
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
            prop_assert_eq!(decoded_id, expected_id.clone());
            prop_assert_eq!(&decoded, &entry);
            prop_assert_eq!(&decode_as(&expected_id, &bytes).unwrap(), &decoded);
            prop_assert_eq!(encode(&decoded).unwrap(), bytes);
        }
        }
    }

    #[test]
    fn generated_record_damage_is_rejected(body in "[a-z]{1,12}") {
        let (store, ids) = store_with_every_kind();
        for id in &ids {
        let mut entry = store.get(id).unwrap().clone();
        if let Entry::Record(record) = &mut entry { record.after.description = body.clone(); }
        let bytes = encode(&entry).unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        for damage in 0..5 {
        let bad = match damage {
            0 => text.replacen("\"entity\":\"i3\",", "", 1),
            1 => text.replacen("\"entity\":\"i3\"", "\"entity\":123", 1),
            2 => text.replacen("\"entity\":\"i3\"", "\"entity\":\"i3\",\"entity\":\"i3\"", 1),
            3 => text.replacen("\"entity\":\"i3\"", "\"entity\":\"i3\",\"extra\":true", 1),
            _ => text.replacen("\"entity\":\"i3\",", "\"entity\": \"i3\",", 1),
        };
        prop_assert!(decode(bad.as_bytes()).is_err(), "{bad}");
        let altered = RecordId::of(bad.as_bytes());
        prop_assert_ne!(altered.clone(), id.clone());
        prop_assert!(decode_as(id, bad.as_bytes()).is_err());
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

    #[test]
    fn generated_parent_field_matrix_rejects_forbidden_changes(suffix in "[a-z]{1,8}") {
        // Columns: kind, lifecycle, owner, text, parent, needs, condition.
        const ALLOWED: [[bool; 7]; 9] = [
            [false, true, true, false, false, false, false], // transition
            [false, false, false, true, false, false, false], // edit
            [false, false, false, false, true, false, false], // parent
            [false, false, false, false, false, true, false], // dependency
            [false, false, false, false, false, false, true], // condition
            [false, false, false, true, true, true, false], // import
            [false, true, true, false, false, false, false], // release transition
            [true, false, false, false, false, false, false], // convert
            [false; 7], // resolve repeats its chosen parent
        ];
        const REJECTED_WITHOUT_PARENT: [[bool; 7]; 9] = [
            [true, true, true, false, false, false, false],
            [true, true, false, false, false, false, false],
            [true, true, false, false, false, false, false],
            [true, true, false, false, false, false, false],
            [true, true, false, false, false, false, false],
            [true, true, false, false, false, false, false],
            [true, true, true, false, false, false, false],
            [false, false, true, false, false, false, false],
            [false, false, true, false, false, false, false],
        ];
        let (source, ids) = store_with_every_kind();
        let mut checked = 0;
        for (row, record_id) in ids[1..10].iter().enumerate() {
            for (field, allowed) in ALLOWED[row].iter().enumerate() {
                if *allowed { continue; }
                let mut store = Store::new();
                let record = source.record(record_id).unwrap();
                let mut ancestors = record.parents.iter().cloned().collect::<Vec<_>>();
                while let Some(parent_id) = ancestors.pop() {
                    if store.get(&parent_id).is_some() { continue; }
                    let parent = source.record(&parent_id).unwrap();
                    ancestors.extend(parent.parents.iter().cloned());
                    store.insert(Entry::Record(parent.clone())).unwrap();
                }
                let mut damaged = record.clone();
                match field {
                    0 => damaged.after.kind = if damaged.after.kind == Kind::Issue { Kind::Group } else { Kind::Issue },
                    1 => damaged.after.lifecycle = if damaged.after.lifecycle == Lifecycle::NotStarted { Lifecycle::Undecided } else { Lifecycle::NotStarted },
                    2 => damaged.after.owner = Some(format!("owner-{suffix}")),
                    3 => damaged.after.title.push_str(&suffix),
                    4 => damaged.after.parent = if damaged.after.parent.is_some() { None } else { Some(id("g0")) },
                    5 => { damaged.after.needs.insert(id(&format!("need-{suffix}"))); },
                    _ => damaged.after.condition = Some(format!("exit 0 # {suffix}")),
                }
                let entry = Entry::Record(damaged);
                let encoded = encode(&entry);
                if REJECTED_WITHOUT_PARENT[row][field] {
                    prop_assert!(encoded.is_err(), "record {} field {field} unexpectedly encoded", record.kind.name());
                    continue;
                }
                let bytes = encoded.unwrap();
                prop_assert!(decode(&bytes).is_ok());
                store.insert(entry).unwrap();
                prop_assert!(store.view().is_err(), "record {} changed field {field}", record.kind.name());
                checked += 1;
            }
        }
        prop_assert!(checked >= 20, "only {checked} encodable forbidden changes");
    }

    #[test]
    fn generated_wrong_entity_or_kind_parent_is_rejected(
        suffix in "[a-z]{1,8}",
    ) {
      for wrong_entity in [false, true] {
        let mut replica = Replica::new("r0");
        let parent = if wrong_entity {
            replica.head("g0")
        } else {
            let conversion = replica.store.convert(&id("i3"), Kind::Group, replica.tick()).unwrap().unwrap();
            insert(&mut replica.store, conversion)
        };
        let mut record = replica.store.record(&replica.head("i3")).unwrap().clone();
        record.kind = RecordKind::Edit;
        record.parents = BTreeSet::from([parent]);
        record.at = replica.tick().at;
        record.after.kind = Kind::Issue;
        record.after.title.push_str(&suffix);
        let bytes = encode(&Entry::Record(record.clone())).unwrap();
        prop_assert!(decode(&bytes).is_ok());
        replica.store.insert(Entry::Record(record)).unwrap();
        prop_assert!(replica.store.view().is_err());
      }
    }
}
