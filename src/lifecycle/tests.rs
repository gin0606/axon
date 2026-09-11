use super::*;
use chrono::{TimeZone, Utc};
use std::collections::{BTreeMap, BTreeSet};

fn id(text: &str) -> EntityId {
    text.to_string().try_into().unwrap()
}
fn context(seconds: i64) -> Context {
    Context {
        at: Utc.timestamp_opt(seconds, 123456789).unwrap(),
        recorder: Some(Recorder {
            actor: "agent-or-person".into(),
            data: BTreeMap::from([
                ("session_id".into(), serde_json::json!("session-1")),
                (
                    "extra".into(),
                    serde_json::json!({"nested": [true, null, "日本語", 42]}),
                ),
            ]),
        }),
    }
}
fn current(lifecycle: Lifecycle) -> Current {
    Current {
        lifecycle,
        title: "task".into(),
        description: "body\n日本語".into(),
        condition: Some("exit 1".into()),
        parent: None,
        dependencies: BTreeSet::new(),
    }
}
fn fixture(kind: Kind, lifecycle: Lifecycle) -> Snapshot {
    let mut snapshot = Snapshot::new(StoreId::generate());
    snapshot
        .create(id("item"), kind, current(lifecycle), context(10))
        .unwrap();
    snapshot
}
fn roundtrip(snapshot: &Snapshot) {
    let bytes = encode(snapshot).unwrap();
    let restored = decode(&bytes).unwrap();
    assert_eq!(&restored, snapshot);
    assert_eq!(encode(&restored).unwrap(), bytes);
}
fn branch() -> (Snapshot, Snapshot, RecordId, RecordId) {
    let mut base = fixture(Kind::Issue, Lifecycle::NotStarted);
    base.add_note(&id("item"), "before branching".into(), context(11))
        .unwrap();
    let mut left = base.clone();
    let mut right = base;
    left.perform(&id("item"), Operation::Start, None, context(1))
        .unwrap();
    right
        .perform(&id("item"), Operation::Start, None, context(90))
        .unwrap();
    let done = left
        .perform(
            &id("item"),
            Operation::Complete,
            Some("finished".into()),
            context(1),
        )
        .unwrap();
    let released = right
        .perform(
            &id("item"),
            Operation::Release,
            Some("handoff".into()),
            context(0),
        )
        .unwrap();
    left.add_note(&id("item"), "same content".into(), context(3))
        .unwrap();
    right
        .add_note(&id("item"), "same content".into(), context(3))
        .unwrap();
    (left, right, done, released)
}
fn choose(left: &Snapshot, right: &Snapshot, side: Side) -> Snapshot {
    left.integrate(
        right,
        &BTreeMap::from([(id("item"), side)]),
        Some("explicit input choice".into()),
        context(-20),
    )
    .unwrap()
}

