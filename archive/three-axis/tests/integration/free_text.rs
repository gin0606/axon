use crate::common::{TestRepo, assert_success, stdout};

#[test]
fn creation_separators_and_equals_save_literal_text() {
    let repo = TestRepo::new();
    repo.init("text");
    for command in [
        vec!["plan"],
        vec!["capture"],
        vec!["group", "plan"],
        vec!["group", "capture"],
    ] {
        for value in [
            "ordinary text",
            "--color flag handling",
            "--help",
            "-m",
            "  --title spaced  ",
        ] {
            let message = format!("--message={value}");
            let mut args = command.clone();
            args.extend([&message, "--", value]);
            let output = repo.axon(&args);
            assert_success(&output);
            let id = stdout(&output)
                .split_whitespace()
                .next()
                .unwrap()
                .to_string();
            let saved = repo.snapshot(&id);
            assert_eq!(saved.title, value.trim());
            assert_eq!(saved.description.as_deref(), Some(value.trim()));
        }
    }
}

#[test]
fn free_text_options_save_hyphens_and_existing_option_names() {
    let repo = TestRepo::new();
    repo.init("text");
    let id = repo.capture("draft");
    for value in [
        "ordinary text",
        "--color flag handling",
        "--help",
        "-r",
        "  --title spaced  ",
    ] {
        let title = format!("--title={value}");
        for message_option in ["--message", "-m"] {
            let message = format!("{message_option}={value}");
            assert_success(&repo.axon(&["write", &id, &title, &message]));
            let saved = repo.snapshot(&id);
            assert_eq!(saved.title, value.trim());
            assert_eq!(saved.description.as_deref(), Some(value.trim()));
            let output = repo.axon(&["note", "add", &id, &message]);
            assert_success(&output);
            let note_id = stdout(&output)
                .split_whitespace()
                .nth(2)
                .unwrap()
                .to_string();
            assert!(stdout(&repo.axon(&["note", "show", &id, &note_id])).contains(value));
        }
        for reason_option in ["--reason", "-r"] {
            let reason = format!("{reason_option}={value}");
            assert_success(&repo.axon(&["decide", "accept", &id, &reason]));
            assert!(stdout(&repo.axon(&["log", &id])).contains(value));
            assert_success(&repo.axon(&["start", &id]));
            assert_success(&repo.axon(&["release", &id, &reason]));
            assert!(stdout(&repo.axon(&["show", &id])).contains(value));
            assert_success(&repo.axon(&["when", "manual", &id, &reason]));
            assert_success(&repo.axon(&["when", "clear", &id, &reason]));
            assert_success(&repo.axon(&["decide", "undecide", &id, &reason]));
        }
    }
    assert_success(&repo.axon(&["write", &id, "--message="]));
    assert_eq!(repo.snapshot(&id).description, None);
    let before = repo.snapshot(&id);
    assert!(!repo.axon(&["write", &id, "--title="]).status.success());
    assert_eq!(repo.snapshot(&id), before);
}

#[test]
fn invalid_option_values_get_equals_guidance_without_mutation() {
    let repo = TestRepo::new();
    repo.init("text");
    let id = repo.capture("draft");
    let before = repo.snapshot(&id);
    for (prefix, option, canonical) in [
        (vec!["write", &id], "--title", "--title"),
        (
            vec!["write", &id, "--title", "valid title"],
            "--message",
            "--message",
        ),
        (vec!["note", "add", &id], "-m", "--message"),
        (vec!["capture"], "-m", "--message"),
        (vec!["group", "plan"], "--message", "--message"),
        (vec!["decide", "accept", &id], "-r", "--reason"),
        (vec!["release", &id], "--reason", "--reason"),
        (vec!["when", "manual", &id], "-r", "--reason"),
    ] {
        for value in ["--color text", "--help text", "--help", "--title", "--"] {
            let mut args = prefix.clone();
            args.extend([option, value]);
            let output = repo.axon(&args);
            assert_eq!(output.status.code(), Some(2), "{args:?}");
            let error = String::from_utf8(output.stderr).unwrap();
            assert!(
                error.contains(&format!("{canonical}='--help text'")),
                "{args:?}: {error}"
            );
            assert!(!error.contains("a similar argument"), "{error}");
            assert!(!error.contains("as a value, use '--"), "{error}");
            assert_eq!(repo.snapshot(&id), before);
            assert!(!stdout(&repo.axon(&["note", "list", &id])).contains("note-"));
            assert_eq!(repo.entity_count(), 1);
        }
    }
}

#[test]
fn guidance_does_not_change_help_or_confuse_option_tokens_in_values() {
    let repo = TestRepo::new();
    repo.init("text");
    let id = repo.capture("draft");
    for args in [vec!["write", &id, "--help"], vec!["capture", "--help"]] {
        assert_success(&repo.axon(&args));
    }
    for args in [
        vec!["write", &id, "--file", "--message", "--unknown"],
        vec!["capture", "--", "--message", "--unknown"],
        vec!["list", "--kind", "--unknown"],
    ] {
        let output = repo.axon(&args);
        assert!(
            !String::from_utf8(output.stderr)
                .unwrap()
                .contains("attach it with '='")
        );
    }
    for command in [
        vec!["plan"],
        vec!["capture"],
        vec!["group", "plan"],
        vec!["group", "capture"],
    ] {
        let mut args = command;
        args.push("--help text");
        let output = repo.axon(&args);
        assert_eq!(output.status.code(), Some(2));
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(error.contains("put all options before '--'"), "{error}");
    }
    let output = repo.axon(&["capture", "--parnt", "parent", "title"]);
    assert_eq!(output.status.code(), Some(2));
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(
        error.contains("a similar argument exists: '--parent'"),
        "{error}"
    );
    assert!(error.contains("if the positional title"), "{error}");
}
