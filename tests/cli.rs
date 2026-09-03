mod common;

use common::{TestDir, TestRepo, assert_failure, assert_success, stderr, stdout};
use std::fs;
use std::process::Output;

fn created_id(output: &Output) -> String {
    assert_success(output);
    stdout(output)
        .split_whitespace()
        .next()
        .expect("create command prints an ID")
        .to_string()
}

#[test]
fn every_creation_command_accepts_an_initial_description() {
    let repo = TestRepo::new();
    repo.init("test");
    let cases = [
        (
            vec!["plan", "accepted issue", "--message", " issue body "],
            "issue",
            "accepted",
            "issue body",
        ),
        (
            vec!["capture", "captured issue", "-m", "capture body"],
            "issue",
            "undecided",
            "capture body",
        ),
        (
            vec!["group", "plan", "accepted group", "-m", "group body"],
            "group",
            "accepted",
            "group body",
        ),
        (
            vec![
                "group",
                "capture",
                "captured group",
                "--message",
                "follow-up body",
            ],
            "group",
            "undecided",
            "follow-up body",
        ),
    ];

    for (args, kind, disposition, description) in cases {
        let id = created_id(&repo.axon(&args));
        let snapshot = repo.snapshot(&id);
        assert_eq!(snapshot.kind, kind);
        assert_eq!(snapshot.disposition, disposition);
        assert_eq!(snapshot.description.as_deref(), Some(description));
    }
}

#[test]
fn creation_description_files_stdin_and_empty_values_match_write() {
    let repo = TestRepo::new();
    repo.init("test");
    let description = repo.root().join("description.md");
    fs::write(&description, "\n file body \n").unwrap();

    let from_file =
        created_id(&repo.axon(&["plan", "from file", "--file", description.to_str().unwrap()]));
    assert_eq!(
        repo.snapshot(&from_file).description.as_deref(),
        Some("file body")
    );

    let from_stdin = created_id(&repo.axon_with_stdin(
        &["group", "capture", "from stdin", "-F", "-"],
        "\n stdin body \n",
    ));
    assert_eq!(
        repo.snapshot(&from_stdin).description.as_deref(),
        Some("stdin body")
    );

    let empty_message = created_id(&repo.axon(&["capture", "empty message", "-m", "  "]));
    assert_eq!(repo.snapshot(&empty_message).description, None);

    fs::write(&description, " \n").unwrap();
    let empty_file = created_id(&repo.axon(&[
        "group",
        "plan",
        "empty file",
        "-F",
        description.to_str().unwrap(),
    ]));
    assert_eq!(repo.snapshot(&empty_file).description, None);
}

#[test]
fn creation_input_failures_do_not_leave_partial_entities() {
    let repo = TestRepo::new();
    repo.init("test");
    let missing = repo.root().join("missing.md");
    let existing = repo.root().join("description.md");
    fs::write(&existing, "body").unwrap();
    let failures = [
        vec!["plan", "missing file", "-F", missing.to_str().unwrap()],
        vec!["capture", "invalid\ntitle", "-m", "body"],
        vec!["group", "plan", "missing parent", "--parent", "unknown"],
        vec![
            "group",
            "capture",
            "conflicting input",
            "-m",
            "body",
            "-F",
            existing.to_str().unwrap(),
        ],
    ];

    for args in failures {
        let before = repo.entity_count();
        assert_failure(&repo.axon(&args));
        assert_eq!(repo.entity_count(), before, "{args:?}");
    }
}