#[test]
fn lifecycle_matrix_matches_the_seven_spec_operations() {
    use Lifecycle::*;
    use Operation::*;
    let states = [Undecided, NotStarted, InProgress, Completed, Cancelled];
    let operations = [
        Accept, Withdraw, Start, Release, Complete, Cancel, Reconsider,
    ];
    let allowed = [
        (Undecided, Accept, NotStarted),
        (NotStarted, Withdraw, Undecided),
        (NotStarted, Start, InProgress),
        (InProgress, Release, NotStarted),
        (InProgress, Complete, Completed),
        (Undecided, Cancel, Cancelled),
        (NotStarted, Cancel, Cancelled),
        (InProgress, Cancel, Cancelled),
        (Cancelled, Reconsider, Undecided),
    ];
    for state in states {
        for operation in operations {
            let expected = allowed
                .iter()
                .find(|(s, o, _)| *s == state && *o == operation)
                .map(|(_, _, s)| *s);
            assert_eq!(
                operation.apply(state).ok(),
                expected,
                "{operation:?} from {state:?}"
            );
        }
    }
}
#[test]
fn operation_and_history_are_atomic_and_other_entities_unchanged() {
    let mut snapshot = fixture(Kind::Issue, Lifecycle::Undecided);
    snapshot
        .create(
            id("other"),
            Kind::Group,
            current(Lifecycle::NotStarted),
            context(1),
        )
        .unwrap();
    let other = snapshot.entity(&id("other")).unwrap().clone();
    for operation in [
        Operation::Accept,
        Operation::Start,
        Operation::Release,
        Operation::Withdraw,
        Operation::Cancel,
        Operation::Reconsider,
        Operation::Accept,
        Operation::Start,
        Operation::Complete,
    ] {
        let before = snapshot.entity(&id("item")).unwrap().clone();
        let record = snapshot
            .perform(
                &id("item"),
                operation,
                Some("reason\n理由".into()),
                context(-10),
            )
            .unwrap();
        let after = snapshot.entity(&id("item")).unwrap();
        assert_eq!(after.current.title, before.current.title);
        assert_eq!(after.current.description, before.current.description);
        assert_eq!(after.current.condition, before.current.condition);
        assert_eq!(
            snapshot.state_record(&record).unwrap().parents,
            BTreeSet::from([before.head])
        );
        assert_eq!(
            snapshot.state_record(&record).unwrap().event,
            StateEvent::Transition {
                operation,
                before: before.current.lifecycle,
                after: after.current.lifecycle,
                reason: Some("reason\n理由".into()),
            }
        );
        assert_eq!(snapshot.entity(&id("other")).unwrap(), &other);
        roundtrip(&snapshot);
    }
    for operation in [
        Operation::Accept,
        Operation::Withdraw,
        Operation::Start,
        Operation::Release,
        Operation::Complete,
        Operation::Cancel,
        Operation::Reconsider,
    ] {
        let before = snapshot.clone();
        assert!(
            snapshot
                .perform(&id("item"), operation, None, context(0))
                .is_err()
        );
        assert_eq!(snapshot, before);
    }
}
#[test]
fn text_edit_contract_and_notes_hold_for_issues_and_groups() {
    for kind in [Kind::Issue, Kind::Group] {
        let mut snapshot = fixture(kind, Lifecycle::Undecided);
        for operation in [
            None,
            Some(Operation::Accept),
            Some(Operation::Start),
            Some(Operation::Cancel),
            Some(Operation::Reconsider),
            Some(Operation::Accept),
            Some(Operation::Start),
            Some(Operation::Complete),
        ] {
            if let Some(operation) = operation {
                snapshot
                    .perform(&id("item"), operation, None, context(1))
                    .unwrap();
            }
            let before = snapshot.clone();
            let editable = snapshot
                .entity(&id("item"))
                .unwrap()
                .current
                .lifecycle
                .editable();
            let result = snapshot.write(
                &id("item"),
                Some("changed".into()),
                Some("new description".into()),
            );
            assert_eq!(result.is_ok(), editable);
            assert_eq!(before.states, snapshot.states);
            assert_eq!(before.notes, snapshot.notes);
            if !editable {
                assert_eq!(snapshot, before);
            }
            let entity = snapshot.entity(&id("item")).unwrap().clone();
            snapshot
                .add_note(&id("item"), "supplement".into(), context(1))
                .unwrap();
            assert_eq!(snapshot.entity(&id("item")).unwrap(), &entity);
            roundtrip(&snapshot);
        }
    }
}
#[test]
fn invalid_operations_and_empty_information_leave_no_partial_change() {
    let mut snapshot = fixture(Kind::Issue, Lifecycle::Undecided);
    let before = snapshot.clone();
    assert!(
        snapshot
            .write(
                &id("item"),
                Some(" \n".into()),
                Some("must not persist".into())
            )
            .is_err()
    );
    assert!(
        snapshot
            .add_note(&id("item"), "\n\t".into(), context(0))
            .is_err()
    );
    assert!(
        snapshot
            .perform(&id("item"), Operation::Complete, None, context(0))
            .is_err()
    );
    assert!(
        snapshot
            .add_note(&id("absent"), "note".into(), context(0))
            .is_err()
    );
    assert!(
        snapshot
            .create(
                id("item"),
                Kind::Group,
                current(Lifecycle::NotStarted),
                context(0)
            )
            .is_err()
    );
    assert!(
        snapshot
            .create(
                id("new"),
                Kind::Issue,
                current(Lifecycle::Completed),
                context(0)
            )
            .is_err()
    );
    assert_eq!(snapshot, before);
}
#[test]
fn creation_has_an_origin_not_a_fabricated_transition() {
    for lifecycle in [Lifecycle::Undecided, Lifecycle::NotStarted] {
        let snapshot = fixture(Kind::Issue, lifecycle);
        let records = snapshot.history(&id("item")).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].event,
            StateEvent::Created {
                kind: Kind::Issue,
                initial: lifecycle
            }
        );
        roundtrip(&snapshot);
    }
}
#[test]
fn both_histories_survive_unfinished_selection_and_subsequent_normal_work() {
    let (left, right, done, released) = branch();
    let left_before = left.clone();
    let right_before = right.clone();
    let mut merged = choose(&left, &right, Side::Right);
    assert_eq!(left, left_before);
    assert_eq!(right, right_before);
    assert_eq!(
        merged.entity(&id("item")).unwrap().current.lifecycle,
        Lifecycle::NotStarted
    );
    assert_eq!(
        merged.states.len(),
        left.states.len() + right.states.len() - 1 + 1
    );
    assert!(!merged.precedes(&done, &released).unwrap());
    assert!(!merged.precedes(&released, &done).unwrap());
    let integration = merged.entity(&id("item")).unwrap().head.clone();
    assert!(merged.precedes(&done, &integration).unwrap());
    assert!(merged.precedes(&released, &integration).unwrap());
    let record = merged.state_record(&integration).unwrap();
    let StateEvent::Integration {
        inputs, selected, ..
    } = &record.event
    else {
        panic!()
    };
    assert_eq!(inputs.len(), 2);
    assert_eq!(inputs[*selected].head, released);
    assert_eq!(inputs[0].current.lifecycle, Lifecycle::Completed);
    let notes = merged.notes(&id("item")).unwrap();
    assert_eq!(notes.len(), 3);
    let twins: Vec<_> = notes.iter().filter(|n| n.body == "same content").collect();
    assert_ne!(twins[0].id, twins[1].id);
    assert!(!merged.precedes(&twins[0].id, &twins[1].id).unwrap());
    let tips: BTreeSet<_> = twins.iter().map(|n| n.id.clone()).collect();
    let added = merged
        .add_note(&id("item"), "after merge".into(), context(-100))
        .unwrap();
    assert_eq!(merged.note(&added).unwrap().parents, tips);
    merged
        .write(&id("item"), Some("finish remaining work".into()), None)
        .unwrap();
    let start = merged
        .perform(&id("item"), Operation::Start, None, context(-101))
        .unwrap();
    assert_eq!(
        merged.state_record(&start).unwrap().parents,
        BTreeSet::from([integration])
    );
    merged
        .perform(&id("item"), Operation::Complete, None, context(-102))
        .unwrap();
    assert!(merged.state_record(&done).is_ok());
    assert!(merged.state_record(&released).is_ok());
    roundtrip(&merged);
}
#[test]
fn completed_selection_does_not_create_a_normal_reopen_path() {
    let (left, right, _, _) = branch();
    let mut merged = choose(&left, &right, Side::Left);
    let before = merged.clone();
    assert!(
        merged
            .perform(&id("item"), Operation::Start, None, context(0))
            .is_err()
    );
    assert!(
        merged
            .write(&id("item"), Some("changed".into()), None)
            .is_err()
    );
    assert_eq!(merged, before);
    merged
        .add_note(&id("item"), "correction".into(), context(0))
        .unwrap();
    roundtrip(&merged);
}
#[test]
fn integration_selects_whole_entity_even_when_heads_are_equal() {
    let mut left = fixture(Kind::Issue, Lifecycle::NotStarted);
    let mut right = left.clone();
    left.write(&id("item"), Some("left".into()), None).unwrap();
    right
        .write(&id("item"), None, Some("right".into()))
        .unwrap();
    let merged = choose(&left, &right, Side::Right);
    assert_eq!(
        merged.entity(&id("item")).unwrap().current,
        right.entity(&id("item")).unwrap().current
    );
    assert_eq!(merged.entity(&id("item")).unwrap().current.title, "task");
    roundtrip(&merged);
}
#[test]
fn explicit_integration_handles_new_entities_and_requires_all_choices() {
    let left = fixture(Kind::Issue, Lifecycle::NotStarted);
    let mut right = left.clone();
    right
        .create(
            id("new"),
            Kind::Group,
            current(Lifecycle::Undecided),
            context(1),
        )
        .unwrap();
    assert!(
        left.integrate(
            &right,
            &BTreeMap::from([(id("item"), Side::Left)]),
            None,
            context(1)
        )
        .is_err()
    );
    assert!(
        left.integrate(
            &right,
            &BTreeMap::from([(id("item"), Side::Left), (id("new"), Side::Left)]),
            None,
            context(1)
        )
        .is_err()
    );
    let merged = left
        .integrate(
            &right,
            &BTreeMap::from([(id("item"), Side::Left), (id("new"), Side::Right)]),
            None,
            context(1),
        )
        .unwrap();
    roundtrip(&merged);
}
#[test]
fn equal_or_reverse_times_do_not_determine_causality() {
    let mut snapshot = fixture(Kind::Issue, Lifecycle::NotStarted);
    let root = snapshot.entity(&id("item")).unwrap().root.clone();
    let start = snapshot
        .perform(&id("item"), Operation::Start, None, context(0))
        .unwrap();
    let done = snapshot
        .perform(&id("item"), Operation::Complete, None, context(0))
        .unwrap();
    assert!(snapshot.precedes(&root, &done).unwrap());
    assert!(snapshot.precedes(&start, &done).unwrap());
    assert!(!snapshot.precedes(&done, &start).unwrap());
    assert_eq!(
        snapshot
            .history(&id("item"))
            .unwrap()
            .iter()
            .map(|r| &r.id)
            .collect::<Vec<_>>(),
        vec![&root, &start, &done]
    );
    roundtrip(&snapshot);
}
#[test]
fn absent_recorder_and_empty_data_are_retained_without_permissions() {
    let mut snapshot = fixture(Kind::Issue, Lifecycle::NotStarted);
    let mut ctx = context(0);
    ctx.recorder = None;
    let start = snapshot
        .perform(&id("item"), Operation::Start, None, ctx.clone())
        .unwrap();
    let note = snapshot.add_note(&id("item"), "note".into(), ctx).unwrap();
    assert_eq!(
        snapshot.state_record(&start).unwrap().context.recorder,
        None
    );
    assert_eq!(snapshot.note(&note).unwrap().context.recorder, None);
    let mut ctx = context(-1);
    ctx.recorder = Some(Recorder {
        actor: "another actor".into(),
        data: BTreeMap::new(),
    });
    snapshot
        .perform(&id("item"), Operation::Release, None, ctx)
        .unwrap();
    roundtrip(&snapshot);
}
#[test]
fn same_record_id_with_different_content_is_never_overwritten() {
    let mut left = fixture(Kind::Issue, Lifecycle::NotStarted);
    let note = left
        .add_note(&id("item"), "first".into(), context(0))
        .unwrap();
    let mut right = left.clone();
    right.notes.get_mut(&note).unwrap().body = "different".into();
    right.validate().unwrap();
    let before = left.clone();
    assert!(
        left.integrate(
            &right,
            &BTreeMap::from([(id("item"), Side::Left)]),
            None,
            context(0)
        )
        .is_err()
    );
    assert_eq!(left, before);
}
#[test]
fn identity_collisions_and_different_stores_are_rejected() {
    let left = fixture(Kind::Issue, Lifecycle::NotStarted);
    let mut right = fixture(Kind::Issue, Lifecycle::NotStarted);
    let choices = BTreeMap::from([(id("item"), Side::Right)]);
    assert!(left.integrate(&right, &choices, None, context(0)).is_err());
    right.store = left.store.clone();
    assert!(left.integrate(&right, &choices, None, context(0)).is_err());
}
#[test]
fn malformed_references_cycles_and_wrong_current_proofs_are_rejected() {
    let mut base = fixture(Kind::Issue, Lifecycle::NotStarted);
    let start = base
        .perform(&id("item"), Operation::Start, None, context(0))
        .unwrap();
    let note = base
        .add_note(&id("item"), "note".into(), context(0))
        .unwrap();
    let mut broken = base.clone();
    broken.states.get_mut(&start).unwrap().parents = BTreeSet::from([RecordId::generate()]);
    assert!(broken.validate().is_err());
    let mut broken = base.clone();
    broken.states.get_mut(&start).unwrap().parents = BTreeSet::from([note.clone()]);
    assert!(broken.validate().is_err());
    let mut broken = base.clone();
    broken.notes.get_mut(&note).unwrap().parents = BTreeSet::from([note.clone()]);
    assert!(broken.validate().is_err());
    let mut broken = base.clone();
    broken
        .entities
        .get_mut(&id("item"))
        .unwrap()
        .current
        .lifecycle = Lifecycle::Completed;
    assert!(broken.validate().is_err());
    let mut broken = base.clone();
    broken.states.get_mut(&start).unwrap().event = StateEvent::Transition {
        operation: Operation::Start,
        before: Lifecycle::Completed,
        after: Lifecycle::InProgress,
        reason: None,
    };
    assert!(broken.validate().is_err());
    let mut broken = base.clone();
    broken.entities.clear();
    assert!(broken.validate().is_err());
    let mut broken = base.clone();
    broken.states.get_mut(&start).unwrap().parents = BTreeSet::from([start.clone()]);
    assert!(broken.validate().is_err());
}
#[test]
fn cross_entity_references_and_omitted_branch_are_rejected() {
    let (left, right, done, _) = branch();
    let mut merged = choose(&left, &right, Side::Left);
    merged.entities.get_mut(&id("item")).unwrap().head = done;
    assert!(merged.validate().is_err());
    let mut snapshot = fixture(Kind::Issue, Lifecycle::NotStarted);
    snapshot
        .create(
            id("other"),
            Kind::Issue,
            current(Lifecycle::NotStarted),
            context(0),
        )
        .unwrap();
    let other_root = snapshot.entity(&id("other")).unwrap().root.clone();
    let start = snapshot
        .perform(&id("item"), Operation::Start, None, context(0))
        .unwrap();
    snapshot.states.get_mut(&start).unwrap().parents = BTreeSet::from([other_root]);
    assert!(snapshot.validate().is_err());
}
#[test]
fn integration_cannot_select_an_unproven_state_or_non_input() {
    let (left, right, _, _) = branch();
    let base = choose(&left, &right, Side::Right);
    let head = base.entity(&id("item")).unwrap().head.clone();
    let mut broken = base.clone();
    if let StateEvent::Integration { selected, .. } =
        &mut broken.states.get_mut(&head).unwrap().event
    {
        *selected = 3;
    }
    assert!(broken.validate().is_err());
    let mut broken = base.clone();
    if let StateEvent::Integration { inputs, .. } = &mut broken.states.get_mut(&head).unwrap().event
    {
        inputs[0].current.lifecycle = Lifecycle::Cancelled;
    }
    assert!(broken.validate().is_err());
    let mut broken = base.clone();
    broken.states.get_mut(&head).unwrap().parents.clear();
    assert!(broken.validate().is_err());
}
#[test]
fn codec_normalizes_row_order_and_rejects_duplicate_ids_and_unknown_data() {
    let (left, right, _, _) = branch();
    let snapshot = choose(&left, &right, Side::Right);
    let bytes = encode(&snapshot).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    let mut lines: Vec<_> = text.lines().map(str::to_string).collect();
    lines[1..].reverse();
    assert_eq!(
        encode(&decode(lines.join("\n").as_bytes()).unwrap()).unwrap(),
        bytes
    );
    let duplicate = format!("{text}{}\n", text.lines().nth(1).unwrap());
    assert!(decode(duplicate.as_bytes()).is_err());
    let note_row = text
        .lines()
        .find(|l| l.contains("\"type\":\"Note\""))
        .unwrap();
    let duplicate = format!("{text}{note_row}\n");
    assert!(decode(duplicate.as_bytes()).is_err());
    assert!(
        decode(
            text.replace("axon-lifecycle/v1", "axon-lifecycle/v99")
                .as_bytes()
        )
        .is_err()
    );
    assert!(
        decode(
            text.replace(
                "\"type\":\"Header\"",
                "\"unknown\":true,\"type\":\"Header\""
            )
            .as_bytes()
        )
        .is_err()
    );
    assert!(
        decode(
            text.replace("\"title\":", "\"unknown\":true,\"title\":")
                .as_bytes()
        )
        .is_err()
    );
    assert!(decode(b"").is_err());
    assert!(decode(b"\xff").is_err());
    assert!(decode(format!("{text}\n").as_bytes()).is_err());
}

