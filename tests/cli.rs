mod common;

use common::{TestRepo, assert_failure, assert_success, stderr, stdout};

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

#[test]
fn repeated_transitions_fail_without_changing_state_history_or_timestamp() {
    let repo = TestRepo::new();
    repo.init("test");

    let done_issue = repo.plan("finish once");
    assert_success(&repo.axon(&["start", &done_issue]));
    let before = repo.issue_snapshot(&done_issue);
    let repeated = repo.axon(&["start", &done_issue]);
    assert_failure(&repeated);
    assert!(repeated.stdout.is_empty());
    assert_eq!(
        stderr(&repeated),
        format!("エラー: {done_issue} は着手できません (進行は着手中です)\n")
    );
    assert_eq!(repo.issue_snapshot(&done_issue), before);

    assert_success(&repo.axon(&["done", &done_issue, "--reason", "completed"]));
    let before = repo.issue_snapshot(&done_issue);
    let repeated = repo.axon(&[
        "done",
        &done_issue,
        "--reason",
        "SECRET RESULT MUST NOT BE ECHOED",
    ]);
    assert_failure(&repeated);
    assert!(repeated.stdout.is_empty());
    assert_eq!(
        stderr(&repeated),
        format!("エラー: {done_issue} は終了できません (進行は終了です)\n")
    );
    assert_eq!(repo.issue_snapshot(&done_issue), before);

    let repeated = repo.axon(&["start", &done_issue]);
    assert_failure(&repeated);
    assert_eq!(repo.issue_snapshot(&done_issue), before);

    let released_issue = repo.plan("release once");
    assert_success(&repo.axon(&["start", &released_issue]));
    assert_success(&repo.axon(&["release", &released_issue, "--reason", "handoff"]));
    let before = repo.issue_snapshot(&released_issue);
    let repeated = repo.axon(&[
        "release",
        &released_issue,
        "--reason",
        "SECRET HANDOFF MUST NOT BE ECHOED",
    ]);
    assert_failure(&repeated);
    assert!(repeated.stdout.is_empty());
    assert_eq!(
        stderr(&repeated),
        format!("エラー: {released_issue} は解放できません (進行は未着手です)\n")
    );
    assert_eq!(repo.issue_snapshot(&released_issue), before);

    let decision_issue = repo.capture("decide once");
    assert_success(&repo.axon(&["decide", "accept", &decision_issue, "--reason", "accepted"]));
    let before = repo.issue_snapshot(&decision_issue);
    let repeated = repo.axon(&[
        "decide",
        "accept",
        &decision_issue,
        "--reason",
        "SECRET DECISION MUST NOT BE ECHOED",
    ]);
    assert_failure(&repeated);
    assert!(repeated.stdout.is_empty());
    assert_eq!(
        stderr(&repeated),
        format!("エラー: {decision_issue} の採否はすでに採用です\n")
    );
    assert_eq!(repo.issue_snapshot(&decision_issue), before);

    let scheduled_issue = repo.plan("schedule once");
    assert_success(&repo.axon(&[
        "when",
        "at",
        &scheduled_issue,
        "2099-12-31",
        "--reason",
        "later",
    ]));
    let before = repo.issue_snapshot(&scheduled_issue);
    let repeated = repo.axon(&[
        "when",
        "at",
        &scheduled_issue,
        "2099-12-31",
        "--reason",
        "SECRET SCHEDULE MUST NOT BE ECHOED",
    ]);
    assert_failure(&repeated);
    assert!(repeated.stdout.is_empty());
    assert_eq!(
        stderr(&repeated),
        format!("エラー: {scheduled_issue} の時期はすでに2099-12-31です\n")
    );
    assert_eq!(repo.issue_snapshot(&scheduled_issue), before);

    assert_success(&repo.axon(&["when", "clear", &scheduled_issue]));
    let before = repo.issue_snapshot(&scheduled_issue);
    let repeated = repo.axon(&["when", "clear", &scheduled_issue]);
    assert_failure(&repeated);
    assert_eq!(
        stderr(&repeated),
        format!("エラー: {scheduled_issue} の時期はすでに常に浮上です\n")
    );
    assert_eq!(repo.issue_snapshot(&scheduled_issue), before);
}

