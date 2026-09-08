use crate::common::{TestRepo, assert_success, stderr, stdout};
use std::fs;

fn search(repo: &TestRepo, text: &str) -> String {
    let out = repo.axon(&["list", "--search", text]);
    assert_success(&out);
    stdout(&out)
}
fn note(repo: &TestRepo, id: &str, body: &str) -> String {
    let out = repo.axon(&["note", "add", id, "-m", body]);
    assert_success(&out);
    stdout(&out)
        .split_whitespace()
        .find(|x| x.starts_with("note-"))
        .unwrap()
        .to_string()
}

#[test]
fn list_search_fields_literals_order_and_filters_on_both_backends() {
    for backend in ["file", "sqlite"] {
        let repo = TestRepo::new();
        assert_success(&repo.axon(&["init", "test", "--backend", backend]));
        let title = repo.plan("検索語 title Case é %_");
        let desc = repo.capture("description owner");
        assert_success(&repo.axon(&["write", &desc, "-m", "検索語 description"]));
        let only_note = repo.group_plan("note owner");
        let old = note(&repo, &only_note, "検索語 old");
        let new = note(&repo, &only_note, "検索語 new");
        let multiple = repo.capture("検索語 multiple");
        assert_success(&repo.axon(&["write", &multiple, "-m", "検索語"]));
        let multi_note = note(&repo, &multiple, "検索語");
        let unrelated = repo.plan("unrelated");
        let before = stdout(&repo.axon(&["list"]));
        let result = search(&repo, "検索語");
        let expected: Vec<_> = before
            .lines()
            .filter(|line| !line.starts_with(&unrelated))
            .collect();
        let rows: Vec<_> = result.lines().collect();
        assert_eq!(rows.len(), 4);
        for (row, original) in rows.iter().zip(expected) {
            assert!(row.starts_with(original));
        }
        assert!(
            rows.iter()
                .find(|r| r.starts_with(&title))
                .unwrap()
                .ends_with("Matched: title")
        );
        assert!(
            rows.iter()
                .find(|r| r.starts_with(&desc))
                .unwrap()
                .ends_with("Matched: description")
        );
        let mut note_ids = [old.clone(), new];
        note_ids.sort();
        assert!(
            rows.iter()
                .find(|r| r.starts_with(&only_note))
                .unwrap()
                .ends_with(&format!("Matched: {}", note_ids.join(", ")))
        );
        assert!(
            rows.iter()
                .find(|r| r.starts_with(&multiple))
                .unwrap()
                .ends_with(&format!("Matched: title, description, {multi_note}"))
        );
        let full = repo.axon(&["note", "show", &only_note, &old]);
        assert_success(&full);
        assert!(stdout(&full).contains("検索語 old"));
        for literal in ["%", "_", "%_", "Case", "é"] {
            assert!(search(&repo, literal).starts_with(&title));
        }
        for missing in ["case", "e\u{301}", "検索.*", "no match"] {
            assert!(search(&repo, missing).is_empty());
        }
        let filtered = repo.axon(&[
            "list",
            "--search",
            "検索語",
            "--kind",
            "issue",
            "--disposition",
            "accepted",
            "--progress",
            "not-started",
            "--terminal=false",
        ]);
        assert_success(&filtered);
        assert_eq!(stdout(&filtered).lines().count(), 1);
        assert!(stdout(&filtered).starts_with(&title));
        assert_eq!(before, stdout(&repo.axon(&["list"])));
        let history = repo.capture("historical-needle");
        assert_success(&repo.axon(&["decide", "accept", &history, "-r", "reason-needle"]));
        repo.undecide(&history);
        assert_success(&repo.axon(&["write", &history, "--title", "current"]));
        assert!(search(&repo, "historical-needle").is_empty());
        assert!(search(&repo, "reason-needle").is_empty());
        note(&repo, &history, "ordinary body");
        assert!(search(&repo, "test-actor").is_empty());
    }
}

#[test]
fn list_search_excludes_commands_but_preserves_ancestor_evaluation_and_skip() {
    let repo = TestRepo::new();
    repo.init("test");
    let bad = repo.plan("unmatched");
    assert_success(&repo.axon(&[
        "when",
        "command",
        &bad,
        "echo bad >> bad; exit 7",
        "-r",
        "fixture",
    ]));
    let parent = repo.group_plan("ancestor");
    assert_success(&repo.axon(&["start", &parent]));
    let child = repo.plan("needle");
    repo.set_parent(&child, &parent);
    assert_success(&repo.axon(&[
        "when",
        "command",
        &parent,
        "echo ran >> ancestor; exit 0",
        "-r",
        "fixture",
    ]));
    assert!(search(&repo, "needle").starts_with(&child));
    assert!(!repo.root().join("bad").exists());
    assert_eq!(
        fs::read_to_string(repo.root().join("ancestor")).unwrap(),
        "ran\n"
    );
    assert!(search(&repo, "absent").is_empty());
    let skip = repo.axon(&[
        "list",
        "--search",
        "unmatched",
        "--skip-command-evaluation",
        "--trace-conditions",
    ]);
    assert_success(&skip);
    assert!(stdout(&skip).contains("unevaluated"));
    assert!(stdout(&skip).contains("Matched: title"));
    assert!(stderr(&skip).is_empty());
    assert!(!repo.root().join("bad").exists());
    assert_eq!(
        fs::read_to_string(repo.root().join("ancestor")).unwrap(),
        "ran\n"
    );
}

#[test]
fn list_search_input_contract() {
    let repo = TestRepo::new();
    for args in [
        vec!["list", "--search", ""],
        vec!["list", "--search", "a", "--search", "b"],
    ] {
        assert_eq!(repo.axon(&args).status.code(), Some(2));
    }
    let help = repo.axon(&["list", "--help"]);
    assert_success(&help);
    for word in [
        "Case-sensitive",
        "normalization",
        "Empty",
        "literal",
        "Note",
    ] {
        assert!(stdout(&help).contains(word));
    }
    repo.init("test");
    let id = repo.capture("prefix --help text");
    let out = repo.axon(&["list", "--search=--help"]);
    assert_success(&out);
    assert!(stdout(&out).starts_with(&id));
    assert!(search(&repo, " ").starts_with(&id));
}