#[test]
fn shared_in_progress_ancestor_and_repeated_integrations_retain_all_records() {
    let mut base = fixture(Kind::Issue, Lifecycle::NotStarted);
    let start = base
        .perform(&id("item"), Operation::Start, None, context(20))
        .unwrap();
    let mut left = base.clone();
    let mut right = base;
    let done = left
        .perform(&id("item"), Operation::Complete, None, context(1))
        .unwrap();
    let release = right
        .perform(&id("item"), Operation::Release, None, context(2))
        .unwrap();
    let first = choose(&left, &right, Side::Right);
    assert_eq!(first.states.len(), 5);
    assert!(first.precedes(&start, &done).unwrap());
    assert!(first.precedes(&start, &release).unwrap());
    let merged = choose(&first, &left, Side::Left);
    for (id, record) in first.states {
        assert_eq!(merged.state_record(&id).unwrap(), &record);
    }
    assert_eq!(
        merged.entity(&id("item")).unwrap().current.lifecycle,
        Lifecycle::NotStarted
    );
    roundtrip(&merged);
}

#[test]
fn recorder_float_values_survive_canonical_roundtrip_and_record_union() {
    let mut snapshot = Snapshot::new(StoreId::generate());
    let mut ctx = context(0);
    ctx.recorder.as_mut().unwrap().data.insert(
        "values".into(),
        serde_json::json!({
            "small": -1.1563603239824424e-299_f64,
            "other": -3.597847523160892e-242_f64,
            "subnormal": f64::from_bits(1),
            "max": f64::MAX,
        }),
    );
    snapshot
        .create(id("item"), Kind::Issue, current(Lifecycle::NotStarted), ctx)
        .unwrap();
    roundtrip(&snapshot);
    let restored = decode(&encode(&snapshot).unwrap()).unwrap();
    let merged = choose(&snapshot, &restored, Side::Right);
    roundtrip(&merged);
}
#[test]
fn recorder_numbers_are_lossless_and_distinct_payloads_conflict() {
    let mut snapshot = Snapshot::new(StoreId::generate());
    let mut ctx = context(0);
    ctx.recorder.as_mut().unwrap().data.insert(
        "numbers".into(),
        serde_json::json!({"nested": ["numeric-placeholder"]}),
    );
    snapshot
        .create(
            id("item"),
            Kind::Issue,
            current(Lifecycle::NotStarted),
            ctx.clone(),
        )
        .unwrap();
    snapshot
        .add_note(&id("item"), "metadata".into(), ctx)
        .unwrap();
    let template = String::from_utf8(encode(&snapshot).unwrap()).unwrap();
    for (left_number, right_number) in [
        ("18446744073709551617", "18446744073709551618"),
        ("-18446744073709551617", "-18446744073709551618"),
        ("1.23456789012345678901", "1.23456789012345678902"),
        ("1e-1000", "2e-1000"),
    ] {
        let left_bytes = template.replace("\"numeric-placeholder\"", left_number);
        let right_bytes = template.replace("\"numeric-placeholder\"", right_number);
        let left = decode(left_bytes.as_bytes()).unwrap();
        let right = decode(right_bytes.as_bytes()).unwrap();
        assert_eq!(encode(&left).unwrap(), left_bytes.as_bytes());
        assert_eq!(encode(&right).unwrap(), right_bytes.as_bytes());
        assert_ne!(left, right);
        assert!(
            left.integrate(
                &right,
                &BTreeMap::from([(id("item"), Side::Left)]),
                None,
                context(0),
            )
            .is_err()
        );
    }
}