#[test]
fn issues_and_groups_are_visible_as_entities_and_filterable_by_kind() {
    let repo = TestRepo::new();
    repo.init("test");
    let issue = repo.plan("ship the feature");
    let captured = repo.capture("investigate the risk");
    let group = repo.group_plan("release plan");
    let captured_group = repo.group_capture("possible follow-up");

    let ready = repo.axon(&["ready"]);
    assert_success(&ready);
    let ready = stdout(&ready);
    assert!(ready.contains(&format!("{issue}  Issue  ship the feature")));
    assert!(ready.contains(&format!("{group}  Group  release plan")));
    assert!(!ready.contains(&captured));
    assert!(!ready.contains(&captured_group));

    let issue_only = repo.axon(&["list", "--kind", "issue"]);
    assert_success(&issue_only);
    let issue_only = stdout(&issue_only);
    assert!(issue_only.contains(&issue));
    assert!(issue_only.contains(&captured));
    assert!(!issue_only.contains(&group));

    let group_only = repo.axon(&["triage", "--kind", "group"]);
    assert_success(&group_only);
    assert_eq!(
        stdout(&group_only),
        format!("{captured_group}  Group  Undecided  possible follow-up\n")
    );

    for id in [&issue, &group] {
        let show = repo.axon(&["show", id]);
        assert_success(&show);
        assert!(stdout(&show).starts_with(&format!("{id}  ")));
    }
}

#[test]
fn a_group_explicitly_opens_and_completes_its_plan_scope() {
    let repo = TestRepo::new();
    repo.init("test");
    let group = repo.group_plan("delivery");
    let issue = repo.plan("implementation");
    repo.set_parent(&issue, &group);

    let ready = stdout(&repo.axon(&["ready"]));
    assert!(ready.contains(&group));
    assert!(!ready.contains(&issue));
    assert_success(&repo.axon(&["start", &group]));
    let ready = stdout(&repo.axon(&["ready"]));
    assert!(!ready.contains(&group));
    assert!(ready.contains(&issue));

    assert_success(&repo.axon(&["start", &issue]));
    let premature = repo.axon(&["done", &group]);
    assert_failure(&premature);
    assert!(stderr(&premature).contains("non-terminal descendants"));
    assert_success(&repo.axon(&["done", &issue]));
    assert_success(&repo.axon(&["done", &group]));

    let show = stdout(&repo.axon(&["show", &group]));
    assert!(show.contains("Progress: Ended"));
    assert!(show.contains(
        "Direct children: 1 (Issue: 1, Group: 0, NotStarted: 0, InProgress: 0, Ended: 1, Undecided: 0, Accepted: 1, Rejected: 0, Terminal: 1)"
    ));
    assert!(show.contains(
        "Descendants: 1 (Issue: 1, Group: 0, NotStarted: 0, InProgress: 0, Ended: 1, Undecided: 0, Accepted: 1, Rejected: 0, Terminal: 1)"
    ));
}

#[test]
fn release_of_a_group_waits_for_active_descendants() {
    let repo = TestRepo::new();
    repo.init("test");
    let group = repo.group_plan("delegated plan");
    let issue = repo.plan("delegated task");
    repo.set_parent(&issue, &group);
    assert_success(&repo.axon(&["start", &group]));
    assert_success(&repo.axon(&["start", &issue]));

    let release = repo.axon(&["release", &group]);
    assert_failure(&release);
    assert!(stderr(&release).contains("InProgress descendants"));
    assert_success(&repo.axon(&["release", &issue]));
    assert_success(&repo.axon(&["release", &group, "--reason", "handoff"]));
    let show = stdout(&repo.axon(&["show", &group]));
    assert!(show.contains("Released  (handoff)"));
}