#[test]
fn start_requires_the_issue_to_be_ready() {
    let repo = TestRepo::new();
    repo.init("test");

    let undecided = repo.capture("not decided");
    let before = repo.issue_snapshot(&undecided);
    let start = repo.axon(&["start", &undecided]);
    assert_failure(&start);
    assert_eq!(
        stderr(&start),
        format!("エラー: {undecided} は着手できません (採否は未判断です)\n")
    );
    assert_eq!(repo.issue_snapshot(&undecided), before);

    let prerequisite = repo.plan("prerequisite");
    let blocked = repo.plan("blocked");
    assert_success(&repo.axon(&["dep", "add", &blocked, "--needs", &prerequisite]));
    let before = repo.issue_snapshot(&blocked);
    let start = repo.axon(&["start", &blocked]);
    assert_failure(&start);
    assert_eq!(
        stderr(&start),
        format!("エラー: {blocked} は着手できません (未終了の前提があります)\n")
    );
    assert_eq!(repo.issue_snapshot(&blocked), before);

    let deferred = repo.plan("deferred");
    assert_success(&repo.axon(&["when", "at", &deferred, "2099-12-31"]));
    let before = repo.issue_snapshot(&deferred);
    let start = repo.axon(&["start", &deferred]);
    assert_failure(&start);
    assert_eq!(
        stderr(&start),
        format!("エラー: {deferred} は着手できません (時期の条件を満たしていません)\n")
    );
    assert_eq!(repo.issue_snapshot(&deferred), before);
}

#[test]
fn repeated_settings_succeed_without_duplicate_updates() {
    let repo = TestRepo::new();
    repo.init("test");

    let issue = repo.plan("same title");
    let before = repo.issue_snapshot(&issue);
    let write = repo.axon(&["write", &issue, "--title", "same title"]);
    assert_success(&write);
    assert_eq!(stdout(&write), "変更はありません\n");
    assert_eq!(repo.issue_snapshot(&issue), before);

    assert_success(&repo.axon(&["group", "new", "batch", "Batch"]));
    assert_success(&repo.axon(&["group", "set", &issue, "batch"]));
    let before = repo.issue_snapshot(&issue);
    assert_success(&repo.axon(&["group", "set", &issue, "batch"]));
    assert_eq!(repo.issue_snapshot(&issue), before);

    assert_success(&repo.axon(&["group", "unset", &issue]));
    let before = repo.issue_snapshot(&issue);
    assert_success(&repo.axon(&["group", "unset", &issue]));
    assert_eq!(repo.issue_snapshot(&issue), before);

    let prerequisite = repo.plan("dependency");
    assert_success(&repo.axon(&["dep", "add", &issue, "--needs", &prerequisite]));
    let issue_before = repo.issue_snapshot(&issue);
    let prerequisite_before = repo.issue_snapshot(&prerequisite);
    assert_success(&repo.axon(&["dep", "add", &issue, "--needs", &prerequisite]));
    assert_eq!(repo.dep_count("issue_deps"), 1);
    assert_eq!(repo.issue_snapshot(&issue), issue_before);
    assert_eq!(repo.issue_snapshot(&prerequisite), prerequisite_before);

    assert_success(&repo.axon(&["dep", "rm", &issue, "--needs", &prerequisite]));
    assert_success(&repo.axon(&["dep", "rm", &issue, "--needs", &prerequisite]));
    assert_eq!(repo.dep_count("issue_deps"), 0);

    assert_success(&repo.axon(&["group", "new", "foundation", "Foundation"]));
    assert_success(&repo.axon(&["group", "new", "delivery", "Delivery"]));
    assert_success(&repo.axon(&["group", "dep", "add", "delivery", "--needs", "foundation"]));
    assert_success(&repo.axon(&["group", "dep", "add", "delivery", "--needs", "foundation"]));
    assert_eq!(repo.dep_count("group_deps"), 1);
    assert_success(&repo.axon(&["group", "dep", "rm", "delivery", "--needs", "foundation"]));
    assert_success(&repo.axon(&["group", "dep", "rm", "delivery", "--needs", "foundation"]));
    assert_eq!(repo.dep_count("group_deps"), 0);

    let already_rejected = repo.plan("already rejected");
    let newly_rejected = repo.plan("newly rejected");
    assert_success(&repo.axon(&["group", "set", &already_rejected, "batch"]));
    assert_success(&repo.axon(&["group", "set", &newly_rejected, "batch"]));
    assert_success(&repo.axon(&["decide", "reject", &already_rejected]));
    let already_before = repo.issue_snapshot(&already_rejected);

    let reject = repo.axon(&["group", "reject", "batch"]);
    assert_success(&reject);
    assert_eq!(repo.issue_snapshot(&already_rejected), already_before);
    let newly_after = repo.issue_snapshot(&newly_rejected);

    let repeated = repo.axon(&["group", "reject", "batch"]);
    assert_success(&repeated);
    assert_eq!(stdout(&repeated), "batch の 0 件を不採用にしました\n");
    assert_eq!(repo.issue_snapshot(&already_rejected), already_before);
    assert_eq!(repo.issue_snapshot(&newly_rejected), newly_after);
}
