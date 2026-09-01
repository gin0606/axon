mod common;

use common::{TestRepo, assert_success, stderr, stdout};

#[test]
fn created_issues_are_observable_through_the_query_commands() {
    let repo = TestRepo::new();
    repo.init("test");
    let planned = repo.plan("ship the feature");
    let captured = repo.capture("investigate the risk");

    let ready = repo.axon(&["ready"]);
    assert_success(&ready);
    assert_eq!(stdout(&ready), format!("{planned}  ship the feature\n"));
    assert!(ready.stderr.is_empty());

    let triage = repo.axon(&["triage"]);
    assert_success(&triage);
    assert_eq!(
        stdout(&triage),
        format!("{captured}  未判断    investigate the risk\n")
    );
    assert!(triage.stderr.is_empty());

    let show = repo.axon(&["show", &planned]);
    assert_success(&show);
    assert_eq!(
        stdout(&show),
        format!("{planned}  ship the feature\n進行: 未着手  採否: 採用  時期: 常に浮上\n")
    );
    assert!(show.stderr.is_empty());

    let list = repo.axon(&["list"]);
    assert_success(&list);
    assert_eq!(
        stdout(&list),
        format!(
            "{planned}  [未着手/採用]  ship the feature\n\
             {captured}  [未着手/未判断]  investigate the risk\n"
        )
    );
    assert!(list.stderr.is_empty());
}

#[test]
fn decision_reasons_are_read_back_from_log() {
    let repo = TestRepo::new();
    repo.init("test");
    let issue = repo.capture("make a decision");

    let decide = repo.axon(&["decide", "accept", &issue, "--reason", "required by users"]);
    assert_success(&decide);

    let when = repo.axon(&[
        "when",
        "at",
        &issue,
        "2099-12-31",
        "--reason",
        "wait for the migration",
    ]);
    assert_success(&when);

    let log = repo.axon(&["log", &issue]);
    assert_success(&log);
    let log = stdout(&log);
    assert!(
        log.lines().any(|line| {
            line.contains("test-actor  採否: 未判断 → 採用  (required by users)")
        }),
        "missing decide reason in log:\n{log}"
    );
    assert!(
        log.lines().any(|line| {
            line.contains("test-actor  時期: 2099-12-31 まで後回し  (wait for the migration)")
        }),
        "missing when reason in log:\n{log}"
    );
}

#[test]
fn empty_issue_queries_keep_guidance_out_of_stdout() {
    let repo = TestRepo::new();
    repo.init("test");

    for (command, guidance) in [
        ("ready", "着手できるものはありません\n"),
        ("triage", "判断を待っているものはありません\n"),
        ("list", "issue はまだありません\n"),
    ] {
        let output = repo.axon(&[command]);
        assert_success(&output);
        assert!(
            output.stdout.is_empty(),
            "{command} wrote guidance to stdout"
        );
        assert_eq!(stderr(&output), guidance);
    }
}

#[test]
fn linked_worktrees_share_the_same_database() {
    let repo = TestRepo::new();
    repo.init("test");
    let from_main = repo.plan("created in main worktree");
    let worktree = repo.add_worktree();

    let show = repo.axon_in(&worktree, &["show", &from_main]);
    assert_success(&show);
    assert!(
        stdout(&show).starts_with(&format!("{from_main}  created in main worktree\n")),
        "linked worktree did not read the issue created in the main worktree"
    );

    let create = repo.axon_in(&worktree, &["capture", "created in linked worktree"]);
    assert_success(&create);
    let from_linked = stdout(&create)
        .split_whitespace()
        .next()
        .expect("capture should print an issue id")
        .to_string();

    let list = repo.axon(&["list"]);
    assert_success(&list);
    let list = stdout(&list);
    assert!(list.contains(&format!("{from_main}  [未着手/採用]")));
    assert!(list.contains(&format!("{from_linked}  [未着手/未判断]")));
}
