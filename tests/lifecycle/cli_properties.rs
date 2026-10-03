use super::*;

fn excerpt(before: &str, query: &str, after: &str) -> String {
    let left: Vec<char> = before.chars().collect();
    let right: Vec<char> = after.chars().collect();
    let mut visible = String::new();
    if left.len() > 24 {
        visible.push('…');
    }
    visible.extend(left.iter().skip(left.len().saturating_sub(24)));
    visible.push_str(query);
    visible.extend(right.iter().take(24));
    if right.len() > 24 {
        visible.push('…');
    }
    let mut escaped = String::new();
    for character in visible.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\t' => escaped.push_str("\\t"),
            '\r' => escaped.push_str("\\r"),
            '\0'..='\u{1f}' | '\u{7f}'..='\u{9f}' => {
                escaped.push_str(&format!("\\x{:02x}", character as u32))
            }
            '\u{061c}'
            | '\u{200e}'..='\u{200f}'
            | '\u{2028}'..='\u{2029}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2066}'..='\u{2069}' => escaped.extend(character.escape_unicode()),
            _ => escaped.push(character),
        }
    }
    escaped
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    #[test]
    fn generated_valid_prefixes_are_stored_verbatim(
        middle in "[a-z0-9-]{0,12}",
    ) {
        let valid = format!("a{middle}z");
        let good = Fixture::new();
        prop_assert!(good.run(&["init", &valid]).status.success());
        let header = record::decode_header(&fs::read(good.header()).unwrap()).unwrap();
        prop_assert_eq!(header.prefix, valid);
    }

    #[test]
    fn generated_ids_accept_only_lowercase_ascii_digits_and_hyphens(
        valid in "[a-z0-9-]{1,24}",
        invalid in prop::sample::select(vec!["A", "_", " ", ".", "é", "\n", "\u{1b}"])
    ) {
        prop_assert!(EntityId::try_from(valid.clone()).is_ok());
        let bad = format!("{valid}{invalid}");
        prop_assert!(EntityId::try_from(bad).is_err());
        prop_assert!(EntityId::try_from(String::new()).is_err());
    }

    #[test]
    fn generated_note_search_uses_first_literal_match_and_character_context(
        left_len in 0usize..42,
        right_len in 0usize..42,
        left_char in prop::sample::select(vec!['前', '\n', '\\', '\t', '\r', '\u{1b}', '\u{85}', '\u{202e}']),
        right_char in prop::sample::select(vec!['後', '\n', '\\', '\t', '\r', '\u{1b}', '\u{85}', '\u{202e}']),
    ) {
        let f = Fixture::new();
        f.init();
        let id = f.accepted("unrelated title");
        let left = left_char.to_string().repeat(left_len);
        let right = right_char.to_string().repeat(right_len);
        f.ok(&["condition", "set", &id, "--command", "echo ran > observed; exit 19"]);
        for query in ["語", "e\u{301}", "é", "%_.*", "Needle"] {
            let body = format!("{left}{query}{right}{query}");
            let added = f.ok(&["note", "add", &id, "-m", &body]);
            let note_id = added.split_whitespace().nth(2).unwrap();
            let rows = f.ok(&["note", "search", query]);
            prop_assert_eq!(rows.lines().count(), 1);
            prop_assert!(rows.contains(note_id));
            let wanted = excerpt(&left, query, &format!("{right}{query}"));
            prop_assert!(rows.ends_with(&format!("Excerpt: {wanted}\n")), "{rows}");
            let upper = f.run(&["note", "search", &query.to_uppercase()]);
            if query != query.to_uppercase() && !body.contains(&query.to_uppercase()) {
                prop_assert!(upper.status.success() && upper.stdout.is_empty());
            }
        }
        prop_assert!(!f.0.join("observed").exists());
    }

    #[test]
    fn generated_one_line_fields_reject_controls_without_changing_records(
        control in prop::sample::select(vec!['\n', '\r', '\t', '\u{1b}', '\u{7f}', '\u{85}']),
        padding in "[a-z]{0,24}",
    ) {
        let f = Fixture::new();
        f.init();
        let id = f.accepted("Work");
        let value = format!("{padding}{control}suffix");
        let before = f.records();
        for args in [
            vec!["capture", "--label", "chore", "--title", &value],
            vec!["write", &id, "--title", &value],
            vec!["start", &id, "-r", &value],
        ] {
            let out = f.run(&args);
            prop_assert_eq!(out.status.code(), Some(1));
            prop_assert!(out.stdout.is_empty());
            prop_assert!(String::from_utf8_lossy(&out.stderr).contains("control character"));
            prop_assert_eq!(f.records(), before.clone());
        }
    }

    #[test]
    fn same_time_notes_follow_record_id_within_creation_order(
        bodies in prop::collection::vec("[a-z]{1,8}", 3..7),
        later_seconds in prop::collection::vec(1i64..7, 1..5),
        reverse in any::<bool>(),
    ) {
        let f = Fixture::new();
        f.init();
        let records = f.records();
        let at = context();
        let ids = ["t-zzzzzz", "t-aaaaaa"];
        let mut registrations: Vec<_> = ids.iter().map(|id| Entry::Record(records.create(eid(id), current(id), at.clone()).unwrap())).collect();
        if reverse { registrations.reverse(); }
        f.publish(registrations);
        let records = f.records();
        let mut entries = Vec::new();
        let mut expected = Vec::new();
        for (index, body) in bodies.iter().enumerate() {
            let entity = if index < 2 { ids[0] } else { ids[index % 2] };
            let mut when = at.clone();
            if index >= 2 { when.at += chrono::Duration::seconds(later_seconds[(index - 2) % later_seconds.len()]); }
            let note = records.add_note(&eid(entity), format!("find {body}"), None, when.clone()).unwrap();
            let entry = Entry::Note(note);
            expected.push((entity, when.at, entry.id().unwrap().to_string()));
            entries.push(entry);
        }
        if reverse { entries.reverse(); }
        f.publish(entries);
        let entity_order = [ids[1], ids[0]];
        expected.sort_by_key(|(entity, at, id)| (entity_order.iter().position(|candidate| candidate == entity).unwrap(), *at, id.clone()));
        let rows = f.ok(&["note", "search", "find"]);
        prop_assert_eq!(rows.lines().count(), bodies.len());
        for (row, (entity, _, note_id)) in rows.lines().zip(expected) {
            prop_assert!(row.starts_with(entity));
            prop_assert!(row.contains(&note_id));
        }
    }


}

