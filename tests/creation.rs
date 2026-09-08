mod common;

use common::{TestRepo, assert_failure, assert_success, stdout};
use rusqlite::Connection;
use std::fs;

fn create(repo: &TestRepo, args: &[&str]) -> String {
    let output = repo.axon(args);
    assert_success(&output);
    stdout(&output).split_whitespace().next().unwrap().into()
}

fn saved_bytes(repo: &TestRepo, backend: &str) -> Vec<u8> {
    fs::read(repo.root().join(if backend == "file" {
        ".axon/state.jsonl"
    } else {
        ".axon/axon.db"
    }))
    .unwrap()
}

#[test]
fn four_creators_save_complete_initial_state_and_revision_without_transitions() {
    for backend in ["sqlite", "file"] {
        for command in [
            vec!["plan"],
            vec!["capture"],
            vec!["group", "plan"],
            vec!["group", "capture"],
        ] {
            for condition in [
                vec![],
                vec!["--manual"],
                vec!["--at", "2099-12-31T00:00:00Z"],
                vec!["--after", "TARGET"],
                vec!["--command", "exit 7"],
            ] {
                let repo = TestRepo::new();
                assert_success(&repo.axon(&["init", "test", "--backend", backend]));
                let parent = repo.group_plan("parent");
                let first = repo.plan("first prerequisite");
                let second = repo.group_plan("second prerequisite");
                let mut args = command.clone();
                args.extend([
                    "--parent",
                    parent.strip_prefix("test-").unwrap(),
                    "--needs",
                    &first,
                    "--needs",
                    second.strip_prefix("test-").unwrap(),
                    "--needs",
                    &first,
                    "-m",
                    "complete description",
                ]);
                args.extend(condition.iter().map(|value| {
                    if *value == "TARGET" {
                        first.as_str()
                    } else {
                        *value
                    }
                }));
                args.push("complete title");
                let id = create(&repo, &args);
                let show =
                    repo.axon_in_timezone("UTC", &["show", &id, "--skip-command-evaluation"]);
                assert_success(&show);
                let show = stdout(&show);
                assert!(show.contains("Progress: NotStarted"));
                assert!(!show.contains("Claim:"));
                let accepted = command.last() == Some(&"plan");
                assert!(show.contains(if accepted {
                    "Disposition: Accepted"
                } else {
                    "Disposition: Undecided"
                }));
                assert!(show.contains("Decision history: 0  Progress history: 0"));
                assert!(show.contains(&format!("Parent: {parent}")));
                assert!(show.contains(&first) && show.contains(&second));
                let label = match condition.first().copied() {
                    None => "Always".to_string(),
                    Some("--manual") => "Manual".into(),
                    Some("--at") => "AtDate(2099-12-31T00:00:00+00:00)".into(),
                    Some("--after") => format!("AfterEntity({first})"),
                    _ => "Command(exit 7)".into(),
                };
                assert!(
                    show.contains(&format!("Resurface condition: {label}")),
                    "{show}"
                );
                if accepted {
                    let revision = show
                        .split("fixed at Revision ")
                        .nth(1)
                        .unwrap()
                        .split_whitespace()
                        .next()
                        .unwrap();
                    let revision = stdout(&repo.axon(&["revision", "show", &id, revision]));
                    assert!(
                        revision.contains("complete title")
                            && revision.contains("complete description")
                    );
                    assert!(revision.contains(&format!("Parent: {parent}")));
                    assert_eq!(revision.matches(&first).count(), 1);
                    assert_eq!(revision.matches(&second).count(), 1);
                    assert!(!revision.contains("Resurface condition"));
                } else {
                    assert!(show.contains("Revisions: 0"));
                }
                assert!(stdout(&repo.axon(&["claims"])).is_empty());
                if backend == "sqlite" {
                    let snapshot = repo.snapshot(&id);
                    assert_eq!(snapshot.decision_events, 0);
                    assert_eq!(snapshot.progress_events, 0);
                    let conn = Connection::open(repo.root().join(".axon/axon.db")).unwrap();
                    let count: i64 = conn
                        .query_row(
                            "SELECT count(*) FROM entity_deps WHERE entity_id=?1",
                            [&id],
                            |r| r.get(0),
                        )
                        .unwrap();
                    assert_eq!(count, 2);
                }
            }
        }
    }
}