#[test]
fn dependencies_and_after_conditions_accept_every_kind_combination() {
    let repo = TestRepo::new();
    repo.init("test");
    let prerequisite_group = repo.group_plan("foundation");
    let dependent_issue = repo.plan("delivery issue");
    repo.add_dependency(&dependent_issue, &prerequisite_group);
    assert!(!stdout(&repo.axon(&["ready"])).contains(&dependent_issue));
    assert_success(&repo.axon(&["start", &prerequisite_group]));
    assert_success(&repo.axon(&["done", &prerequisite_group]));
    assert!(stdout(&repo.axon(&["ready"])).contains(&dependent_issue));

    let trigger_issue = repo.plan("trigger issue");
    let waiting_group = repo.group_plan("waiting group");
    assert_success(&repo.axon(&["when", "after", &waiting_group, &trigger_issue]));
    assert!(!stdout(&repo.axon(&["ready"])).contains(&waiting_group));
    assert_success(&repo.axon(&["decide", "reject", &trigger_issue]));
    assert!(stdout(&repo.axon(&["ready"])).contains(&waiting_group));

    let rejected_group = repo.group_plan("rejected prerequisite");
    let dependent_group = repo.group_plan("orphaned plan");
    repo.add_dependency(&dependent_group, &rejected_group);
    assert_success(&repo.axon(&["decide", "reject", &rejected_group]));
    let triage = stdout(&repo.axon(&["triage"]));
    assert!(triage.contains(&format!("{dependent_group}  Group  Orphaned")));
}

#[test]
fn group_dependency_applies_to_its_descendant_frontier() {
    let repo = TestRepo::new();
    repo.init("test");
    let plan = repo.group_plan("delivery");
    let task = repo.plan("delivery task");
    let prerequisite = repo.plan("external result");
    repo.set_parent(&task, &plan);
    repo.add_dependency(&plan, &prerequisite);
    assert!(!stdout(&repo.axon(&["ready"])).contains(&plan));
    assert_success(&repo.axon(&["start", &prerequisite]));
    assert_success(&repo.axon(&["done", &prerequisite]));
    assert!(stdout(&repo.axon(&["ready"])).contains(&plan));
    assert_success(&repo.axon(&["start", &plan]));
    assert!(stdout(&repo.axon(&["ready"])).contains(&task));
}

#[test]
fn triage_shows_only_the_active_scope_frontier() {
    let repo = TestRepo::new();
    repo.init("test");
    let parent = repo.group_capture("undecided plan");
    let child = repo.capture("undecided detail");
    assert_success(&repo.axon(&["group", "set", &child, &parent]));
    let triage = stdout(&repo.axon(&["triage"]));
    assert!(triage.contains(&parent));
    assert!(!triage.contains(&child));
    let inactive = format!("inactive: {parent} (Progress=NotStarted, Disposition=Undecided)");
    assert!(stdout(&repo.axon(&["list"])).contains(&inactive));
    assert!(stdout(&repo.axon(&["show", &child])).contains(&format!(
        "Active scope: no ({parent} (Progress=NotStarted, Disposition=Undecided))"
    )));

    assert_success(&repo.axon(&["decide", "accept", &parent]));
    assert_success(&repo.axon(&["start", &parent]));
    let triage = stdout(&repo.axon(&["triage"]));
    assert!(!triage.contains(&parent));
    assert!(triage.contains(&child));
}

#[test]
fn ended_group_structure_and_descendant_terminal_state_are_fixed() {
    let repo = TestRepo::new();
    repo.init("test");
    let ended = repo.group_plan("completed plan");
    let outside = repo.group_plan("outside");
    assert_success(&repo.axon(&["start", &ended]));
    assert_success(&repo.axon(&["done", &ended]));

    let move_group = repo.axon(&["group", "set", &ended, &outside]);
    assert_failure(&move_group);
    assert!(stderr(&move_group).contains("Ended group"));
    let issue = repo.plan("late work");
    repo.undecide(&issue);
    let add_child = repo.axon(&["group", "set", &issue, &ended]);
    assert_failure(&add_child);
    assert!(stderr(&add_child).contains("below an Ended group"));
    let create_child = repo.axon(&["plan", "late creation", "--parent", &ended]);
    assert_failure(&create_child);
    assert!(stderr(&create_child).contains("below an Ended group"));
    let add_dep = repo.axon(&["dep", "add", &ended, "--needs", &outside]);
    assert_failure(&add_dep);
    assert!(stderr(&add_dep).contains("cannot change its dependencies"));
}