/// The length limits do not depend on generated input, so their boundaries run once.
#[test]
fn one_line_fields_reject_values_over_the_length_limit_without_changing_records() {
    let f = Fixture::new();
    f.init();
    for title_len in [199usize, 200, 201] {
        let title = "t".repeat(title_len);
        let before = f.records();
        let out = f.run(&["capture", "--label", "chore", "--title", &title]);
        if title_len > 200 {
            assert_eq!(out.status.code(), Some(1));
            assert!(failure(out).contains("the limit is"));
            assert_eq!(f.records(), before);
        } else {
            success(out);
        }
    }
    for reason_len in [499usize, 500, 501] {
        let work = f.accepted("Reason boundary");
        let reason = "r".repeat(reason_len);
        let before = f.records();
        let out = f.run(&["start", &work, "-r", &reason]);
        if reason_len > 500 {
            assert_eq!(out.status.code(), Some(1));
            assert!(failure(out).contains("the limit is"));
            assert_eq!(f.records(), before);
        } else {
            success(out);
        }
    }
}

#[test]
fn search_is_literal_current_text_and_skips_excluded_conditions() {
    let f = Fixture::new();
    f.init();
    let title = "Needle %_.* é".to_owned();
    let description = "line\ne\u{301} body";
    let matching = f.accepted(&title);
    f.ok(&["write", &matching, "-m", description]);
    let finished = f.accepted(&title);
    f.ok(&["write", &finished, "-m", description]);
    f.ok(&["start", &finished]);
    f.ok(&["complete", &finished]);
    let group = f.ok(&[
        "capture", "--label", "chore", "--kind", "group", "--accept", "--title", &title,
    ]);
    let group = group.split_whitespace().next().unwrap().to_owned();
    f.ok(&["write", &group, "-m", description]);
    let excluded = f.accepted("Other");
    f.ok(&[
        "note",
        "add",
        &excluded,
        "-m",
        "Needle needle é e\u{301} %_.*\n",
    ]);
    f.ok(&[
        "condition",
        "set",
        &excluded,
        "--command",
        "echo ran > observed; exit 19",
    ]);
    for query in ["Needle", "needle", "é", "e\u{301}", "%_.*", "\n"] {
        let expected = title.contains(query) || description.contains(query);
        for (scope, issue, completed, group_row) in [
            (vec![], true, true, true),
            (vec!["--kind", "issue"], true, true, false),
            (vec!["--kind", "group"], false, false, true),
            (vec!["--lifecycle", "not-started"], true, false, true),
            (vec!["--terminal=false"], true, false, true),
            (vec!["--terminal=true"], false, true, false),
        ] {
            let mut args = vec!["list", "--search", query];
            args.extend(scope);
            let listed = f.run(&args);
            assert!(listed.status.success());
            let listed = String::from_utf8(listed.stdout).unwrap();
            assert_eq!(listed.contains(&matching), expected && issue);
            assert_eq!(listed.contains(&finished), expected && completed);
            assert_eq!(listed.contains(&group), expected && group_row);
            assert!(!listed.contains(&excluded));
        }
        let tasks = f.run(&["tasks", "--search", query]);
        assert!(tasks.status.success());
        assert_eq!(
            String::from_utf8(tasks.stdout).unwrap().contains(&matching),
            expected
        );
    }
    let no_query = f.run(&["list", "--search="]);
    assert_eq!(no_query.status.code(), Some(2));
    assert!(!f.0.join("observed").exists());
}