#[test]
fn recorder_number_marker_objects_remain_objects_and_conflict_with_numbers() {
    for marker in [
        "$serde_json::private::Number",
        "$serde_json::private::RawValue",
    ] {
        for text in ["123", "not a number"] {
            let mut snapshot = Snapshot::new(StoreId::generate());
            let mut ctx = context(0);
            let object = serde_json::Value::Object(serde_json::Map::from_iter([(
                marker.into(),
                serde_json::Value::String(text.into()),
            )]));
            ctx.recorder
                .as_mut()
                .unwrap()
                .data
                .insert("nested".into(), object);
            snapshot
                .create(
                    id("item"),
                    Kind::Issue,
                    current(Lifecycle::NotStarted),
                    ctx.clone(),
                )
                .unwrap();
            snapshot
                .add_note(&id("item"), "metadata".into(), ctx)
                .unwrap();
            roundtrip(&snapshot);
            let encoded = String::from_utf8(encode(&snapshot).unwrap()).unwrap();
            let number_bytes = encoded.replace(&format!("{{\"{marker}\":\"{text}\"}}"), "123");
            let number = decode(number_bytes.as_bytes()).unwrap();
            assert!(
                snapshot
                    .integrate(
                        &number,
                        &BTreeMap::from([(id("item"), Side::Left)]),
                        None,
                        context(0)
                    )
                    .is_err()
            );
        }
    }
}

