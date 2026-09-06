mod common;

use common::{TestRepo, assert_success, stderr, stdout};
use std::fs;

fn ids(repo: &TestRepo, args: &[&str]) -> Vec<String> {
    let output = repo.axon(args);
    assert_success(&output);
    stdout(&output)
        .lines()
        .map(|line| line.split_whitespace().next().unwrap().to_string())
        .collect()
}

#[test]
fn list_filters_cover_states_kinds_overlap_and_preserve_storage() {
    let repo = TestRepo::new();
    repo.init("test");
    let mut entities = Vec::new();
    for kind in ["issue", "group"] {
        for progress in ["not-started", "in-progress", "ended"] {
            for disposition in ["undecided", "accepted", "rejected"] {
                let id = if kind == "issue" {
                    repo.plan("same title")
                } else {
                    repo.group_plan("same title")
                };
                if progress != "not-started" {
                    assert_success(&repo.axon(&["start", &id]));
                }
                if progress == "ended" {
                    assert_success(&repo.axon(&["done", &id]));
                }
                match disposition {
                    "undecided" => repo.undecide(&id),
                    "rejected" => {
                        assert_success(&repo.axon(&["decide", "reject", &id, "-r", "fixture"]))
                    }
                    _ => (),
                }
                entities.push((id, kind, progress, disposition));
            }
        }
    }
    let before: Vec<_> = entities
        .iter()
        .map(|(id, ..)| stdout(&repo.axon(&["show", id])))
        .collect();
    let claims = stdout(&repo.axon(&["claims"]));
    let original = ids(&repo, &["list"]);
    assert_eq!(original.len(), 18);
    for kind in [None, Some("issue"), Some("group")] {
        for progress in [
            None,
            Some("not-started"),
            Some("in-progress"),
            Some("ended"),
        ] {
            for disposition in [None, Some("undecided"), Some("accepted"), Some("rejected")] {
                for terminal in [None, Some(false), Some(true)] {
                    let mut args = vec!["list"];
                    if let Some(value) = kind {
                        args.extend(["--kind", value]);
                    }
                    if let Some(value) = progress {
                        args.extend(["--progress", value]);
                    }
                    if let Some(value) = disposition {
                        args.extend(["--disposition", value]);
                    }
                    if let Some(value) = terminal {
                        args.push(if value {
                            "--terminal=true"
                        } else {
                            "--terminal=false"
                        });
                    }
                    let expected: Vec<_> = original
                        .iter()
                        .filter(|id| {
                            let (_, k, p, d) = entities
                                .iter()
                                .find(|(candidate, ..)| candidate == *id)
                                .unwrap();
                            kind.is_none_or(|v| v == *k)
                                && progress.is_none_or(|v| v == *p)
                                && disposition.is_none_or(|v| v == *d)
                                && terminal.is_none_or(|v| v == (*p == "ended" || *d == "rejected"))
                        })
                        .cloned()
                        .collect();
                    assert_eq!(ids(&repo, &args), expected, "{args:?}");
                }
            }
        }
    }
    let after: Vec<_> = entities
        .iter()
        .map(|(id, ..)| stdout(&repo.axon(&["show", id])))
        .collect();
    assert_eq!(before, after);
    assert_eq!(claims, stdout(&repo.axon(&["claims"])));
}

#[test]
fn list_nonterminal_includes_inactive_and_unsurfaced_matches() {
    let repo = TestRepo::new();
    repo.init("test");
    let parent = repo.group_plan("closed gate");
    let child = repo.plan("inactive");
    repo.set_parent(&child, &parent);
    let manual = repo.plan("unsurfaced");
    assert_success(&repo.axon(&["when", "manual", &manual, "-r", "fixture"]));
    let result = ids(
        &repo,
        &[
            "list",
            "--terminal=false",
            "--progress",
            "not-started",
            "--disposition",
            "accepted",
            "--kind",
            "issue",
        ],
    );
    assert!(result.contains(&child));
    assert!(result.contains(&manual));
    assert_eq!(result.len(), 2);
    let ready = ids(&repo, &["ready"]);
    assert!(!ready.contains(&child));
    assert!(!ready.contains(&manual));
    repo.undecide(&manual);
    assert!(!ids(&repo, &["triage"]).contains(&manual));
    assert_eq!(
        ids(&repo, &["list", "--disposition", "undecided"]),
        vec![manual]
    );
}

