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
        format!("{captured}  Undecided  investigate the risk\n")
    );
    assert!(triage.stderr.is_empty());

    let show = repo.axon(&["show", &planned]);
    assert_success(&show);
    assert_eq!(
        stdout(&show),
        format!(
            "{planned}  ship the feature\nProgress: NotStarted  Disposition: Accepted  Resurface condition: Always\n"
        )
    );
    assert!(show.stderr.is_empty());

    let list = repo.axon(&["list"]);
    assert_success(&list);
    assert_eq!(
        stdout(&list),
        format!(
            "{planned}  [NotStarted/Accepted]  ship the feature\n\
             {captured}  [NotStarted/Undecided]  investigate the risk\n"
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
            line.contains("test-actor  Disposition: Undecided -> Accepted  (required by users)")
        }),
        "missing decide reason in log:\n{log}"
    );
    assert!(
        log.lines().any(|line| {
            line.contains(
                "test-actor  Resurface condition: Always -> AtDate(2099-12-31)  (wait for the migration)"
            )
        }),
        "missing when reason in log:\n{log}"
    );
}

#[test]
fn empty_issue_queries_keep_guidance_out_of_stdout() {
    let repo = TestRepo::new();
    repo.init("test");

    for (command, guidance) in [
        ("ready", "No ready issues\n"),
        ("triage", "No issues need triage\n"),
        ("list", "No issues\n"),
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
    assert!(list.contains(&format!("{from_main}  [NotStarted/Accepted]")));
    assert!(list.contains(&format!("{from_linked}  [NotStarted/Undecided]")));
}

#[test]
fn user_provided_text_is_displayed_without_language_changes() {
    let repo = TestRepo::new();
    repo.init("test");
    let issue = repo.capture("日本語の表題");

    assert_success(&repo.axon(&["write", &issue, "--message", "日本語の説明"]));
    assert_success(&repo.axon(&["group", "new", "jp", "日本語グループ"]));
    assert_success(&repo.axon(&["group", "set", &issue, "jp"]));
    assert_success(&repo.axon(&["decide", "accept", &issue, "--reason", "日本語の採用理由"]));

    let show = repo.axon(&["show", &issue]);
    assert_success(&show);
    let show = stdout(&show);
    assert!(show.contains("日本語の表題"), "{show}");
    assert!(show.contains("日本語の説明"), "{show}");
    assert!(show.contains("Group: jp  日本語グループ"), "{show}");

    let log = repo.axon(&["log", &issue]);
    assert_success(&log);
    assert!(stdout(&log).contains("日本語の採用理由"));
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
        format!("Error: {done_issue} cannot be claimed (Progress is InProgress)\n")
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
        format!("Error: {done_issue} cannot be ended (Progress is Ended)\n")
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
        format!("Error: {released_issue} cannot be released (Progress is NotStarted)\n")
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
        format!("Error: {decision_issue}: Disposition is already Accepted\n")
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
        format!("Error: {scheduled_issue}: Resurface condition is already AtDate(2099-12-31)\n")
    );
    assert_eq!(repo.issue_snapshot(&scheduled_issue), before);

    assert_success(&repo.axon(&["when", "clear", &scheduled_issue]));
    let before = repo.issue_snapshot(&scheduled_issue);
    let repeated = repo.axon(&["when", "clear", &scheduled_issue]);
    assert_failure(&repeated);
    assert_eq!(
        stderr(&repeated),
        format!("Error: {scheduled_issue}: Resurface condition is already Always\n")
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
        format!("Error: {undecided} cannot be claimed (Disposition is Undecided)\n")
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
        format!("Error: {blocked} cannot be claimed (an unresolved dependency exists)\n")
    );
    assert_eq!(repo.issue_snapshot(&blocked), before);

    let deferred = repo.plan("deferred");
    assert_success(&repo.axon(&["when", "at", &deferred, "2099-12-31"]));
    let before = repo.issue_snapshot(&deferred);
    let start = repo.axon(&["start", &deferred]);
    assert_failure(&start);
    assert_eq!(
        stderr(&start),
        format!("Error: {deferred} cannot be claimed (resurface condition is not satisfied)\n")
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
    assert_eq!(stdout(&write), "No changes\n");
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
    assert_eq!(stdout(&repeated), "0 issues in batch set to Rejected\n");
    assert_eq!(repo.issue_snapshot(&already_rejected), already_before);
    assert_eq!(repo.issue_snapshot(&newly_rejected), newly_after);
}

#[test]
fn cycle_forming_commands_fail_atomically_with_a_typed_path() {
    let repo = TestRepo::new();
    repo.init("test");

    let dep_a = repo.plan("dependency a");
    let dep_b = repo.plan("dependency b");
    let dep_c = repo.plan("dependency c");
    assert_success(&repo.axon(&["dep", "add", &dep_a, "--needs", &dep_b]));
    assert_success(&repo.axon(&["dep", "add", &dep_b, "--needs", &dep_c]));
    let before = repo.issue_snapshot(&dep_c);
    let cycle = repo.axon(&["dep", "add", &dep_c, "--needs", &dep_a]);
    assert_failure(&cycle);
    assert!(cycle.stdout.is_empty());
    assert_eq!(
        stderr(&cycle),
        format!(
            "Error: cycle would be created: {dep_c} -[issue dependency]-> {dep_a} \
             -[issue dependency]-> {dep_b} -[issue dependency]-> {dep_c}\n"
        )
    );
    assert_eq!(repo.dep_count("issue_deps"), 2);
    assert_eq!(repo.issue_snapshot(&dep_c), before);

    let after_a = repo.plan("resurface a");
    let after_b = repo.plan("resurface b");
    let after_c = repo.plan("resurface c");
    assert_success(&repo.axon(&["when", "after", &after_a, &after_b]));
    assert_success(&repo.axon(&["when", "after", &after_b, &after_c]));
    let before = repo.issue_snapshot(&after_c);
    let cycle = repo.axon(&["when", "after", &after_c, &after_a]);
    assert_failure(&cycle);
    assert_eq!(
        stderr(&cycle),
        format!(
            "Error: cycle would be created: {after_c} -[AfterIssue reference]-> {after_a} \
             -[AfterIssue reference]-> {after_b} -[AfterIssue reference]-> {after_c}\n"
        )
    );
    assert_eq!(repo.issue_snapshot(&after_c), before);

    let mixed_a = repo.plan("mixed a");
    let mixed_b = repo.plan("mixed b");
    let mixed_c = repo.plan("mixed c");
    assert_success(&repo.axon(&["dep", "add", &mixed_a, "--needs", &mixed_b]));
    assert_success(&repo.axon(&["when", "after", &mixed_b, &mixed_c]));
    let cycle = repo.axon(&["dep", "add", &mixed_c, "--needs", &mixed_a]);
    assert_failure(&cycle);
    assert_eq!(
        stderr(&cycle),
        format!(
            "Error: cycle would be created: {mixed_c} -[issue dependency]-> {mixed_a} \
             -[issue dependency]-> {mixed_b} -[AfterIssue reference]-> {mixed_c}\n"
        )
    );

    for group in ["foundation", "platform", "delivery"] {
        assert_success(&repo.axon(&["group", "new", group]));
    }
    assert_success(&repo.axon(&["group", "dep", "add", "foundation", "--needs", "platform"]));
    assert_success(&repo.axon(&["group", "dep", "add", "platform", "--needs", "delivery"]));
    let cycle = repo.axon(&["group", "dep", "add", "delivery", "--needs", "foundation"]);
    assert_failure(&cycle);
    assert_eq!(
        stderr(&cycle),
        "Error: cycle would be created: delivery -[group dependency]-> foundation \
         -[group dependency]-> platform -[group dependency]-> delivery\n"
    );
    assert_eq!(repo.dep_count("group_deps"), 2);
}

#[test]
fn existing_cycles_remain_observable_and_removable() {
    let repo = TestRepo::new();
    repo.init("test");

    let issue_a = repo.plan("existing cycle a");
    let issue_b = repo.plan("existing cycle b");
    repo.insert_issue_dep_unchecked(&issue_a, &issue_b);
    repo.insert_issue_dep_unchecked(&issue_b, &issue_a);
    assert_success(&repo.axon(&["show", &issue_a]));
    assert_success(&repo.axon(&["dep", "rm", &issue_a, "--needs", &issue_b]));
    assert_success(&repo.axon(&["dep", "rm", &issue_b, "--needs", &issue_a]));

    let mixed_a = repo.plan("existing mixed cycle a");
    let mixed_b = repo.plan("existing mixed cycle b");
    repo.insert_issue_dep_unchecked(&mixed_a, &mixed_b);
    repo.set_after_issue_unchecked(&mixed_b, &mixed_a);
    assert_success(&repo.axon(&["show", &mixed_a]));
    assert_success(&repo.axon(&["when", "clear", &mixed_b]));
    assert_success(&repo.axon(&["dep", "rm", &mixed_a, "--needs", &mixed_b]));

    assert_success(&repo.axon(&["group", "new", "cycle-a"]));
    assert_success(&repo.axon(&["group", "new", "cycle-b"]));
    repo.insert_group_dep_unchecked("cycle-a", "cycle-b");
    repo.insert_group_dep_unchecked("cycle-b", "cycle-a");
    assert_success(&repo.axon(&["group", "list"]));
    assert_success(&repo.axon(&["group", "dep", "rm", "cycle-a", "--needs", "cycle-b"]));
    assert_success(&repo.axon(&["group", "dep", "rm", "cycle-b", "--needs", "cycle-a"]));
}