#[test]
fn terminal_integration_head_rejects_changed_text_but_active_text_remains_editable() {
    let (left, right, _, _) = branch();
    for terminal in [left.clone(), {
        let mut cancelled = right.clone();
        cancelled
            .perform(&id("item"), Operation::Cancel, None, context(0))
            .unwrap();
        cancelled
    }] {
        let merged = choose(&terminal, &terminal, Side::Left);
        for field in ["title", "description"] {
            let encoded = String::from_utf8(encode(&merged).unwrap()).unwrap();
            let mut rows: Vec<serde_json::Value> = encoded
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            rows.iter_mut().find(|row| row["type"] == "Entity").unwrap()["value"]["current"]
                [field] = serde_json::json!("tampered terminal text");
            let bytes = rows
                .iter()
                .map(serde_json::to_string)
                .collect::<std::result::Result<Vec<_>, _>>()
                .unwrap()
                .join("\n");
            assert!(decode(bytes.as_bytes()).is_err());
        }
    }
    let mut active = choose(&left, &right, Side::Right);
    active
        .write(
            &id("item"),
            Some("ordinary update".into()),
            Some("new body".into()),
        )
        .unwrap();
    roundtrip(&active);
}

#[test]
fn nested_terminal_integration_inputs_cannot_change_recorded_text() {
    let (left, right, _, _) = branch();
    for terminal in [left, {
        let mut cancelled = right;
        cancelled
            .perform(&id("item"), Operation::Cancel, None, context(0))
            .unwrap();
        cancelled
    }] {
        let first = choose(&terminal, &terminal, Side::Left);
        let second = choose(&first, &first, Side::Left);
        let head = second.entity(&id("item")).unwrap().head.to_string();
        for field in ["title", "description"] {
            for candidate in [0, 1] {
                let encoded = String::from_utf8(encode(&second).unwrap()).unwrap();
                let mut rows: Vec<serde_json::Value> = encoded
                    .lines()
                    .map(|line| serde_json::from_str(line).unwrap())
                    .collect();
                let integration = rows
                    .iter_mut()
                    .find(|row| row["type"] == "State" && row["value"]["id"] == head)
                    .unwrap();
                integration["value"]["event"]["Integration"]["inputs"][candidate]["current"]
                    [field] = serde_json::json!("tampered terminal text");
                if candidate == 0 {
                    rows.iter_mut().find(|row| row["type"] == "Entity").unwrap()["value"]["current"]
                        [field] = serde_json::json!("tampered terminal text");
                }
                let bytes = rows
                    .iter()
                    .map(serde_json::to_string)
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .unwrap()
                    .join("\n");
                assert!(decode(bytes.as_bytes()).is_err());
            }
        }
    }
}

