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

fn assert_show_field(output: &str, label: &str, expected: &str) {
    let field = format!("{label} {expected}");
    assert!(
        output.lines().any(|line| line.contains(&field)),
        "missing show field {field:?} in:\n{output}"
    );
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
fn mutation_confirmations_begin_with_the_affected_entity() {
    let repo = TestRepo::new();
    repo.init("test");

    let created = repo.axon(&["capture", "draft task"]);
    let draft = created_id(&created);
    assert_eq!(
        stdout(&created),
        format!("{draft}  Created  Issue  [Undecided]  draft task\n")
    );

    let planned = repo.axon(&["plan", "active task"]);
    let active = created_id(&planned);
    assert_eq!(
        stdout(&planned),
        format!("{active}  Created  Issue  [Accepted]  active task\n")
    );
    assert_eq!(
        stdout(&repo.axon(&["start", &active])),
        format!("{active}  Started  Claim: test-actor\n")
    );
    assert_eq!(
        stdout(&repo.axon(&["done", &active])),
        format!("{active}  Ended\n")
    );

    let releasable = repo.plan("releasable task");
    assert_success(&repo.axon(&["start", &releasable]));
    assert_eq!(
        stdout(&repo.axon(&["release", &releasable])),
        format!("{releasable}  Released\n")
    );

    assert_eq!(
        stdout(&repo.axon(&["decide", "accept", &draft])),
        format!("{draft}  Disposition: Accepted\n")
    );
    repo.undecide(&draft);
    assert_eq!(
        stdout(
            &repo.axon_in_timezone("UTC", &["when", "at", &draft, "2099-01-02T03:04:05.1200Z"],)
        ),
        format!("{draft}  Resurface condition: AtDate(2099-01-02T03:04:05.1200+00:00)\n")
    );
    let event_count = repo.snapshot(&draft).decision_events;
    assert_failure(&repo.axon(&["when", "at", &draft, "2099-01-02T12:04:05.1200+09:00"]));
    assert_eq!(repo.snapshot(&draft).decision_events, event_count);
    assert_eq!(
        repo.snapshot(&draft).resurface_date.as_deref(),
        Some("2099-01-02T03:04:05.1200Z")
    );
    let log = stdout(&repo.axon_in_timezone("Asia/Tokyo", &["log", &draft]));
    assert!(log.contains("AtDate(2099-01-02T12:04:05.1200+09:00)"));

    let prerequisite = repo.plan("prerequisite");
    assert_eq!(
        stdout(&repo.axon(&["dep", "add", &draft, "--needs", &prerequisite])),
        format!("{draft}  Dependency added: {prerequisite}\n")
    );
    assert_eq!(
        stdout(&repo.axon(&["dep", "rm", &draft, "--needs", &prerequisite])),
        format!("{draft}  Dependency removed: {prerequisite}\n")
    );

    let parent = repo.group_plan("parent");
    assert_eq!(
        stdout(&repo.axon(&["group", "set", &draft, &parent])),
        format!("{draft}  Parent: {parent}\n")
    );
    assert_eq!(
        stdout(&repo.axon(&["group", "unset", &draft])),
        format!("{draft}  Parent: (none)\n")
    );
    assert_eq!(
        stdout(&repo.axon(&["note", "add", &draft, "-m", "context"])),
        format!("{draft}  Note {} recorded\n", repo.note_id(&draft, 1))
    );
    assert_eq!(
        stdout(&repo.axon(&["write", &draft, "--title", "draft task"])),
        format!("{draft}  No changes\n")
    );
}

#[test]
fn setting_confirmations_follow_saved_changes_and_preserve_noop_storage() {
    for backend in ["sqlite", "file"] {
        let repo = TestRepo::new();
        assert_success(&repo.axon(&["init", "--backend", backend, "test"]));
        let saved_path = repo.root().join(if backend == "sqlite" {
            ".axon/axon.db"
        } else {
            ".axon/state.jsonl"
        });
        let parent = repo.group_capture("parent");
        let prerequisite = repo.capture("prerequisite");
        for id in [repo.capture("draft"), repo.group_capture("draft")] {
            let cases = [
                (
                    vec!["write", &id, "--title", "renamed"],
                    "Title updated".to_string(),
                    "No changes".to_string(),
                ),
                (
                    vec!["write", &id, "-m", "body"],
                    "Description updated".to_string(),
                    "No changes".to_string(),
                ),
                (
                    vec!["write", &id, "--title", "both", "-m", "both body"],
                    "Title updated  Description updated".to_string(),
                    "No changes".to_string(),
                ),
                (
                    vec!["write", &id, "--title", "both", "-m", "partial"],
                    "Description updated".to_string(),
                    "No changes".to_string(),
                ),
                (
                    vec!["write", &id, "--title", "final", "-m", "partial"],
                    "Title updated".to_string(),
                    "No changes".to_string(),
                ),
                (
                    vec!["write", &id, "-m", ""],
                    "Description removed".to_string(),
                    "No changes".to_string(),
                ),
                (
                    vec!["dep", "add", &id, "--needs", &prerequisite],
                    format!("Dependency added: {prerequisite}"),
                    format!("No changes  Dependency already present: {prerequisite}"),
                ),
                (
                    vec!["dep", "rm", &id, "--needs", &prerequisite],
                    format!("Dependency removed: {prerequisite}"),
                    format!("No changes  Dependency already absent: {prerequisite}"),
                ),
                (
                    vec!["group", "set", &id, &parent],
                    format!("Parent: {parent}"),
                    format!("No changes  Parent: {parent}"),
                ),
                (
                    vec!["group", "unset", &id],
                    "Parent: (none)".to_string(),
                    "No changes  Parent: (none)".to_string(),
                ),
            ];
            for (args, changed, unchanged) in cases {
                let output = repo.axon(&args);
                assert_success(&output);
                assert!(output.stderr.is_empty());
                assert_eq!(stdout(&output), format!("{id}  {changed}\n"));
                let before = fs::read(&saved_path).unwrap();
                let output = repo.axon(&args);
                assert_success(&output);
                assert!(output.stderr.is_empty());
                assert_eq!(stdout(&output), format!("{id}  {unchanged}\n"));
                assert_eq!(fs::read(&saved_path).unwrap(), before);
            }
            assert_success(&repo.axon(&["dep", "add", &id, "--needs", &prerequisite]));
            assert_success(&repo.axon(&["group", "set", &id, &parent]));
            for decision in ["accept", "reject"] {
                assert_success(&repo.axon(&["decide", decision, &id]));
                let before = fs::read(&saved_path).unwrap();
                for (args, expected) in [
                    (
                        vec!["write", &id, "--title", "final", "-m", "  "],
                        "No changes".to_string(),
                    ),
                    (
                        vec!["dep", "add", &id, "--needs", &prerequisite],
                        format!("No changes  Dependency already present: {prerequisite}"),
                    ),
                    (
                        vec!["dep", "rm", &id, "--needs", &parent],
                        format!("No changes  Dependency already absent: {parent}"),
                    ),
                    (
                        vec!["group", "set", &id, &parent],
                        format!("No changes  Parent: {parent}"),
                    ),
                ] {
                    let output = repo.axon(&args);
                    assert_success(&output);
                    assert_eq!(stdout(&output), format!("{id}  {expected}\n"));
                }
                for args in [
                    vec!["write", &id, "--title", "forbidden"],
                    vec!["write", &id, "-m", "forbidden"],
                    vec!["dep", "rm", &id, "--needs", &prerequisite],
                    vec!["dep", "add", &id, "--needs", &parent],
                    vec!["group", "unset", &id],
                ] {
                    let output = repo.axon(&args);
                    assert_failure(&output);
                    assert!(stderr(&output).contains("plan declaration"));
                }
                assert_eq!(fs::read(&saved_path).unwrap(), before);
                repo.undecide(&id);
            }
        }
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
    assert!(issue_only.contains(&format!(
        "{issue}  Issue  [NotStarted/Accepted]  ship the feature"
    )));
    assert!(issue_only.contains(&format!(
        "{captured}  Issue  [NotStarted/Undecided]  investigate the risk"
    )));
    assert!(!issue_only.contains(&group));

    let group_only = repo.axon(&["triage", "--kind", "group"]);
    assert_success(&group_only);
    assert_eq!(
        stdout(&group_only),
        format!("{captured_group}  Group  possible follow-up  Reason: Undecided\n")
    );

    for id in [&issue, &group] {
        let show = repo.axon(&["show", id]);
        assert_success(&show);
        let show = stdout(&show);
        assert!(show.starts_with(&format!("{id}  ")));
        assert!(!show.contains('\u{1b}'));
    }
}

#[test]
fn human_output_keeps_non_terminal_output_plain() {
    let repo = TestRepo::new();
    repo.init("test");
    let issue = repo.plan("styled output");

    let plain = repo.axon(&["show", &issue]);
    assert_success(&plain);
    let plain = stdout(&plain);
    assert!(!plain.contains('\u{1b}'));

    let forced = repo.axon_with_env(
        &["show", &issue],
        &[("NO_COLOR", None), ("CLICOLOR_FORCE", Some("1"))],
    );
    assert_success(&forced);
    assert_eq!(stdout(&forced), plain);

    let no_color = repo.axon_with_env(
        &["show", &issue],
        &[("NO_COLOR", Some("1")), ("CLICOLOR_FORCE", Some("1"))],
    );
    assert_success(&no_color);
    assert_eq!(stdout(&no_color), plain);

    for args in [
        &["ready"][..],
        &["list"][..],
        &["revision", "list", &issue][..],
    ] {
        let output = repo.axon(args);
        assert_success(&output);
        assert!(!stdout(&output).contains('\u{1b}'), "{args:?}");
    }
}

#[test]
fn human_output_preserves_escape_sequences_stored_in_content() {
    let repo = TestRepo::new();
    repo.init("test");
    let title = "title \u{1b}[35mmagenta\u{1b}[0m";
    let description = "description \u{1b}[31mred\u{1b}[0m";
    let note = "note \u{1b}[32mgreen\u{1b}[0m";
    let reason = "reason \u{1b}[34mblue\u{1b}[0m";
    let created = repo.axon(&["plan", title, "-m", description]);
    let issue = created_id(&created);
    assert!(stdout(&created).contains(title));
    assert_success(&repo.axon(&["note", "add", &issue, "-m", note]));

    let show = stdout(&repo.axon(&["show", &issue]));
    assert!(show.contains(title));
    assert!(show.contains(description));
    assert!(show.contains(note));
    assert!(stdout(&repo.axon(&["list"])).contains(title));
    assert!(stdout(&repo.axon(&["note", "show", &issue, &repo.note_id(&issue, 1)])).contains(note));
    assert!(
        stdout(&repo.axon(&["revision", "show", &issue, &repo.revision_id(&issue, 1)]))
            .contains(description)
    );

    assert_success(&repo.axon(&["decide", "undecide", &issue, "-r", reason]));
    assert!(stdout(&repo.axon(&["log", &issue])).contains(reason));
}

#[cfg(unix)]
#[test]
fn human_output_treats_a_closed_pipe_as_success() {
    let repo = TestRepo::new();
    repo.init("test");
    let issue = repo.plan("closed pipe");

    for args in [
        &["list"][..],
        &["show", &issue][..],
        &["write", &issue, "--title", "closed pipe"][..],
    ] {
        let output = repo.axon_with_closed_stdout(args);
        assert_success(&output);
        assert!(output.stderr.is_empty(), "{args:?}");
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
    assert_show_field(&show, "Progress:", "Ended");
    assert!(show.contains("  Direct children: 1  Issue: 1  Group: 0  Terminal: 1"));
    assert!(show.contains("  Descendants: 1  Issue: 1  Group: 0  Terminal: 1"));
    assert_eq!(
        show.matches("Progress: NotStarted: 0  InProgress: 0  Ended: 1")
            .count(),
        2
    );
    assert_eq!(
        show.matches("Disposition: Undecided: 0  Accepted: 1  Rejected: 0")
            .count(),
        2
    );
    assert!(show.contains(&format!(
        "Subtree\n  {issue}  Issue  [Ended/Accepted]  implementation"
    )));
    assert!(!show.contains("Non-terminal descendants"));
}

#[test]
fn group_show_renders_the_complete_subtree_and_direct_dependencies() {
    let repo = TestRepo::new();
    repo.init("test");

    let external_satisfied = repo.plan("external foundation");
    assert_success(&repo.axon(&["start", &external_satisfied]));
    assert_success(&repo.axon(&["done", &external_satisfied]));
    let external_unresolved = repo.plan("external review");
    let external_rejected = repo.plan("rejected external result");
    assert_success(&repo.axon(&[
        "decide",
        "reject",
        &external_rejected,
        "-r",
        "not available",
    ]));

    let group = repo.group_plan("delivery");
    let ended = created_id(&repo.axon(&["plan", "schema", "--parent", &group]));
    let ready = created_id(&repo.axon(&["plan", "api", "--parent", &group]));
    let active = created_id(&repo.axon(&["plan", "worker", "--parent", &group]));
    let blocked = created_id(&repo.axon(&["plan", "release", "--parent", &group]));
    let orphaned = created_id(&repo.axon(&["plan", "legacy", "--parent", &group]));
    let deferred = created_id(&repo.axon(&["plan", "later", "--parent", &group]));
    let rejected = created_id(&repo.axon(&["plan", "discarded", "--parent", &group]));
    let nested = created_id(&repo.axon(&["group", "plan", "frontend", "--parent", &group]));
    let nested_draft = created_id(&repo.axon(&["capture", "browser tests", "--parent", &nested]));

    repo.add_dependency(&group, &external_satisfied);
    repo.add_dependency(&ready, &ended);
    repo.add_dependency(&blocked, &external_unresolved);
    repo.add_dependency(&orphaned, &external_rejected);
    assert_success(&repo.axon(&["dep", "add", &nested_draft, "--needs", &ready]));
    assert_success(&repo.axon(&["when", "at", &deferred, "2099-01-02T00:00:00Z"]));
    assert_success(&repo.axon(&["decide", "reject", &rejected, "-r", "not needed"]));

    assert_success(&repo.axon(&["start", &group]));
    assert_success(&repo.axon(&["start", &ended]));
    assert_success(&repo.axon(&["done", &ended]));
    assert_success(&repo.axon(&["start", &active]));

    let show = stdout(&repo.axon(&["show", &group]));
    let subtree = show
        .split_once("\n\nSubtree\n")
        .unwrap()
        .1
        .split_once("\n\nDetails\n")
        .unwrap()
        .0;
    for (id, title) in [
        (&ended, "schema"),
        (&ready, "api"),
        (&active, "worker"),
        (&blocked, "release"),
        (&orphaned, "legacy"),
        (&deferred, "later"),
        (&rejected, "discarded"),
        (&nested, "frontend"),
        (&nested_draft, "browser tests"),
    ] {
        assert!(
            subtree.contains(&format!("{id}  ")),
            "missing {id}:\n{show}"
        );
        assert!(subtree.contains(title), "missing {title:?}:\n{show}");
    }
    let nested_line = subtree.lines().find(|line| line.contains(&nested)).unwrap();
    let nested_draft_line = subtree
        .lines()
        .find(|line| line.contains(&nested_draft))
        .unwrap();
    assert!(nested_line.starts_with("  ") && !nested_line.starts_with("    "));
    assert!(nested_draft_line.starts_with("    "));
    assert!(subtree.contains(&format!("{ended}  Issue  [Ended/Accepted]  schema")));
    assert!(subtree.contains(&format!(
        "{rejected}  Issue  [NotStarted/Rejected]  discarded"
    )));
    assert!(subtree.contains(&format!(
        "{ready}  Issue  [NotStarted/Accepted]  api  Ready"
    )));
    assert!(subtree.contains(&format!("{active}  Issue  [InProgress/Accepted]  worker")));
    let subtree_line = |id: &str| {
        subtree
            .lines()
            .find(|line| line.contains(id))
            .unwrap_or_else(|| panic!("missing subtree line for {id}:\n{show}"))
    };
    assert!(subtree.contains(&format!(
        "{}\n    Unresolved dependency (Blocked): {external_unresolved}",
        subtree_line(&blocked)
    )));
    assert!(subtree.contains(&format!(
        "{}\n    Rejected prerequisite (Orphaned): {external_rejected}",
        subtree_line(&orphaned)
    )));
    assert!(subtree.contains(&format!("{}\n    Not surfaced", subtree_line(&deferred))));
    assert!(subtree.contains(&format!("Descendant gate closed: {nested}")));
    assert!(!subtree_line(&nested_draft).contains("Inactive"));
    assert!(!show.contains("Non-terminal descendants"));

    let mut direct = [
        &ended, &ready, &active, &blocked, &orphaned, &deferred, &rejected, &nested,
    ];
    direct.sort();
    for pair in direct.windows(2) {
        assert!(
            subtree
                .lines()
                .position(|line| line.starts_with(&format!("  {}  ", pair[0])))
                .unwrap()
                < subtree
                    .lines()
                    .position(|line| line.starts_with(&format!("  {}  ", pair[1])))
                    .unwrap(),
            "siblings must be ID ordered:\n{subtree}"
        );
    }

    let dependencies = show.split_once("\n\nDependencies\n").unwrap().1;
    for owner in [&group, &ready, &blocked, &orphaned, &nested_draft] {
        assert!(dependencies.contains(&format!("  {owner}\n")), "{show}");
    }
    assert!(dependencies.contains(&format!("Needs: {ended}  Satisfied")));
    assert!(dependencies.contains(&format!(
        "Needs: {external_unresolved}  Unresolved  External: external review"
    )));
    assert!(dependencies.contains(&format!(
        "Needs: {external_rejected}  Rejected  External: rejected external result"
    )));
    assert_eq!(show.matches(&external_satisfied).count(), 1);

    let issue_show = stdout(&repo.axon(&["show", &ready]));
    assert!(!issue_show.contains("\n\nSubtree\n"));
    assert!(!issue_show.contains("\n\nDependencies\n"));
    assert!(issue_show.contains(&format!("Satisfied dependency:  {ended}")));

    let waiting_group = repo.group_plan("waiting delivery");
    repo.add_dependency(&waiting_group, &external_unresolved);
    let waiting_show = stdout(&repo.axon(&["show", &waiting_group]));
    assert_eq!(waiting_show.matches(&external_unresolved).count(), 2);
    assert!(!waiting_show.contains("Root cause:"));
}

#[test]
fn empty_group_show_names_its_empty_subtree() {
    let repo = TestRepo::new();
    repo.init("test");
    let group = repo.group_plan("empty");

    let show = stdout(&repo.axon(&["show", &group]));
    assert!(show.contains("\n\nSubtree\n  No descendants\n"));
    assert!(!show.contains("\n\nDependencies\n"));
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
    assert!(triage.contains(&format!(
        "{dependent_group}  Group  orphaned plan  Reason: Orphaned  Rejected dependencies: {rejected_group}"
    )));
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
    let inactive = format!("Inactive: {parent} (Progress=NotStarted, Disposition=Undecided)");
    assert!(stdout(&repo.axon(&["list"])).contains(&inactive));
    assert_show_field(
        &stdout(&repo.axon(&["show", &child])),
        "Active scope:",
        &format!("no ({parent} (Progress=NotStarted, Disposition=Undecided))"),
    );

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
    assert!(log.contains("Disposition: Undecided -> Accepted  Reason: approved"));
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
        format!("{issue}  Disposition: Rejected\n")
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
    for command in ["ready", "triage", "claims", "list"] {
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
    assert_failure(&repo.axon_in(&worktree, &["init"]));
    assert_success(&repo.axon_in(&worktree, &["start", &group]));
    let claims = repo.axon(&["claims"]);
    assert_success(&claims);
    let claims = stdout(&claims);
    assert!(claims.contains(&group));
    assert!(claims.contains(&format!("{group}  Group  shared plan  Claim: test-actor")));
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
         UPDATE causal_links SET payload = json_set(payload, '$.result.progress.InProgress.at', '2026-09-01T17:55:00Z')
           WHERE json_extract(payload,'$.owner') = '{issue}' AND json_type(payload,'$.result.progress.InProgress') IS NOT NULL;
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
    assert!(show.contains(&format!(
        "Note {}  {expected}  test-actor",
        repo.note_id(&issue, 1)
    )));
    assert!(show.contains(&format!("  {expected}  test-actor  Started")));

    let log = run(&repo, &["log", &issue]);
    assert!(log.starts_with(expected), "{log}");
    assert!(
        run(&repo, &["note", "list", &issue])
            .contains(&format!("{}  {expected}", repo.note_id(&issue, 1)))
    );
    assert!(
        run(&repo, &["note", "show", &issue, &repo.note_id(&issue, 1)])
            .contains(&format!("Recorded: {expected}  Actor: test-actor"))
    );
    assert!(
        run(&repo, &["revision", "list", &issue])
            .contains(&format!("{}  {expected}", repo.revision_id(&issue, 1)))
    );
    assert!(
        run(
            &repo,
            &["revision", "show", &issue, &repo.revision_id(&issue, 1)]
        )
        .contains(&format!("Created: {expected}"))
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
    assert!(list.contains(&repo.note_id(suffix, 1)));
    assert!(list.contains(&repo.note_id(suffix, 2)));

    let first = repo.axon(&["note", "show", suffix, &repo.note_id(suffix, 1)]);
    assert_success(&first);
    assert!(stdout(&first).ends_with(&long_markdown));

    let show = repo.axon(&["show", suffix]);
    assert_success(&show);
    let show = stdout(&show);
    assert_show_field(
        &show,
        "Plan declaration:",
        &format!("fixed at Revision {}", repo.revision_id(&accepted, 1)),
    );
    assert_show_field(
        &show,
        "Records:",
        "Notes: 2  Revisions: 1  Decision history: 0  Progress history: 0",
    );
    assert!(show.contains("Declaration description\nwith two lines"));
    assert!(show.contains(&long_markdown));
    assert!(
        show.find(&repo.note_id(&accepted, 1)).unwrap()
            < show.find(&repo.note_id(&accepted, 2)).unwrap(),
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
    assert_show_field(&draft, "Plan declaration:", "draft");
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
    assert!(list.contains(&repo.revision_id(suffix, 1)));
    assert!(list.contains(&repo.revision_id(suffix, 2)));
    assert!(list.contains("[current]  second title"));

    let revision = repo.axon(&["revision", "show", suffix, &repo.revision_id(suffix, 2)]);
    assert_success(&revision);
    let revision = stdout(&revision);
    assert!(revision.contains(&format!(
        "Declaration Revision {}  [current]",
        repo.revision_id(&issue, 2)
    )));
    assert!(revision.contains("Title: second title"));
    assert!(revision.contains("new description\nwith detail"));
    assert!(revision.contains(&format!("Parent: {parent}")));
    assert!(revision.contains(&format!("  {prerequisite}")));

    let diff = repo.axon(&[
        "revision",
        "diff",
        suffix,
        &repo.revision_id(suffix, 1),
        &repo.revision_id(suffix, 2),
    ]);
    assert_success(&diff);
    let diff = stdout(&diff);
    assert!(diff.contains("Title\n- first title\n+ second title"));
    assert!(diff.contains("Description\n- absent\n+ present\n+ new description\n+ with detail"));
    assert!(diff.contains(&format!("Parent\n- (none)\n+ {parent}")));
    assert!(diff.contains(&format!("Outgoing dependencies\n+ {prerequisite}")));

    let show = stdout(&repo.axon(&["show", &issue]));
    assert_show_field(
        &show,
        "Plan declaration:",
        &format!("fixed at Revision {}", repo.revision_id(&issue, 2)),
    );
    assert!(show.contains("Revisions: 2"));
    assert!(show.contains("Decision history: 3"));
    let log = stdout(&repo.axon(&["log", &issue]));
    assert!(log.contains(&format!("Revision: {}", repo.revision_id(&issue, 1))));
    assert!(log.contains(&format!("Revision: {}", repo.revision_id(&issue, 2))));

    repo.undecide(&issue);
    assert_success(&repo.axon(&["write", &issue, "-m", "(none)"]));
    repo.accept(&issue);
    let literal = stdout(&repo.axon(&["revision", "show", &issue, &repo.revision_id(&issue, 3)]));
    assert!(literal.contains("Description\npresent\n(none)"));
    let absence_to_literal = stdout(&repo.axon(&[
        "revision",
        "diff",
        &issue,
        &repo.revision_id(&issue, 1),
        &repo.revision_id(&issue, 3),
    ]));
    assert!(absence_to_literal.contains("Description\n- absent\n+ present\n+ (none)"));
}

#[test]
fn schema_mismatch_explains_recovery_without_changing_the_database() {
    let repo = TestRepo::new();
    repo.init("test");
    let id = repo.plan("preserved task");
    let snapshot = repo.snapshot(&id);
    for version in [0, 7, 999] {
        repo.execute_batch(&format!("PRAGMA user_version = {version}"));
        let path = repo.root().join(".axon/axon.db");
        let before = fs::read(&path).unwrap();
        let output = repo.axon(&["list"]);
        assert_failure(&output);
        assert!(output.stdout.is_empty());
        let diagnostic = stderr(&output);
        assert!(diagnostic.contains(&format!("unsupported axon schema version {version}")));
        assert!(diagnostic.contains(&format!("supports DB schema {version}")));
        assert!(diagnostic.contains("database schema update was not applied"));
        assert!(diagnostic.contains("requested operation did not run"));
        assert!(diagnostic.contains("init cannot upgrade"));
        assert!(diagnostic.contains("axon docs"));
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(repo.snapshot(&id), snapshot);
        let init = repo.axon(&["init", "test"]);
        assert_failure(&init);
        assert!(stderr(&init).contains("already initialized"));
        assert!(!stderr(&init).contains("restore a matching config/state pair"));
        assert_eq!(fs::read(&path).unwrap(), before);
        for args in [&["docs"][..], &["--help"][..], &["--version"][..]] {
            assert_success(&repo.axon(args));
        }
        let docs = stdout(&repo.axon(&["docs"]));
        assert!(docs.contains("Storage recovery"));
        assert!(docs.contains("export also requires a compatible build"));
    }
}

#[test]
fn failed_operations_offer_inspection_without_changing_state() {
    let repo = TestRepo::new();
    repo.init("test");
    let id = repo.capture("unadopted task");
    let before = repo.snapshot(&id);
    let output = repo.axon(&["start", &id]);
    assert_failure(&output);
    assert!(output.stdout.is_empty());
    assert!(stderr(&output).contains(&format!("axon show {id}")));
    assert!(!stderr(&output).contains("decide accept"));
    assert_eq!(repo.snapshot(&id), before);

    repo.accept(&id);
    let before = repo.snapshot(&id);
    let output = repo.axon(&["write", &id, "--title", "changed"]);
    assert_failure(&output);
    assert!(stderr(&output).contains("If changing the plan is intended"));
    assert!(stderr(&output).contains("decide separately"));
    assert_eq!(repo.snapshot(&id), before);

    let output = repo.axon(&["note", "show", &id, "999"]);
    assert_failure(&output);
    assert!(stderr(&output).contains(&format!("axon note list {id}")));
}

#[test]
fn show_leads_with_situation_remaining_and_owned_waits() {
    let repo = TestRepo::new();
    repo.init("test");
    let root = created_id(&repo.axon(&["group", "plan", "delivery", "-m", "full\n  description"]));
    let nested = created_id(&repo.axon(&["group", "plan", "abandoned scope", "--parent", &root]));
    let child = created_id(&repo.axon(&["plan", "preserved child", "--parent", &nested]));
    let overlap = created_id(&repo.axon(&["plan", "ended and rejected", "--parent", &root]));
    let manual = created_id(&repo.axon(&["group", "plan", "manual scope", "--parent", &root]));
    let manual_child = created_id(&repo.axon(&["plan", "manual child", "--parent", &manual]));
    let prerequisite = repo.plan("lost prerequisite");
    repo.add_dependency(&child, &prerequisite);
    assert_success(&repo.axon(&["decide", "reject", &prerequisite]));
    assert_success(&repo.axon(&["when", "after", &child, &prerequisite]));
    assert_success(&repo.axon(&["when", "manual", &manual]));
    assert_success(&repo.axon(&["note", "add", &root, "-m", "note one\n  untouched"]));
    assert_success(&repo.axon(&["note", "add", &root, "-m", "note two"]));
    let before = stdout(&repo.axon(&["show", &root]));
    assert!(before.contains("Situation: Ready to start"));
    assert!(before.contains("Completion requires Group Progress=InProgress."));
    assert_eq!(
        before
            .matches(&format!("Descendant gate closed: {root}"))
            .count(),
        1
    );
    assert_success(&repo.axon(&["start", &root]));
    assert_success(&repo.axon(&["start", &overlap]));
    assert_success(&repo.axon(&["done", &overlap]));
    assert_success(&repo.axon(&["decide", "reject", &overlap]));
    assert_success(&repo.axon(&["decide", "reject", &nested]));
    let show = stdout(&repo.axon(&["show", &root]));
    assert!(show.contains("Situation: InProgress (saved claim)"));
    assert!(show.contains("Descendants: Ended: 1  Rejected: 2  Unfinished: 3"));
    assert!(
        show.contains(
            "Ended and Rejected overlap: 1; Unfinished excludes both (not a commitment)."
        )
    );
    assert!(show.contains("Completion requires 3 unfinished descendants to become terminal."));
    assert!(show.contains("Rejected Group: already terminal"));
    assert!(show.contains(&format!("{child}  Issue  [NotStarted/Accepted]  preserved child\n      AfterEntity: {prerequisite}; satisfied by Ended or Rejected (yes).\n      Rejected prerequisite (Orphaned): {prerequisite}")));
    assert!(show.contains(&format!("Descendant gate closed: {manual} (Progress=NotStarted, Disposition=Accepted)\n    Not surfaced: Manual")));
    assert!(!show.contains("  Inactive:"));
    for (a, b) in [
        ("Situation:", "Group\n"),
        ("Group\n", "Subtree\n"),
        ("Subtree\n", "Details\n"),
        ("Details\n", "Description\n"),
        ("Description\n", "Notes\n"),
        ("Notes\n", "Progress history\n"),
    ] {
        assert!(show.find(a).unwrap() < show.find(b).unwrap(), "{show}");
    }
    assert!(show.contains("Description\nfull\n  description"));
    assert!(show.contains("note one\n  untouched"));
    assert!(show.contains("note two"));
    assert!(show.contains(&format!(
        "Plan declaration: fixed at Revision {}",
        repo.revision_id(&root, 1)
    )));
    assert!(show.contains("Records: Notes: 2"));
    assert!(show.contains("Claim: test-actor"));
    let child_show = stdout(&repo.axon(&["show", &manual_child]));
    assert!(child_show.contains("Situation: Not started; outside active scope"));
    assert_eq!(
        child_show
            .matches(&format!("Descendant gate closed: {manual}"))
            .count(),
        1
    );
    assert!(child_show.contains("Not surfaced: Manual"));
    assert!(child_show.find("Ancestor scope:").unwrap() < child_show.find("Details\n").unwrap());
}

#[test]
fn show_lists_direct_after_entity_waiters_with_saved_state() {
    let repo = TestRepo::new();
    repo.init("test");

    let issue_target = repo.plan("issue target");
    let dependent = repo.plan("dependency dependent");
    repo.add_dependency(&dependent, &issue_target);
    let issue_waiter = repo.plan("issue waiter");
    assert_success(&repo.axon(&["when", "after", &issue_waiter, &issue_target]));
    let inactive_parent = repo.group_plan("inactive parent");
    let group_waiter = repo.group_plan("inactive group waiter");
    repo.set_parent(&group_waiter, &inactive_parent);
    assert_success(&repo.axon(&["when", "after", &group_waiter, &issue_target]));
    let group_waiter_child =
        created_id(&repo.axon(&["plan", "group waiter child", "--parent", &group_waiter]));

    let group_target = repo.group_plan("group target");
    let ended_issue_waiter = repo.plan("ended issue waiter");
    assert_success(&repo.axon(&["when", "after", &ended_issue_waiter, &group_target]));
    let rejected_group_waiter = repo.group_plan("rejected group waiter");
    assert_success(&repo.axon(&["when", "after", &rejected_group_waiter, &group_target]));
    assert_success(&repo.axon(&["decide", "reject", &group_target]));
    assert_success(&repo.axon(&["start", &ended_issue_waiter]));
    assert_success(&repo.axon(&["done", &ended_issue_waiter]));
    assert_success(&repo.axon(&["decide", "reject", &rejected_group_waiter]));

    let unrelated_target = repo.plan("unrelated target");
    let unrelated_waiter = repo.plan("unrelated waiter");
    assert_success(&repo.axon(&["when", "after", &unrelated_waiter, &unrelated_target]));
    let no_waiters = repo.plan("no waiters");

    for args in [
        vec!["show", issue_target.as_str()],
        vec!["show", issue_target.as_str(), "--skip-command-evaluation"],
    ] {
        let show = stdout(&repo.axon(&args));
        assert!(
            show.lines()
                .any(|line| line.contains("Dependent:") && line.contains(&dependent)),
            "{show}"
        );
        assert!(
            show.lines().any(|line| line.contains("AfterEntity waiter:")
                && line.contains(&format!(
                    "{issue_waiter}  Issue  [NotStarted/Accepted]  issue waiter"
                ))),
            "{show}"
        );
        assert!(
            show.lines().any(|line| line.contains("AfterEntity waiter:")
                && line.contains(&format!(
                    "{group_waiter}  Group  [NotStarted/Accepted]  inactive group waiter"
                ))),
            "{show}"
        );
        assert!(!show.contains(&unrelated_waiter), "{show}");
        assert!(!show.contains(&group_waiter_child), "{show}");
    }

    for args in [
        vec!["show", group_target.as_str()],
        vec!["show", group_target.as_str(), "--skip-command-evaluation"],
    ] {
        let show = stdout(&repo.axon(&args));
        assert!(
            show.lines().any(|line| line.contains("AfterEntity waiter:")
                && line.contains(&format!(
                    "{ended_issue_waiter}  Issue  [Ended/Accepted]  ended issue waiter"
                ))),
            "{show}"
        );
        assert!(
            show.lines().any(|line| line.contains("AfterEntity waiter:")
                && line.contains(&format!(
                    "{rejected_group_waiter}  Group  [NotStarted/Rejected]  rejected group waiter"
                ))),
            "{show}"
        );
    }

    let no_waiters = stdout(&repo.axon(&["show", &no_waiters]));
    assert!(!no_waiters.contains("AfterEntity waiter:"), "{no_waiters}");
}

#[test]
fn show_distinguishes_explicit_completion_from_an_ended_group() {
    let repo = TestRepo::new();
    repo.init("test");
    let group = repo.group_plan("empty plan");
    assert_success(&repo.axon(&["start", &group]));
    let awaiting = stdout(&repo.axon(&["show", &group]));
    assert!(awaiting.contains("Descendants: Ended: 0  Rejected: 0  Unfinished: 0"));
    assert!(awaiting.contains("Can complete: yes"));
    assert!(awaiting.contains("All descendants are terminal; awaiting explicit done."));
    assert!(awaiting.contains("No descendants"));
    assert_success(&repo.axon(&["done", &group]));
    assert_success(&repo.axon(&["decide", "reject", &group]));
    let ended = stdout(&repo.axon(&["show", &group]));
    assert!(ended.contains("Situation: Ended; Rejected"));
    assert!(
        !ended
            .split_once("\n\nDetails\n")
            .unwrap()
            .0
            .contains("Can complete:")
    );
    assert!(!ended.contains("Inactive:"));
}

#[test]
fn show_treats_rejected_groups_as_terminal_without_hiding_saved_state() {
    let repo = TestRepo::new();
    repo.init("test");

    let not_started = repo.group_plan("rejected before start");
    let saved_child = created_id(&repo.axon(&["plan", "saved child", "--parent", &not_started]));
    let dependency = repo.plan("external prerequisite");
    repo.add_dependency(&saved_child, &dependency);
    let nested = created_id(&repo.axon(&[
        "group",
        "plan",
        "rejected nested condition",
        "--parent",
        &not_started,
    ]));
    assert_success(&repo.axon(&["when", "after", &nested, &dependency]));
    assert_success(&repo.axon(&["decide", "reject", &nested]));
    assert_success(&repo.axon(&["decide", "reject", &not_started]));

    let rejected = stdout(&repo.axon(&["show", &not_started]));
    assert!(rejected.contains("Situation: Rejected"));
    assert!(rejected.contains("Rejected Group: already terminal"));
    assert!(rejected.contains(
        "Unfinished descendants are saved states and do not require follow-up by themselves."
    ));
    assert!(!rejected.contains("Can complete:"));
    assert!(!rejected.contains("Completion requires"));
    assert!(rejected.contains(&format!(
        "{saved_child}  Issue  [NotStarted/Accepted]  saved child"
    )));
    assert!(rejected.contains(&format!("Needs: {dependency}")));
    assert!(rejected.contains("Unresolved"));
    assert!(rejected.contains(&format!(
        "{nested}  Group  [NotStarted/Rejected]  rejected nested condition"
    )));
    assert!(rejected.contains(&format!(
        "AfterEntity: {dependency}; satisfied by Ended or Rejected (no)."
    )));

    let in_progress = repo.group_plan("rejected after start");
    let claimed_child =
        created_id(&repo.axon(&["plan", "claimed child", "--parent", &in_progress]));
    assert_success(&repo.axon(&["start", &in_progress]));
    assert_success(&repo.axon(&["start", &claimed_child]));
    assert_success(&repo.axon(&["decide", "reject", &in_progress]));

    let claimed = stdout(&repo.axon(&["show", &in_progress]));
    assert!(claimed.contains("Situation: Rejected"));
    assert!(claimed.contains("Saved claims remain: 2."));
    assert!(claimed.contains(
        "decide separately whether its external work should end, be released, or continue outside this Group."
    ));
    assert!(claimed.contains("Claim: test-actor"));
    assert!(claimed.contains(&format!(
        "{claimed_child}  Issue  [InProgress/Accepted]  claimed child"
    )));
    assert!(!claimed.contains("Can complete:"));
    assert!(!claimed.contains("Completion requires"));

    let ended = repo.group_plan("ended accepted");
    assert_success(&repo.axon(&["start", &ended]));
    assert_success(&repo.axon(&["done", &ended]));
    let ended = stdout(&repo.axon(&["show", &ended]));
    assert!(ended.contains("Situation: Ended"));
    assert!(
        !ended
            .split_once("\n\nDetails\n")
            .unwrap()
            .0
            .contains("Can complete:")
    );

    let unfinished = repo.group_plan("unfinished accepted");
    let unfinished = stdout(&repo.axon(&["show", &unfinished]));
    assert!(unfinished.contains("Situation: Ready to start"));
    assert!(unfinished.contains("Can complete: no"));
    assert!(unfinished.contains("Completion requires Group Progress=InProgress."));
}

#[test]
fn show_keeps_inherited_dependency_gates_at_the_owning_scope() {
    let repo = TestRepo::new();
    repo.init("test");
    let root = repo.group_plan("root");
    let nested = created_id(&repo.axon(&["group", "plan", "nested", "--parent", &root]));
    let child = created_id(&repo.axon(&["plan", "child", "--parent", &nested]));
    created_id(&repo.axon(&["plan", "waiting child", "--parent", &nested]));
    created_id(&repo.axon(&["capture", "draft child", "--parent", &nested]));
    let target = repo.plan("prerequisite");
    assert_success(&repo.axon(&["start", &root]));
    assert_success(&repo.axon(&["start", &nested]));
    assert_success(&repo.axon(&["start", &child]));
    let claims = stdout(&repo.axon(&["claims"]));
    repo.add_dependency(&root, &target);
    for rejected in [false, true] {
        if rejected {
            assert_success(&repo.axon(&["decide", "reject", &target]));
        }
        for selected in [&root, &nested] {
            let show = stdout(&repo.axon(&["show", selected]));
            for counts in [
                if selected == &root {
                    "Descendants: 4  Issue: 3  Group: 1  Terminal: 0"
                } else {
                    "Descendants: 3  Issue: 3  Group: 0  Terminal: 0"
                },
                if selected == &root {
                    "Progress: NotStarted: 2  InProgress: 2  Ended: 0"
                } else {
                    "Progress: NotStarted: 2  InProgress: 1  Ended: 0"
                },
            ] {
                assert!(show.contains(counts), "{show}");
            }
            assert!(!show.contains("  Ready"));
        }
        for selected in [&root, &nested, &child] {
            let show = stdout(&repo.axon(&["show", selected]));
            let summary = show.split_once("\n\nDetails\n").unwrap().0;
            assert_eq!(
                summary.matches("Descendant gate closed:").count(),
                1,
                "{show}"
            );
            assert!(summary.contains(&format!("Descendant gate closed: {root}")));
            assert!(summary.contains(&format!(
                "{}: {target}",
                if rejected {
                    "Rejected prerequisite (Orphaned)"
                } else {
                    "Unresolved dependency (Blocked)"
                }
            )));
        }
        assert_eq!(stdout(&repo.axon(&["claims"])), claims);
    }
    let local_target = repo.plan("nested prerequisite");
    for local_gate in ["manual", "dependency", "rejected dependency"] {
        match local_gate {
            "manual" => assert_success(&repo.axon(&["when", "manual", &nested])),
            "dependency" => {
                assert_success(&repo.axon(&["when", "clear", &nested]));
                repo.add_dependency(&nested, &local_target);
            }
            _ => assert_success(&repo.axon(&["decide", "reject", &local_target])),
        }
        for selected in [&root, &nested] {
            let show = stdout(&repo.axon(&["show", selected]));
            let summary = show.split_once("\n\nDetails\n").unwrap().0;
            assert_eq!(
                summary.matches("Descendant gate closed:").count(),
                2,
                "{show}"
            );
            assert!(summary.contains(&format!("Descendant gate closed: {nested}")));
            if local_gate == "manual" {
                assert!(summary.contains("Not surfaced: Manual"));
            } else {
                assert!(summary.contains(&local_target));
            }
        }
        assert_eq!(stdout(&repo.axon(&["claims"])), claims);
    }
}

#[test]
fn show_keeps_ended_group_structure_out_of_the_summary_and_in_details() {
    let repo = TestRepo::new();
    repo.init("test");
    let root = repo.group_plan("completed root");
    let nested = created_id(&repo.axon(&["group", "plan", "completed nested", "--parent", &root]));
    assert_success(&repo.axon(&["start", &root]));
    assert_success(&repo.axon(&["start", &nested]));
    assert_success(&repo.axon(&["done", &nested]));
    assert_success(&repo.axon(&["done", &root]));

    for selected in [&root, &nested] {
        let show = stdout(&repo.axon(&["show", selected]));
        let (summary, details) = show.split_once("\n\nDetails\n").unwrap();
        assert!(!summary.contains("Descendant gate closed:"), "{show}");
        assert!(!summary.contains("Can complete:"), "{show}");
        assert!(details.contains("Descendant gate closed:"), "{show}");
        assert!(details.contains("Can complete: no"), "{show}");
    }
}

#[test]
fn show_preserves_ended_rejected_group_dependencies_and_inactive_context() {
    let repo = TestRepo::new();
    repo.init("test");
    let outer = repo.group_plan("active outer plan");
    assert_success(&repo.axon(&["start", &outer]));
    let root = created_id(&repo.axon(&[
        "group",
        "plan",
        "ended with saved claim",
        "--parent",
        &outer,
    ]));
    let child = created_id(&repo.axon(&["plan", "rejected running child", "--parent", &root]));
    let unresolved = repo.plan("unresolved root prerequisite");
    let rejected = repo.plan("rejected root prerequisite");
    assert_success(&repo.axon(&["start", &root]));
    assert_success(&repo.axon(&["start", &child]));
    assert_success(&repo.axon(&["decide", "reject", &child]));
    repo.add_dependency(&root, &unresolved);
    repo.add_dependency(&root, &rejected);
    assert_success(&repo.axon(&["decide", "reject", &rejected]));
    assert_success(&repo.axon(&["when", "manual", &root]));
    assert_success(&repo.axon(&["done", &root]));
    assert_success(&repo.axon(&["decide", "reject", &root]));

    let root_show = stdout(&repo.axon(&["show", &root]));
    assert!(
        root_show.contains(&format!("Needs: {unresolved}")),
        "{root_show}"
    );
    assert!(
        root_show.contains(&format!("Needs: {rejected}")),
        "{root_show}"
    );
    assert!(
        root_show.contains("Resurface condition: Manual"),
        "{root_show}"
    );
    for selected in [&outer, &root, &child] {
        let show = stdout(&repo.axon(&["show", selected]));
        assert!(
            show.split_once("\n\nDetails\n")
                .unwrap()
                .0
                .contains("descendant scope is inactive"),
            "{show}"
        );
    }
}
