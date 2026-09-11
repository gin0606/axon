use crate::common::{TestRepo, assert_success, stderr, stdout};
use std::fs;

fn assert_no_terminal_controls(value: &str) {
    assert!(
        value.chars().all(|character| character == '\n'
            || !matches!(character, '\u{00}'..='\u{1f}' | '\u{7f}'..='\u{9f}')),
        "unexpected terminal control in {value:?}"
    );
}

#[test]
fn human_output_escapes_saved_text_and_preserves_generated_round_trips() {
    let repo = TestRepo::new();
    repo.init("safe");
    let title = "title \u{1b}[2J\t\u{7f}";
    let description = "first line\nsecond \u{1b}]0;spoof\u{7}\r\0\u{85}end";
    let actor = "actor \u{1b}[31m\t";
    let reason = "reason \u{1b}[2J\r";

    let description_path = repo.root().join("description.txt");
    fs::write(&description_path, description).unwrap();
    let created = repo.axon_with_env(
        &[
            "capture",
            "--file",
            description_path.to_str().unwrap(),
            "--",
            title,
        ],
        &[("AXON_ACTOR", Some(actor))],
    );
    assert_success(&created);
    let created_output = stdout(&created);
    assert_no_terminal_controls(&created_output);
    assert!(created_output.contains("title \\x1b[2J\\t\\x7f"));
    let id = created_output
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    let saved = repo.snapshot(&id);
    assert_eq!(saved.title, title);
    assert_eq!(saved.description.as_deref(), Some(description));

    let note_body = "note line one\r\nnote \u{1b}]8;;spoof\u{7}\t";
    assert_success(&repo.axon_with_env(
        &["note", "add", &id, "--message", note_body],
        &[("AXON_ACTOR", Some(actor))],
    ));
    assert_success(&repo.axon_with_env(
        &["decide", "accept", &id, "--reason", reason],
        &[("AXON_ACTOR", Some(actor))],
    ));
    let command = "exit 0 # \u{1b}[2J\t";
    let command_confirmation = repo.axon_with_env(
        &["when", "command", &id, command, "--reason", reason],
        &[("AXON_ACTOR", Some(actor))],
    );
    assert_success(&command_confirmation);
    assert_no_terminal_controls(&stdout(&command_confirmation));
    let command_confirmation = stdout(&command_confirmation);
    assert!(
        command_confirmation.contains("\\x1b[2J\\t"),
        "{command_confirmation:?}"
    );
    assert_success(&repo.axon_with_env(&["start", &id], &[("AXON_ACTOR", Some(actor))]));

    for output in [
        repo.axon(&["list"]),
        repo.axon(&["show", &id]),
        repo.axon(&["claims"]),
        repo.axon(&["log", &id]),
        repo.axon(&["note", "list", &id]),
    ] {
        assert_success(&output);
        assert_no_terminal_controls(&stdout(&output));
        assert_no_terminal_controls(&stderr(&output));
    }
    let notes = repo.axon(&["note", "list", &id]);
    assert!(stdout(&notes).contains("note line one\\r"));
    let note_id = stdout(&notes)
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    let note = repo.axon(&["note", "show", &id, &note_id]);
    assert_success(&note);
    assert_no_terminal_controls(&stdout(&note));
    assert!(stdout(&note).contains("note \\x1b]8;;spoof\\x07\\t"));

    let actor_output = repo.axon_with_env(&["actor"], &[("AXON_ACTOR", Some(actor))]);
    assert_success(&actor_output);
    assert_eq!(stdout(&actor_output), "actor \\x1b[31m\\t\n");

    assert_success(&repo.axon_with_env(
        &["release", &id, "--reason", reason],
        &[("AXON_ACTOR", Some(actor))],
    ));
    let shown = repo.axon(&["show", &id, "--skip-command-evaluation"]);
    assert_success(&shown);
    let shown = stdout(&shown);
    assert_no_terminal_controls(&shown);
    for escaped in ["\\x1b", "\\t", "\\r", "\\x00", "\\x7f", "\\x85"] {
        assert!(shown.contains(escaped), "missing {escaped:?} in {shown:?}");
    }

    let exported = repo.axon(&["export", &id]);
    assert_success(&exported);
    let declaration = stdout(&exported);
    let path = repo.root().join("declaration-\u{1b}[2J.yaml");
    fs::write(&path, declaration).unwrap();
    let applied = repo.axon(&["import", "apply", path.to_str().unwrap()]);
    assert_success(&applied);
    assert_no_terminal_controls(&stdout(&applied));
    assert!(stdout(&applied).contains("declaration-\\x1b[2J.yaml"));
    assert_eq!(repo.snapshot(&id).title, title);
    assert_eq!(repo.snapshot(&id).description.as_deref(), Some(description));
}

#[test]
fn group_dependency_output_escapes_external_titles() {
    let repo = TestRepo::new();
    repo.init("safe");
    let group = repo.group_plan("group");
    let child = repo.plan("child");
    let external = repo.plan("external \u{1b}[2J\tend");
    repo.set_parent(&child, &group);
    repo.add_dependency(&child, &external);

    let shown = repo.axon(&["show", &group]);
    assert_success(&shown);
    let shown = stdout(&shown);
    assert_no_terminal_controls(&shown);
    assert!(shown.contains("external \\x1b[2J\\tend"), "{shown:?}");
}

#[test]
fn parser_diagnostics_escape_controls_in_invalid_arguments() {
    let repo = TestRepo::new();
    repo.init("safe");
    let invalid = "bad\u{1b}[2J\t\u{85}";

    let output = repo.axon(&["list", "--kind", invalid]);
    assert!(!output.status.success());
    assert_no_terminal_controls(&stderr(&output));
    for escaped in ["\\x1b", "\\t", "\\x85"] {
        assert!(
            stderr(&output).contains(escaped),
            "missing {escaped:?} in {:?}",
            stderr(&output)
        );
    }
}