fn register(s: &mut Snapshot, name: &str, kind: Kind, parent: Option<&str>) {
    let mut value = current(Lifecycle::NotStarted);
    value.parent = parent.map(id);
    s.create(id(name), kind, value, context(10)).unwrap();
}
fn perform(s: &mut Snapshot, name: &str, op: Operation) {
    s.perform(&id(name), op, None, context(20)).unwrap();
}
fn rejected(s: &mut Snapshot, action: impl FnOnce(&mut Snapshot) -> Result<()>) {
    let before = s.clone();
    assert!(action(s).is_err());
    assert_eq!(*s, before);
    roundtrip(s);
}

#[test]
fn nested_group_work_requires_explicit_start_and_final_confirmation() {
    let mut s = Snapshot::new(StoreId::generate());
    register(&mut s, "root", Kind::Group, None);
    register(&mut s, "nested", Kind::Group, Some("root"));
    register(&mut s, "child", Kind::Issue, Some("nested"));
    rejected(&mut s, |s| {
        s.perform(&id("child"), Operation::Start, None, context(1))
            .map(|_| ())
    });
    perform(&mut s, "root", Operation::Start);
    perform(&mut s, "nested", Operation::Start);
    perform(&mut s, "child", Operation::Withdraw);
    for op in [Operation::Complete, Operation::Cancel] {
        rejected(&mut s, |s| {
            s.perform(&id("nested"), op, None, context(1)).map(|_| ())
        });
    }
    perform(&mut s, "child", Operation::Accept);
    perform(&mut s, "child", Operation::Start);
    for name in ["root", "nested"] {
        rejected(&mut s, |s| {
            s.perform(&id(name), Operation::Release, None, context(1))
                .map(|_| ())
        });
    }
    perform(&mut s, "child", Operation::Cancel);
    assert_eq!(
        s.entity(&id("nested")).unwrap().current.lifecycle,
        Lifecycle::InProgress
    );
    assert!(
        s.check_operation(&id("nested"), Operation::Complete)
            .is_ok()
    );
    rejected(&mut s, |s| {
        s.perform(&id("root"), Operation::Complete, None, context(1))
            .map(|_| ())
    });
    perform(&mut s, "nested", Operation::Complete);
    perform(&mut s, "root", Operation::Complete);
    rejected(&mut s, |s| {
        s.perform(&id("child"), Operation::Reconsider, None, context(1))
            .map(|_| ())
    });
    rejected(&mut s, |s| s.set_parent(&id("nested"), None));
    roundtrip(&s);
}