#[test]
fn containment_and_wait_relations_are_checked_as_one_atomic_graph() {
    let repo = TestRepo::new();
    repo.init("test");
    let group = repo.group_plan("plan");
    let child = repo.plan("child");
    repo.set_parent(&child, &group);
    let before = repo.dep_count();

    repo.undecide(&group);
    let cycle = repo.axon(&["dep", "add", &group, "--needs", &child]);
    assert_failure(&cycle);
    assert!(stderr(&cycle).contains("wait graph"));
    assert_eq!(repo.dep_count(), before);

    let after_cycle = repo.axon(&["when", "after", &child, &group]);
    assert_failure(&after_cycle);
    assert!(stderr(&after_cycle).contains("completion wait graph"));
    assert_eq!(repo.snapshot(&child).resurface_ref, None);
}

#[test]
fn settings_are_noops_but_transitions_reject_repetition() {
    let repo = TestRepo::new();
    repo.init("test");
    let group = repo.group_plan("plan");
    let issue = repo.plan("task");
    repo.set_parent(&issue, &group);
    let before = repo.snapshot(&issue);
    assert_success(&repo.axon(&["group", "set", &issue, &group]));
    assert_eq!(repo.snapshot(&issue), before);

    assert_success(&repo.axon(&["start", &group]));
    let before = repo.snapshot(&group);
    let repeated = repo.axon(&["start", &group]);
    assert_failure(&repeated);
    assert_eq!(repo.snapshot(&group), before);

    let prerequisite = repo.plan("prerequisite");
    repo.add_dependency(&issue, &prerequisite);
    assert_success(&repo.axon(&["dep", "add", &issue, "--needs", &prerequisite]));
    assert_eq!(repo.dep_count(), 1);
}

#[test]
fn fixed_declarations_reject_relation_changes_but_allow_noops() {
    let repo = TestRepo::new();
    repo.init("test");
    let first_group = repo.group_plan("first");
    let second_group = repo.group_plan("second");
    let issue = repo.plan("task");
    let first_prerequisite = repo.plan("first prerequisite");
    let second_prerequisite = repo.plan("second prerequisite");

    repo.set_parent(&issue, &first_group);
    let before = repo.snapshot(&issue);
    assert_success(&repo.axon(&["group", "set", &issue, &first_group]));
    let move_attempt = repo.axon(&["group", "set", &issue, &second_group]);
    assert_failure(&move_attempt);
    assert!(stderr(&move_attempt).contains("plan declaration"));
    assert_eq!(repo.snapshot(&issue), before);

    repo.add_dependency(&issue, &first_prerequisite);
    assert_success(&repo.axon(&["dep", "add", &issue, "--needs", &first_prerequisite]));
    let add_attempt = repo.axon(&["dep", "add", &issue, "--needs", &second_prerequisite]);
    assert_failure(&add_attempt);
    assert!(stderr(&add_attempt).contains("plan declaration"));
    let remove_attempt = repo.axon(&["dep", "rm", &issue, "--needs", &first_prerequisite]);
    assert_failure(&remove_attempt);
    assert_eq!(repo.dep_count(), 1);
}

#[test]
fn decision_and_progress_history_work_for_groups() {
    let repo = TestRepo::new();
    repo.init("test");
    let group = repo.group_capture("plan");
    assert_success(&repo.axon(&["decide", "accept", &group, "--reason", "approved"]));
    assert_success(&repo.axon(&["start", &group]));
    assert_success(&repo.axon(&["release", &group, "--reason", "handoff"]));
    let log = stdout(&repo.axon(&["log", &group]));
    assert!(log.contains("Disposition: Undecided -> Accepted  (approved)"));
    let show = stdout(&repo.axon(&["show", &group]));
    assert!(show.contains("Started"));
    assert!(show.contains("Released  (handoff)"));
}