#[test]
fn condition_capture_keeps_byte_edges_and_utf8_boundary() {
    let f = Fixture::new();
    f.init();
    let id = f.accepted("condition output");
    for exit in [0, 23] {
        for size in [65535usize, 65536, 65537, 80000] {
            let payload = format!("{}日{}", "A".repeat(32767), "B".repeat(size - 32770));
            let stderr_payload = payload.replace('A', "C").replace('B', "D");
            fs::write(f.0.join("condition-output"), &payload).unwrap();
            fs::write(f.0.join("condition-error"), &stderr_payload).unwrap();
            f.ok(&[
                "condition",
                "set",
                &id,
                "--command",
                &format!("cat condition-output; cat condition-error >&2; exit {exit}"),
            ]);
            let out = f.run(&["tasks", "--trace-conditions"]);
            assert_eq!(out.status.success(), exit == 0);
            if exit != 0 {
                assert!(out.stdout.is_empty());
            }
            let diagnostic = String::from_utf8(out.stderr).unwrap();
            let omitted = size.saturating_sub(65536);
            for (label, payload) in [("stdout", payload), ("stderr", stderr_payload)] {
                let bytes = payload.as_bytes();
                let expected = if omitted == 0 {
                    payload.clone()
                } else {
                    format!(
                        "{}\n... {omitted} bytes omitted ...\n{}",
                        String::from_utf8_lossy(&bytes[..32768]),
                        String::from_utf8_lossy(&bytes[size - 32768..])
                    )
                };
                assert_eq!(
                    diagnostic.matches(&format!("{label}:\n{expected}")).count(),
                    1
                );
            }
            assert!(diagnostic.len() < 133000);
            assert_eq!(
                diagnostic.matches("bytes omitted").count(),
                if omitted == 0 { 0 } else { 2 }
            );
        }
    }
}