#[test]
fn dependency_added_during_work_blocks_completion_without_releasing() {
    let mut s = Snapshot::new(StoreId::generate());
    register(&mut s, "work", Kind::Issue, None);
    register(&mut s, "needs", Kind::Group, None);
    perform(&mut s, "work", Operation::Start);
    let head = s.entity(&id("work")).unwrap().head.clone();
    s.add_dependency(&id("work"), &id("needs")).unwrap();
    assert_eq!(s.entity(&id("work")).unwrap().head, head);
    rejected(&mut s, |s| {
        s.perform(&id("work"), Operation::Complete, None, context(1))
            .map(|_| ())
    });
    perform(&mut s, "needs", Operation::Cancel);
    rejected(&mut s, |s| {
        s.perform(&id("work"), Operation::Complete, None, context(1))
            .map(|_| ())
    });
    perform(&mut s, "work", Operation::Release);
    rejected(&mut s, |s| {
        s.perform(&id("work"), Operation::Start, None, context(1))
            .map(|_| ())
    });
    perform(&mut s, "needs", Operation::Reconsider);
    perform(&mut s, "needs", Operation::Accept);
    perform(&mut s, "needs", Operation::Start);
    rejected(&mut s, |s| {
        s.perform(&id("work"), Operation::Start, None, context(1))
            .map(|_| ())
    });
    perform(&mut s, "needs", Operation::Complete);
    perform(&mut s, "work", Operation::Start);
    perform(&mut s, "work", Operation::Complete);
    rejected(&mut s, |s| s.remove_dependency(&id("work"), &id("needs")));
    roundtrip(&s);
}

#[test]
fn registration_and_relation_cycles_are_atomic_for_dynamic_nested_plans() {
    let mut s = Snapshot::new(StoreId::generate());
    for i in 0..20 {
        let parent = format!("g{}", i - 1);
        register(
            &mut s,
            &format!("g{i}"),
            Kind::Group,
            if i == 0 { None } else { Some(&parent) },
        );
    }
    register(&mut s, "leaf", Kind::Issue, Some("g19"));
    for (source, target) in [("g0", "leaf"), ("leaf", "g0"), ("leaf", "leaf")] {
        rejected(&mut s, |s| s.add_dependency(&id(source), &id(target)));
    }
    rejected(&mut s, |s| s.set_parent(&id("g0"), Some(id("g19"))));
    rejected(&mut s, |s| s.set_parent(&id("g0"), Some(id("leaf"))));
    let mut value = current(Lifecycle::Undecided);
    value.parent = Some(id("g19"));
    value.dependencies.insert(id("g0"));
    rejected(&mut s, |s| {
        s.create(id("new"), Kind::Issue, value, context(1))
    });
    register(&mut s, "other", Kind::Group, None);
    register(&mut s, "otherchild", Kind::Issue, Some("other"));
    s.add_dependency(&id("g0"), &id("otherchild")).unwrap();
    rejected(&mut s, |s| s.add_dependency(&id("other"), &id("leaf")));
    rejected(&mut s, |s| s.set_parent(&id("otherchild"), Some(id("g19"))));
    perform(&mut s, "otherchild", Operation::Cancel);
    rejected(&mut s, |s| s.add_dependency(&id("otherchild"), &id("leaf")));
}

#[test]
fn terminal_group_composition_and_working_subtree_moves() {
    let mut s = Snapshot::new(StoreId::generate());
    for name in ["a", "b", "c"] {
        register(&mut s, name, Kind::Group, None);
    }
    register(&mut s, "child", Kind::Group, Some("a"));
    register(&mut s, "leaf", Kind::Issue, Some("child"));
    for name in ["a", "b", "child", "leaf"] {
        perform(&mut s, name, Operation::Start);
    }
    rejected(&mut s, |s| s.set_parent(&id("child"), Some(id("c"))));
    s.set_parent(&id("child"), Some(id("b"))).unwrap();
    assert_eq!(
        s.entity(&id("leaf")).unwrap().current.parent,
        Some(id("child"))
    );
    s.set_parent(&id("child"), None).unwrap();
    perform(&mut s, "leaf", Operation::Cancel);
    perform(&mut s, "child", Operation::Cancel);
    s.set_parent(&id("child"), Some(id("c"))).unwrap();
    rejected(&mut s, |s| s.set_parent(&id("leaf"), None));
    let mut value = current(Lifecycle::NotStarted);
    value.parent = Some(id("child"));
    rejected(&mut s, |s| {
        s.create(id("new"), Kind::Group, value, context(1))
    });
    perform(&mut s, "child", Operation::Reconsider);
    s.set_parent(&id("leaf"), None).unwrap();
}