#[test]
fn rejecting_in_progress_entity_reports_only_the_saved_disposition() {
    let repo = TestRepo::new();
    repo.init("test");
    let issue = repo.plan("active task");
    assert_success(&repo.axon(&["start", &issue]));

    let rejected = repo.axon(&["decide", "reject", &issue, "--reason", "stopped"]);
    assert_success(&rejected);
    assert_eq!(
        stdout(&rejected),
        format!("{issue} Disposition set to Rejected\n")
    );
}

#[test]
fn old_slug_commands_are_not_compatibility_aliases() {
    for args in [
        &["group", "new", "legacy"][..],
        &["group", "list"][..],
        &["group", "show", "legacy"][..],
        &["group", "reject", "legacy"][..],
        &["group", "dep", "add", "legacy", "--needs", "other"][..],
    ] {
        let repo = TestRepo::new();
        let output = repo.axon(args);
        assert_failure(&output);
        assert!(stderr(&output).contains("unrecognized subcommand"));
    }
}

#[test]
fn empty_queries_keep_guidance_out_of_stdout() {
    let repo = TestRepo::new();
    repo.init("test");
    for command in ["ready", "triage", "claims", "stale", "list"] {
        let output = repo.axon(&[command]);
        assert_success(&output);
        assert!(output.stdout.is_empty(), "{command}");
        assert!(!output.stderr.is_empty(), "{command}");
    }
}

#[test]
fn linked_worktrees_share_entity_state_and_claims() {
    let repo = TestRepo::new();
    repo.init("test");
    let group = repo.group_plan("shared plan");
    let worktree = repo.add_worktree();
    assert_success(&repo.axon_in(&worktree, &["start", &group]));
    let claims = repo.axon(&["claims"]);
    assert_success(&claims);
    let claims = stdout(&claims);
    assert!(claims.contains(&group));
    assert!(claims.contains(&format!("Worktree: {}", worktree.display())));
}

#[cfg(unix)]
#[test]
fn human_timestamps_use_local_time_with_a_numeric_offset() {
    fn run(repo: &TestRepo, args: &[&str]) -> String {
        stdout(&repo.axon_in_timezone("JST-9", args))
    }

    let repo = TestRepo::new();
    repo.init("test");
    let issue = repo.plan("timestamped task");
    repo.undecide(&issue);
    repo.accept(&issue);
    assert_success(&repo.axon(&["start", &issue]));
    assert_success(&repo.axon(&["note", "add", &issue, "-m", "timestamped note"]));
    repo.execute_batch(&format!(
        "UPDATE entities SET claimed_at = '2026-09-01T17:55:00Z' WHERE id = '{issue}';
         UPDATE entity_events SET at = '2026-09-01T17:55:00Z' WHERE entity_id = '{issue}';
         UPDATE entity_progress_events SET at = '2026-09-01T17:55:00Z' WHERE entity_id = '{issue}';
         UPDATE entity_notes SET at = '2026-09-01T17:55:00Z' WHERE entity_id = '{issue}';
         UPDATE declaration_revisions SET created_at = '2026-09-01T17:55:00Z'
           WHERE entity_id = '{issue}';"
    ));

    let expected = "2026-09-02 02:55 +09:00";

    assert!(run(&repo, &["claims"]).contains(&format!("Started: {expected}")));

    let show = run(&repo, &["show", &issue]);
    assert!(show.contains(&format!("Started: {expected}")));
    assert!(show.contains(&format!("Note 1  {expected}  test-actor")));
    assert!(show.contains(&format!("  {expected}  test-actor  Started")));

    let log = run(&repo, &["log", &issue]);
    assert!(log.starts_with(expected), "{log}");
    assert!(run(&repo, &["note", "list", &issue]).contains(&format!("1  {expected}")));
    assert!(
        run(&repo, &["note", "show", &issue, "1"])
            .contains(&format!("Recorded: {expected}  test-actor"))
    );
    assert!(run(&repo, &["revision", "list", &issue]).contains(&format!("1  {expected}")));
    assert!(
        run(&repo, &["revision", "show", &issue, "1"]).contains(&format!("Created: {expected}"))
    );
}