#[test]
fn list_filters_evaluate_only_retained_rows_and_required_ancestors() {
    let repo = TestRepo::new();
    repo.init("test");
    let excluded = repo.capture("excluded command");
    assert_success(&repo.axon(&[
        "when",
        "command",
        &excluded,
        "echo bad >> excluded; exit 7",
        "-r",
        "fixture",
    ]));
    let retained = repo.plan("retained command");
    assert_success(&repo.axon(&[
        "when",
        "command",
        &retained,
        "echo ok >> retained; exit 0",
        "-r",
        "fixture",
    ]));
    assert_eq!(
        ids(&repo, &["list", "--disposition", "accepted"]),
        vec![retained.clone()]
    );
    assert!(!repo.root().join("excluded").exists());
    assert_eq!(
        fs::read_to_string(repo.root().join("retained")).unwrap(),
        "ok\n"
    );
    let output = repo.axon(&[
        "list",
        "--disposition",
        "undecided",
        "--skip-command-evaluation",
        "--trace-conditions",
    ]);
    assert_success(&output);
    assert!(stdout(&output).contains(&excluded));
    assert!(stdout(&output).contains("unevaluated"));
    assert!(!repo.root().join("excluded").exists());
    assert!(stderr(&output).is_empty());
    let empty = repo.axon(&["list", "--progress", "ended"]);
    assert_success(&empty);
    assert!(stdout(&empty).is_empty());
    assert_eq!(
        fs::read_to_string(repo.root().join("retained")).unwrap(),
        "ok\n"
    );
    let parent = repo.group_plan("ancestor");
    assert_success(&repo.axon(&["start", &parent]));
    repo.set_parent(&retained, &parent);
    assert_success(&repo.axon(&[
        "when",
        "command",
        &parent,
        "echo ancestor >> ancestor; exit 0",
        "-r",
        "fixture",
    ]));
    ids(
        &repo,
        &[
            "list",
            "--disposition",
            "accepted",
            "--kind",
            "issue",
            "--progress",
            "not-started",
            "--terminal=false",
        ],
    );
    assert_eq!(
        fs::read_to_string(repo.root().join("ancestor")).unwrap(),
        "ancestor\n"
    );
    assert!(!repo.root().join("excluded").exists());
}

#[test]
fn list_filter_help_invalid_values_and_repetition() {
    let repo = TestRepo::new();
    let help = repo.axon(&["list", "--help"]);
    assert_success(&help);
    let help = stdout(&help);
    for value in [
        "not-started",
        "in-progress",
        "ended",
        "undecided",
        "accepted",
        "rejected",
        "true",
        "false",
        "AND",
        "inactive",
        "unsurfaced",
    ] {
        assert!(help.contains(value), "{value}: {help}");
    }
    for args in [
        vec!["list", "--progress", "started"],
        vec!["list", "--disposition", "approved"],
        vec!["list", "--terminal=maybe"],
        vec!["list", "--terminal"],
        vec!["list", "--progress", "ended", "--progress", "ended"],
        vec![
            "list",
            "--disposition",
            "accepted",
            "--disposition",
            "rejected",
        ],
        vec!["list", "--terminal=true", "--terminal=false"],
    ] {
        let output = repo.axon(&args);
        assert_eq!(
            output.status.code(),
            Some(2),
            "{args:?}: {}",
            stderr(&output)
        );
        assert!(stdout(&output).is_empty());
        assert!(stderr(&output).contains("error:"));
    }
}