#[test]
fn integration_and_decode_enforce_combined_relations() {
    let mut base = Snapshot::new(StoreId::generate());
    register(&mut base, "a", Kind::Group, None);
    register(&mut base, "b", Kind::Issue, None);
    let mut left = base.clone();
    let mut right = base;
    left.add_dependency(&id("a"), &id("b")).unwrap();
    right.add_dependency(&id("b"), &id("a")).unwrap();
    let choices = BTreeMap::from([(id("a"), Side::Left), (id("b"), Side::Right)]);
    assert!(left.integrate(&right, &choices, None, context(1)).is_err());
    left.remove_dependency(&id("a"), &id("b")).unwrap();
    right.remove_dependency(&id("b"), &id("a")).unwrap();
    perform(&mut left, "a", Operation::Cancel);
    perform(&mut right, "b", Operation::Cancel);
    right.set_parent(&id("b"), Some(id("a"))).unwrap();
    assert!(left.integrate(&right, &choices, None, context(1)).is_err());
    // Direct construction also goes through the validator used by the codec.
    right.entities.get_mut(&id("a")).unwrap().current.parent = Some(id("b"));
    assert!(right.validate().is_err());
    assert!(encode(&right).is_err());
}

#[test]
fn condition_edits_are_atomic_and_do_not_change_lifecycle_or_records() {
    let mut snapshot = fixture(Kind::Issue, Lifecycle::NotStarted);
    for operation in [None, Some(Operation::Start), Some(Operation::Complete)] {
        if let Some(operation) = operation {
            snapshot
                .perform(&id("item"), operation, None, context(20))
                .unwrap();
        }
        let before = snapshot.clone();
        snapshot
            .set_condition(&id("item"), Some("exit 23".into()))
            .unwrap();
        assert_eq!(
            snapshot.entity(&id("item")).unwrap().head,
            before.entity(&id("item")).unwrap().head
        );
        assert_eq!(
            snapshot.history(&id("item")).unwrap(),
            before.history(&id("item")).unwrap()
        );
        snapshot.set_condition(&id("item"), None).unwrap();
        let cleared = snapshot.clone();
        assert!(
            snapshot
                .set_condition(&id("item"), Some("  ".into()))
                .is_err()
        );
        assert_eq!(snapshot, cleared);
        assert!(snapshot.set_condition(&id("missing"), None).is_err());
        assert_eq!(snapshot, cleared);
        roundtrip(&snapshot);
    }
}

#[test]
fn candidate_evaluation_matches_boolean_oracle_for_all_three_level_conditions() {
    let mut snapshot = fixture(Kind::Group, Lifecycle::NotStarted);
    let mut child = current(Lifecycle::NotStarted);
    child.parent = Some(id("item"));
    snapshot
        .create(id("nested"), Kind::Group, child, context(11))
        .unwrap();
    let mut leaf = current(Lifecycle::NotStarted);
    leaf.parent = Some(id("nested"));
    snapshot
        .create(id("leaf"), Kind::Issue, leaf, context(12))
        .unwrap();
    let mut draft = current(Lifecycle::Undecided);
    draft.parent = Some(id("nested"));
    snapshot
        .create(id("draft"), Kind::Issue, draft, context(13))
        .unwrap();
    for phase in 0..3 {
        if phase > 0 {
            snapshot
                .perform(
                    &id(if phase == 1 { "item" } else { "nested" }),
                    Operation::Start,
                    None,
                    context(20 + phase),
                )
                .unwrap();
        }
        for bits in 0..16 {
            let values = BTreeMap::from([
                (id("item"), bits & 1 != 0),
                (id("nested"), bits & 2 != 0),
                (id("leaf"), bits & 4 != 0),
                (id("draft"), bits & 8 != 0),
            ]);
            for kind in [CandidateList::Tasks, CandidateList::Triage] {
                let mut calls = Vec::new();
                let actual: Vec<_> = candidates(&snapshot, kind, |entity, _| {
                    calls.push(entity.id.clone());
                    Ok::<_, Error>(values[&entity.id])
                })
                .unwrap()
                .into_iter()
                .map(|e| e.id.clone())
                .collect();
                let expected: BTreeSet<_> = snapshot
                    .entities()
                    .filter(|entity| {
                        let state = entity.current.lifecycle;
                        if matches!(kind, CandidateList::Tasks) && state == Lifecycle::InProgress {
                            return true;
                        }
                        if state
                            != if matches!(kind, CandidateList::Tasks) {
                                Lifecycle::NotStarted
                            } else {
                                Lifecycle::Undecided
                            }
                        {
                            return false;
                        }
                        let mut cursor = *entity;
                        loop {
                            if !values[&cursor.id] {
                                return false;
                            }
                            match &cursor.current.parent {
                                Some(parent) => cursor = snapshot.entity(parent).unwrap(),
                                None => return true,
                            }
                        }
                    })
                    .map(|e| e.id.clone())
                    .collect();
                assert_eq!(actual.into_iter().collect::<BTreeSet<_>>(), expected);
                assert_eq!(calls.len(), calls.iter().collect::<BTreeSet<_>>().len());
                for (index, called) in calls.iter().enumerate() {
                    let mut cursor = snapshot.entity(called).unwrap();
                    while let Some(parent) = &cursor.current.parent {
                        assert!(values[parent]);
                        assert!(calls[..index].contains(parent));
                        cursor = snapshot.entity(parent).unwrap();
                    }
                }
            }
        }
    }
}