#[test]
fn git_and_non_git_management_roots_keep_their_boundaries() {
    let dir = TestDir::new("roots");
    let outer = dir.path().join("outer");
    let repository = outer.join("repo");
    fs::create_dir_all(&repository).unwrap();
    assert_success(&dir.axon_in(&outer, &["init", "outer"]));
    dir.init_git(&repository);

    let list = dir.axon_in(&repository, &["list"]);
    assert_failure(&list);
    assert!(stderr(&list).contains("not initialized"));
    assert_success(&dir.axon_in(&repository, &["init", "inner"]));

    let nested = repository.join("nested");
    fs::create_dir(&nested).unwrap();
    let created = dir.axon_in(&nested, &["group", "plan", "nested plan"]);
    assert_success(&created);
    assert!(repository.join(".axon/axon.db").is_file());
    assert!(!nested.join(".axon").exists());
}

#[test]
fn notes_preserve_full_bodies_and_work_for_every_entity_state() {
    let repo = TestRepo::new();
    repo.init("test");
    let accepted = created_id(&repo.axon(&[
        "plan",
        "documented task",
        "-m",
        "Declaration description\nwith two lines",
    ]));
    let undecided = repo.group_capture("draft group");
    let rejected = repo.plan("declined task");
    assert_success(&repo.axon(&["decide", "reject", &rejected, "-r", "not now"]));
    let ended = repo.group_plan("finished group");
    assert_success(&repo.axon(&["start", &ended]));
    assert_success(&repo.axon(&["done", &ended]));

    for id in [&undecided, &rejected, &ended] {
        assert_success(&repo.axon(&["note", "add", id, "-m", "state-independent note"]));
    }

    let long_markdown = format!(
        "# Investigation\n\n- preserves Markdown\n- preserves spacing  \n\n```text\n{}\n```\n",
        "long evidence ".repeat(400)
    );
    let note_file = repo.root().join("note.md");
    fs::write(&note_file, &long_markdown).unwrap();
    assert_success(&repo.axon(&["note", "add", &accepted, "-F", note_file.to_str().unwrap()]));
    assert_success(&repo.axon(&[
        "note",
        "add",
        &accepted,
        "-m",
        "second note\nkeeps its newline",
    ]));

    let suffix = accepted.rsplit('-').next().unwrap();
    let list = repo.axon(&["note", "list", suffix]);
    assert_success(&list);
    let list = stdout(&list);
    assert!(list.contains("1  "));
    assert!(list.contains("2  "));

    let first = repo.axon(&["note", "show", suffix, "1"]);
    assert_success(&first);
    assert!(stdout(&first).ends_with(&long_markdown));

    let show = repo.axon(&["show", suffix]);
    assert_success(&show);
    let show = stdout(&show);
    assert!(show.contains("Plan declaration: fixed at Revision 1"));
    assert!(show.contains("Records: Notes: 2  Revisions: 1"));
    assert!(show.contains("Declaration description\nwith two lines"));
    assert!(show.contains(&long_markdown));
    assert!(
        show.find("Note 1").unwrap() < show.find("Note 2").unwrap(),
        "notes must stay in save order"
    );

    for id in [&undecided, &rejected, &ended] {
        let show = repo.axon(&["show", id]);
        assert_success(&show);
        assert!(stdout(&show).contains("state-independent note"));
    }
}

