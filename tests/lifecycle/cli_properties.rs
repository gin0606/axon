use super::*;
use proptest::prelude::*;

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
    fn generated_prefixes_keep_the_store_boundary(
        middle in "[a-z0-9-]{0,12}",
        invalid in prop::sample::select(vec!["", "-bad", "bad-", "Upper", "space here", "under_score", "日本語", "tab\there"]),
    ) {
        let valid = format!("a{middle}z");
        let good = Fixture::new();
        prop_assert!(good.run(&["init", &valid]).status.success());
        let header = record::decode_header(&fs::read(good.header()).unwrap()).unwrap();
        prop_assert_eq!(header.prefix, valid);
        let bad = Fixture::new();
        let out = bad.run(&["init", "--", invalid]);
        prop_assert_eq!(out.status.code(), Some(1));
        prop_assert!(!bad.header().exists());
        let mut header: serde_json::Value = serde_json::from_slice(&fs::read(good.header()).unwrap()).unwrap();
        header["prefix"] = invalid.into();
        let corrupt = serde_json::to_vec(&header).unwrap();
        fs::write(good.header(), &corrupt).unwrap();
        for args in [vec!["list"], vec!["capture", "--title", "Work"]] {
            prop_assert_eq!(good.run(&args).status.code(), Some(1));
            prop_assert_eq!(fs::read(good.header()).unwrap(), corrupt.as_slice());
        }
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
    fn generated_cli_ids_resolve_exact_unique_and_ambiguous_suffixes(
        tail in "[a-z0-9]{3,8}",
    ) {
        let f = Fixture::new();
        f.init();
        let first = format!("t-a-{tail}");
        let second = format!("t-b-{tail}");
        let longer = format!("other-{first}");
        let unique = format!("b-{tail}");
        f.publish(vec![registration(&f.records(), &first), registration(&f.records(), &second), registration(&f.records(), &longer)]);
        prop_assert!(f.ok(&["show", &first]).starts_with(&first));
        prop_assert!(f.ok(&["show", &longer]).starts_with(&longer));
        prop_assert!(f.ok(&["show", &unique]).starts_with(&second));
        let ambiguous = f.run(&["show", &tail]);
        prop_assert_eq!(ambiguous.status.code(), Some(1));
        let diagnostic = String::from_utf8(ambiguous.stderr).unwrap();
        prop_assert!(diagnostic.contains(&first) && diagnostic.contains(&second));
        prop_assert!(f.ok(&["start", &unique]).contains("Started"));
        prop_assert_eq!(f.current(&first).lifecycle, Lifecycle::NotStarted);
        prop_assert_eq!(f.current(&second).lifecycle, Lifecycle::InProgress);
    }

    #[test]
    fn generated_search_is_literal_current_text_and_skips_excluded_conditions(
        padding in "[a-z]{0,12}",
    ) {
        let f = Fixture::new();
        f.init();
        let title = format!("Needle %_.* é{padding}");
        let description = "line\ne\u{301} body";
        let matching = f.accepted(&title);
        f.ok(&["write", &matching, "-m", description]);
        let finished = f.accepted(&title);
        f.ok(&["write", &finished, "-m", description]);
        f.ok(&["start", &finished]);
        f.ok(&["complete", &finished]);
        let group = f.ok(&["capture", "--kind", "group", "--accept", "--title", &title]);
        let group = group.split_whitespace().next().unwrap().to_owned();
        f.ok(&["write", &group, "-m", description]);
        let excluded = f.accepted("Other");
        f.ok(&["note", "add", &excluded, "-m", "Needle needle é e\u{301} %_.*\n"]);
        f.ok(&["condition", "set", &excluded, "--command", "echo ran > observed; exit 19"]);
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
                prop_assert!(listed.status.success());
                let listed = String::from_utf8(listed.stdout).unwrap();
                prop_assert_eq!(listed.contains(&matching), expected && issue);
                prop_assert_eq!(listed.contains(&finished), expected && completed);
                prop_assert_eq!(listed.contains(&group), expected && group_row);
                prop_assert!(!listed.contains(&excluded));
            }
            let tasks = f.run(&["tasks", "--search", query]);
            prop_assert!(tasks.status.success());
            prop_assert_eq!(String::from_utf8(tasks.stdout).unwrap().contains(&matching), expected);
        }
        let no_query = f.run(&["list", "--search="]);
        prop_assert_eq!(no_query.status.code(), Some(2));
        prop_assert!(!f.0.join("observed").exists());
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
    fn generated_multiline_body_remains_indented_and_preserved(
        first in "[a-zA-Z0-9 ]{0,20}",
        second in "[a-zA-Z0-9 ]{0,20}",
    ) {
        let f = Fixture::new();
        f.init();
        let body = format!("{first}\nDescendants: {second}\n\n日本語");
        let id = f.accepted("Work");
        f.ok(&["write", &id, "-m", &body]);
        prop_assert_eq!(&f.current(&id).description, &body);
        let shown = f.ok(&["show", &id]);
        let first_line = if first.is_empty() { String::new() } else { format!("  {first}") };
        let rendered_body = format!("{first_line}\n  Descendants: {second}\n\n  日本語\n");
        prop_assert!(shown.contains(&rendered_body));
        prop_assert_eq!(shown.lines().filter(|line| line.starts_with("Descendants:")).count(), 0);
        let forged = format!("{}  2020-01-01 00:00 +00:00  human", "0".repeat(64));
        f.ok(&["note", "add", &id, "-m", &format!("{first}\n{forged}\n{second}")]);
        let notes = f.ok(&["note", "list", &id]);
        let indented_forgery = format!("  {forged}\n");
        prop_assert!(notes.contains(&indented_forgery));
        prop_assert_eq!(notes.lines().filter(|line| line.starts_with(&forged)).count(), 0);
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
            vec!["capture", "--title", &value],
            vec!["write", &id, "--title", &value],
            vec!["start", &id, "-r", &value],
        ] {
            let out = f.run(&args);
            prop_assert_eq!(out.status.code(), Some(1));
            prop_assert!(String::from_utf8_lossy(&out.stderr).contains("control character"));
            prop_assert_eq!(f.records(), before.clone());
        }
        for title_len in [199usize, 200, 201] {
            let title = "t".repeat(title_len);
            let before = f.records();
            prop_assert_eq!(f.run(&["capture", "--title", &title]).status.success(), title_len <= 200);
            if title_len > 200 { prop_assert_eq!(f.records(), before); }
        }
        for reason_len in [499usize, 500, 501] {
            let work = f.accepted("Reason boundary");
            let reason = "r".repeat(reason_len);
            let before = f.records();
            prop_assert_eq!(f.run(&["start", &work, "-r", &reason]).status.success(), reason_len <= 500);
            if reason_len > 500 { prop_assert_eq!(f.records(), before); }
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

    #[test]
    fn condition_capture_keeps_byte_edges_and_utf8_boundary(
        exit in prop::sample::select(vec![0, 23]),
        marker in prop::sample::select(vec!["日", "語"]),
    ) {
        let f = Fixture::new();
        f.init();
        let id = f.accepted("condition output");
        for size in [65535usize, 65536, 65537, 80000] {
            let payload = format!("{}{}{}", "A".repeat(32767), marker, "B".repeat(size - 32770));
            fs::write(f.0.join("condition-output"), &payload).unwrap();
            f.ok(&["condition", "set", &id, "--command", &format!("cat condition-output; cat condition-output >&2; exit {exit}")]);
            let out = f.run(&["tasks", "--trace-conditions"]);
            prop_assert_eq!(out.status.success(), exit == 0);
            if exit != 0 { prop_assert!(out.stdout.is_empty()); }
            let diagnostic = String::from_utf8(out.stderr).unwrap();
            let bytes = payload.as_bytes();
            let omitted = size.saturating_sub(65536);
            let expected = if omitted == 0 {
                payload.clone()
            } else {
                format!("{}\n... {omitted} bytes omitted ...\n{}", String::from_utf8_lossy(&bytes[..32768]), String::from_utf8_lossy(&bytes[size - 32768..]))
            };
            prop_assert_eq!(diagnostic.matches(&expected).count(), 2);
            prop_assert_eq!(diagnostic.matches("bytes omitted").count(), if omitted == 0 { 0 } else { 2 });
        }
    }
}