#[test]
fn rejected_initial_inputs_leave_all_storage_bytes_unchanged() {
    for backend in ["sqlite", "file"] {
        let repo = TestRepo::new();
        assert_success(&repo.axon(&["init", "test", "--backend", backend]));
        let issue = repo.plan("not a group");
        let active = repo.group_plan("active parent");
        let ended = repo.group_plan("ended parent");
        assert_success(&repo.axon(&["start", &ended]));
        assert_success(&repo.axon(&["done", &ended]));
        for command in [
            vec!["plan"],
            vec!["capture"],
            vec!["group", "plan"],
            vec!["group", "capture"],
        ] {
            for options in [
                vec!["--needs", &issue, "--needs", "missing"],
                vec!["--parent", "missing"],
                vec!["--after", "missing"],
                vec!["--parent", &issue, "--needs", &ended],
                vec!["--parent", &ended, "--needs", &issue],
                vec!["--parent", &active, "--needs", &active],
                vec!["--parent", &active, "--after", &active],
                vec!["--at", "2099-02-30T00:00:00Z"],
                vec!["--at", "2099-01-01"],
                vec!["--at", "2099-01-01T00:00:00"],
                vec!["--at", "2099-01-01T00:00Z"],
                vec!["--at", "2099-01-01T00:00:00.1234567890Z"],
                vec!["--at", "2099-01-01T00:00:60Z"],
                vec!["--manual", "--at", "2099-01-01T00:00:00Z"],
                vec!["--manual", "--after", &issue],
                vec!["--manual", "--command", "exit 0"],
                vec!["--at", "2099-01-01T00:00:00Z", "--after", &issue],
                vec!["--at", "2099-01-01T00:00:00Z", "--command", "exit 0"],
                vec!["--after", &issue, "--command", "exit 0"],
            ] {
                let before = saved_bytes(&repo, backend);
                let mut args = command.clone();
                args.extend(options);
                args.push("invalid initial entity");
                assert_failure(&repo.axon(&args));
                assert_eq!(saved_bytes(&repo, backend), before, "{args:?}");
            }
        }
    }
}

#[test]
fn initial_command_never_runs_during_creation_but_normal_reads_evaluate_it() {
    for backend in ["sqlite", "file"] {
        for command in [
            vec!["plan"],
            vec!["capture"],
            vec!["group", "plan"],
            vec!["group", "capture"],
        ] {
            let repo = TestRepo::new();
            assert_success(&repo.axon(&["init", "test", "--backend", backend]));
            let parent = create(
                &repo,
                &[
                    "group",
                    "plan",
                    "--command",
                    "echo parent >> evaluations; exit 7",
                    "parent",
                ],
            );
            let mut args = command;
            args.extend([
                "--parent",
                &parent,
                "--command",
                "echo child >> evaluations; exit 0",
                "child",
            ]);
            let id = create(&repo, &args);
            assert!(!repo.root().join("evaluations").exists());
            assert_success(&repo.axon(&["show", &id, "--skip-command-evaluation"]));
            assert!(!repo.root().join("evaluations").exists());
            assert_failure(&repo.axon(&["show", &id]));
            assert!(repo.root().join("evaluations").exists());
            assert_success(&repo.axon(&["when", "clear", &parent]));
            fs::remove_file(repo.root().join("evaluations")).unwrap();
            assert_success(&repo.axon(&["show", &id]));
            assert_eq!(
                fs::read_to_string(repo.root().join("evaluations")).unwrap(),
                "child\n"
            );
        }
    }
}

#[test]
fn initial_conditions_keep_dependency_meaning_group_gate_and_fixed_declarations() {
    let repo = TestRepo::new();
    repo.init("test");
    let prerequisite = repo.plan("prerequisite");
    let group = create(&repo, &["group", "plan", "--manual", "gate"]);
    let args = [
        "plan",
        "--parent",
        &group,
        "--needs",
        &prerequisite,
        "--after",
        &prerequisite,
        "work",
    ];
    let id = create(&repo, &args);
    let duplicate = create(&repo, &args);
    assert_ne!(id, duplicate);
    assert_failure(&repo.axon(&["dep", "rm", &id, "--needs", &prerequisite]));
    assert_success(&repo.axon(&["decide", "reject", &prerequisite]));
    let show = stdout(&repo.axon(&["show", &id]));
    assert!(show.contains("Surfaced: yes") && show.contains("Orphaned: yes"));
    assert!(show.contains("Active scope: no"));
    assert_success(&repo.axon(&["when", "clear", &group]));
    assert_success(&repo.axon(&["start", &group]));
    assert!(stdout(&repo.axon(&["triage"])).contains(&id));
    repo.undecide(&id);
    assert_success(&repo.axon(&["dep", "rm", &id, "--needs", &prerequisite]));
    repo.accept(&id);
    assert!(stdout(&repo.axon(&["ready"])).contains(&id));
}

#[test]
fn all_creation_help_lists_initial_options() {
    let repo = TestRepo::new();
    for command in [
        vec!["plan"],
        vec!["capture"],
        vec!["group", "plan"],
        vec!["group", "capture"],
    ] {
        let mut args = command;
        args.push("--help");
        let output = repo.axon(&args);
        assert_success(&output);
        let help = stdout(&output);
        for option in [
            "--parent",
            "--needs",
            "--manual",
            "--at",
            "--after",
            "--command",
        ] {
            assert!(help.contains(option), "{args:?}: {option}");
        }
        assert!(help.contains("without execution"));
        assert!(help.contains("RFC 3339"));
    }
}