#[test]
fn notes_reject_missing_or_blank_input_and_unknown_local_numbers() {
    let repo = TestRepo::new();
    repo.init("test");
    let issue = repo.plan("task");
    let blank_file = repo.root().join("blank-note.md");
    fs::write(&blank_file, " \n\t ").unwrap();

    for args in [
        vec!["note", "add", &issue],
        vec!["note", "add", &issue, "-m", " \n\t"],
        vec!["note", "add", &issue, "-m", "\u{00a0}\u{3000}"],
        vec!["note", "add", &issue, "-F", blank_file.to_str().unwrap()],
    ] {
        assert_failure(&repo.axon(&args));
    }

    let missing = repo.axon(&["note", "show", &issue, "99"]);
    assert_failure(&missing);
    assert!(stderr(&missing).contains("Note 99 does not exist"));
    let missing = repo.axon(&["revision", "show", &issue, "99"]);
    assert_failure(&missing);
    assert!(stderr(&missing).contains("Declaration Revision 99 does not exist"));
}

#[test]
fn revision_commands_expose_fixed_declarations_and_structural_diffs() {
    let repo = TestRepo::new();
    repo.init("test");
    let parent = repo.group_plan("delivery");
    let prerequisite = repo.plan("foundation");
    let issue = repo.capture("first title");

    let draft = stdout(&repo.axon(&["show", &issue]));
    assert!(draft.contains("Plan declaration: draft"));
    assert!(draft.contains("Revisions: 0"));
    assert_success(&repo.axon(&["decide", "accept", &issue, "-r", "initial plan"]));

    repo.undecide(&issue);
    assert_success(&repo.axon(&[
        "write",
        &issue,
        "--title",
        "second title",
        "-m",
        "new description\nwith detail",
    ]));
    assert_success(&repo.axon(&["group", "set", &issue, &parent]));
    assert_success(&repo.axon(&["dep", "add", &issue, "--needs", &prerequisite]));
    repo.accept(&issue);

    let suffix = issue.rsplit('-').next().unwrap();
    let list = repo.axon(&["revision", "list", suffix]);
    assert_success(&list);
    let list = stdout(&list);
    assert!(list.contains("1  "));
    assert!(list.contains("2  "));
    assert!(list.contains("[current]  second title"));

    let revision = repo.axon(&["revision", "show", suffix, "2"]);
    assert_success(&revision);
    let revision = stdout(&revision);
    assert!(revision.contains("Declaration Revision 2 [current]"));
    assert!(revision.contains("Title: second title"));
    assert!(revision.contains("new description\nwith detail"));
    assert!(revision.contains(&format!("Parent: {parent}")));
    assert!(revision.contains(&format!("  {prerequisite}")));

    let diff = repo.axon(&["revision", "diff", suffix, "1", "2"]);
    assert_success(&diff);
    let diff = stdout(&diff);
    assert!(diff.contains("Title:\n- first title\n+ second title"));
    assert!(diff.contains("Description:\n- absent\n+ present\n+ new description\n+ with detail"));
    assert!(diff.contains(&format!("Parent:\n- (none)\n+ {parent}")));
    assert!(diff.contains(&format!("Outgoing dependencies:\n+ {prerequisite}")));

    let show = stdout(&repo.axon(&["show", &issue]));
    assert!(show.contains("Plan declaration: fixed at Revision 2"));
    assert!(show.contains("Revisions: 2"));
    assert!(show.contains("Decision history: 3"));
    let log = stdout(&repo.axon(&["log", &issue]));
    assert!(log.contains("[Revision 1]"));
    assert!(log.contains("[Revision 2]"));

    repo.undecide(&issue);
    assert_success(&repo.axon(&["write", &issue, "-m", "(none)"]));
    repo.accept(&issue);
    let literal = stdout(&repo.axon(&["revision", "show", &issue, "3"]));
    assert!(literal.contains("Description:\npresent\n(none)"));
    let absence_to_literal = stdout(&repo.axon(&["revision", "diff", &issue, "1", "3"]));
    assert!(absence_to_literal.contains("Description:\n- absent\n+ present\n+ (none)"));
}
