use crate::common::{TestDir, TestRepo, assert_failure, assert_success, stderr, stdout};
use std::fs;

#[cfg(unix)]
fn process_exists(pid: i32) -> bool {
    let result = unsafe { libc::kill(pid, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(unix)]
fn wait_for_file(path: &std::path::Path) -> String {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        if let Ok(value) = fs::read_to_string(path) {
            return value;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("timed out waiting for {}", path.display());
}

#[cfg(unix)]
fn assert_process_exits(pid: i32) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        if !process_exists(pid) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("process {pid} remained after condition evaluation");
}

fn calls(repo: &TestRepo) -> usize {
    fs::read_to_string(repo.root().join("calls"))
        .unwrap_or_default()
        .lines()
        .count()
}

#[test]
fn skipped_show_lists_after_entity_waiters_without_running_commands() {
    let repo = TestRepo::new();
    repo.init("test");
    let target = repo.plan("target");
    let waiter = repo.plan("waiter");
    assert_success(&repo.axon(&["when", "after", &waiter, &target]));
    let command = repo.plan("command condition");
    assert_success(&repo.axon(&["when", "command", &command, "echo called >> calls; exit 0"]));

    let shown = repo.axon(&["show", &target, "--skip-command-evaluation"]);
    assert_success(&shown);
    assert!(stderr(&shown).is_empty());
    assert!(stdout(&shown).contains(&format!(
        "AfterEntity waiter: {waiter}  Issue  [NotStarted/Accepted]  waiter"
    )));
    assert_eq!(calls(&repo), 0);
}

#[test]
fn command_is_observed_once_per_invocation_and_can_stop_surfacing() {
    let repo = TestRepo::new();
    repo.init("test");
    let id = repo.plan("external wait");
    let script =
        "echo call >> calls; echo external-stdout; echo external-stderr >&2; test -f satisfied";
    let revision = repo.current_revision(&id);
    assert_success(&repo.axon(&["when", "command", &id, script, "-r", "wait for release"]));
    assert_eq!(calls(&repo), 0);
    assert_eq!(repo.current_revision(&id), revision);
    let stored = repo.snapshot(&id);
    assert_eq!(stored.resurface_command.as_deref(), Some(script));
    let hidden = repo.axon(&["ready"]);
    assert_success(&hidden);
    assert!(stdout(&hidden).is_empty());
    assert!(!stderr(&hidden).contains("external-stderr"));
    assert_eq!(calls(&repo), 1);
    fs::write(repo.root().join("satisfied"), "").unwrap();
    let ready = repo.axon(&["ready"]);
    assert_success(&ready);
    assert!(stdout(&ready).contains(&id));
    assert!(!stdout(&ready).contains("external-stdout"));
    assert_eq!(calls(&repo), 2);
    fs::remove_file(repo.root().join("satisfied")).unwrap();
    let show = repo.axon(&["show", &id]);
    assert_success(&show);
    assert!(stdout(&show).contains("Surfaced: no"));
    assert_eq!(calls(&repo), 3);
    assert_eq!(repo.snapshot(&id), stored);
    let history = stdout(&repo.axon(&["log", &id]));
    assert!(history.contains("wait for release"));
    assert!(history.contains("Command("));
    assert_eq!(calls(&repo), 3);
}

#[test]
fn trace_reports_normal_results_without_changing_default_output() {
    let repo = TestRepo::new();
    repo.init("test");
    let id = repo.plan("trace output");
    let script =
        "printf 'child-out\\033[2J'; printf '\\377child-err\\033]0;spoof\\007' >&2; exit 0";
    assert_success(&repo.axon(&["when", "command", &id, script]));

    let ordinary = repo.axon(&["ready"]);
    assert_success(&ordinary);
    assert!(stderr(&ordinary).is_empty());

    let traced = repo.axon(&["ready", "--trace-conditions"]);
    assert_success(&traced);
    assert!(stdout(&traced).contains(&id));
    let trace = stderr(&traced);
    for expected in [
        &format!("Condition trace: {id}"),
        &format!("cwd: {}", repo.root().display()),
        "result: satisfied (exit 0)",
        "stdout:\nchild-out\\x1b[2J\n",
        "stderr:\n�child-err\\x1b]0;spoof\\x07\n",
        &format!("End condition trace: {id}"),
    ] {
        assert!(trace.contains(expected), "{trace}");
    }
    assert_no_terminal_controls(&trace);

    assert_success(&repo.axon(&["when", "command", &id, "exit 1"]));
    let waiting = repo.axon(&["ready", "--trace-conditions"]);
    assert_success(&waiting);
    assert!(stdout(&waiting).is_empty());
    let trace = stderr(&waiting);
    assert!(trace.contains("result: not satisfied (exit 1)"), "{trace}");
    assert_eq!(trace.matches("(empty)").count(), 2, "{trace}");
}

#[test]
fn command_output_is_bounded_per_stream_with_edges_and_omitted_bytes() {
    let repo = TestRepo::new();
    repo.init("test");
    let id = repo.plan("large output");
    let script = "awk 'BEGIN { for (i=0;i<40000;i++) printf \"A\"; for (i=0;i<40000;i++) printf \"B\" }'; awk 'BEGIN { for (i=0;i<40000;i++) printf \"C\"; for (i=0;i<40000;i++) printf \"D\" }' >&2; exit 23";
    assert_success(&repo.axon(&["when", "command", &id, script]));

    let output = repo.axon(&["show", &id]);
    assert_failure(&output);
    let diagnostic = stderr(&output);
    let streams = diagnostic.split_once("\nstdout:\n").unwrap().1;
    let (stdout_diagnostic, stderr_and_help) = streams.split_once("\nstderr:\n").unwrap();
    let stderr_diagnostic = stderr_and_help.split_once("\nHelp:").unwrap().0;
    for (stream, first, last) in [(stdout_diagnostic, 'A', 'B'), (stderr_diagnostic, 'C', 'D')] {
        assert!(stream.starts_with(&first.to_string().repeat(32 * 1024)));
        assert!(stream.ends_with(&last.to_string().repeat(32 * 1024)));
        assert!(stream.contains("... 14464 bytes omitted ..."));
    }
}

#[test]
#[cfg(unix)]
fn timeout_kills_the_process_group_and_does_not_start_the_entity() {
    let repo = TestRepo::new();
    repo.init("test");
    let id = repo.plan("timeout");
    let script = "echo $$ > condition-shell-pid; sh -c 'trap \"\" TERM; echo $$ > condition-descendant-pid; while :; do :; done' </dev/null >/dev/null 2>/dev/null & wait";
    assert_success(&repo.axon(&["when", "command", &id, script]));
    let before = repo.snapshot(&id);

    let output = repo.axon(&["start", &id, "--condition-timeout", "500ms"]);
    assert_failure(&output);
    let diagnostic = stderr(&output);
    assert!(diagnostic.contains(&id), "{diagnostic}");
    assert!(diagnostic.contains("timed out after 500ms"), "{diagnostic}");
    assert!(
        diagnostic.contains("TERM followed by KILL after 1s grace"),
        "{diagnostic}"
    );
    assert_eq!(repo.snapshot(&id), before);

    for name in ["condition-shell-pid", "condition-descendant-pid"] {
        let pid = wait_for_file(&repo.root().join(name))
            .trim()
            .parse()
            .unwrap();
        assert_process_exits(pid);
    }
}

#[test]
#[cfg(unix)]
fn ctrl_c_terminates_descendants_and_does_not_start_the_entity() {
    use std::process::Stdio;

    let repo = TestRepo::new();
    repo.init("test");
    let id = repo.plan("interrupt");
    let script = "echo $$ > interrupt-shell-pid; sh -c 'echo $$ > interrupt-descendant-pid; while :; do sleep 1; done' & wait";
    assert_success(&repo.axon(&["when", "command", &id, script]));
    let before = repo.snapshot(&id);

    let child = repo
        .axon_command()
        .args(["start", &id, "--condition-timeout", "10s"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let descendant = wait_for_file(&repo.root().join("interrupt-descendant-pid"))
        .trim()
        .parse()
        .unwrap();
    assert_eq!(unsafe { libc::kill(child.id() as i32, libc::SIGINT) }, 0);
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    let diagnostic = stderr(&output);
    assert!(diagnostic.contains("interrupted by Ctrl-C"), "{diagnostic}");
    assert!(
        diagnostic.contains("process group termination"),
        "{diagnostic}"
    );
    assert_eq!(repo.snapshot(&id), before);
    assert_process_exits(descendant);
}

#[test]
fn trace_follows_evaluation_order_and_skips_memoized_references() {
    let repo = TestRepo::new();
    repo.init("test");
    let first = repo.plan("first trace");
    let second = repo.plan("second trace");
    assert_success(&repo.axon(&["when", "command", &first, "echo first; exit 0"]));
    assert_success(&repo.axon(&["when", "command", &second, "echo second; exit 0"]));

    let listed = repo.axon(&["list", "--trace-conditions"]);
    assert_success(&listed);
    let trace = stderr(&listed);
    assert_eq!(trace.matches("Condition trace:").count(), 2, "{trace}");
    assert!(
        trace.find(&format!("Condition trace: {first}")).unwrap()
            < trace.find(&format!("Condition trace: {second}")).unwrap(),
        "{trace}"
    );

    let shown = repo.axon(&["show", &first, "--trace-conditions"]);
    assert_success(&shown);
    let trace = stderr(&shown);
    assert_eq!(
        trace.matches(&format!("Condition trace: {first}")).count(),
        1,
        "{trace}"
    );
    assert_eq!(calls(&repo), 0);
}

#[test]
fn condition_evaluating_leaf_commands_enable_the_shared_trace() {
    let repo = TestRepo::new();
    repo.init("test");
    let ready = repo.plan("ready trace");
    let triage = repo.capture("triage trace");
    assert_success(&repo.axon(&["when", "command", &ready, "exit 0"]));
    assert_success(&repo.axon(&["when", "command", &triage, "exit 0"]));

    for args in [
        vec!["ready", "--trace-conditions"],
        vec!["triage", "--trace-conditions"],
        vec!["list", "--trace-conditions"],
        vec!["show", &ready, "--trace-conditions"],
    ] {
        let output = repo.axon(&args);
        assert_success(&output);
        assert!(stderr(&output).contains("Condition trace:"), "{args:?}");
    }

    let started = repo.axon(&["start", &ready, "--trace-conditions"]);
    assert_success(&started);
    assert!(stderr(&started).contains(&format!("Condition trace: {ready}")));
    assert_eq!(repo.snapshot(&ready).progress, "in_progress");
}

#[test]
fn evaluation_failure_is_diagnostic_and_start_is_not_written() {
    let repo = TestRepo::new();
    repo.init("test");
    let id = repo.plan("failing wait");
    let script = "echo call >> calls; printf 'diagnostic-out\\033[2J\\n'; printf 'diagnostic-err\\033]0;spoof\\007\\n' >&2; exit 23 # \u{1b}\t\r\u{85}";
    assert_success(&repo.axon(&["when", "command", &id, script]));
    let before = repo.snapshot(&id);
    for args in [
        vec!["show", &id, "--trace-conditions"],
        vec!["list", "--trace-conditions"],
        vec!["ready", "--trace-conditions"],
        vec!["start", &id, "--trace-conditions"],
    ] {
        let output = repo.axon(&args);
        assert_failure(&output);
        assert!(stdout(&output).is_empty());
        let error = stderr(&output);
        for expected in [
            &id,
            "printf 'diagnostic-out\\033",
            "# \\x1b\\t\\r\\x85",
            "23",
            "diagnostic-out\\x1b[2J",
            "diagnostic-err\\x1b]0;spoof\\x07",
        ] {
            assert!(error.contains(expected), "{error}");
        }
        assert_no_terminal_controls(&error);
        assert!(!error.contains("Condition trace:"), "{error}");
        assert_eq!(repo.snapshot(&id), before);
    }
    assert_eq!(calls(&repo), 4);
    assert_success(&repo.axon(&["when", "command", &id, "exit 0"]));
    assert_eq!(calls(&repo), 4);
    assert_success(&repo.axon(&["start", &id]));
    assert_success(&repo.axon(&["when", "command", &id, "exit 7"]));
    assert_success(&repo.axon(&["when", "clear", &id]));
    assert!(repo.snapshot(&id).resurface_command.is_none());
    assert_success(&repo.axon(&["show", &id]));
}

fn assert_no_terminal_controls(value: &str) {
    assert!(
        value.chars().all(|character| {
            character == '\n' || !matches!(character, '\u{00}'..='\u{1f}' | '\u{7f}'..='\u{9f}')
        }),
        "unexpected terminal control in {value:?}"
    );
}

#[cfg(unix)]
#[test]
fn trace_write_failure_aborts_start_before_state_change() {
    let repo = TestRepo::new();
    repo.init("test");
    let id = repo.plan("closed trace sink");
    assert_success(&repo.axon(&["when", "command", &id, "exit 0"]));
    let before = repo.snapshot(&id);
    let output = repo.axon_with_closed_stderr(&["start", &id, "--trace-conditions"]);
    assert_failure(&output);
    assert_eq!(repo.snapshot(&id), before);
}

#[cfg(unix)]
#[test]
fn signal_failure_and_triage_failure_are_not_unsatisfied_conditions() {
    let repo = TestRepo::new();
    repo.init("test");
    let id = repo.capture("triage wait");
    assert_success(&repo.axon(&[
        "when",
        "command",
        &id,
        "echo signal-detail >&2; kill -TERM $$",
    ]));
    let output = repo.axon(&["triage"]);
    assert_failure(&output);
    assert!(stderr(&output).contains("signal"));
    assert!(stderr(&output).contains("signal-detail"));
    assert!(stdout(&output).is_empty());
    assert_success(&repo.axon(&["when", "clear", &id]));
    assert!(stdout(&repo.axon(&["triage"])).contains(&id));
}

#[test]
fn stored_reads_and_relation_repairs_do_not_execute_conditions() {
    let repo = TestRepo::new();
    repo.init("test");
    let id = repo.capture("draft");
    let group = repo.group_capture("parent");
    let other = repo.plan("reference");
    assert_success(&repo.axon(&["when", "command", &id, "echo call >> calls; exit 2"]));
    for args in [
        vec!["log", &id],
        vec!["claims"],
        vec!["export", &id],
        vec!["note", "list", &id],
        vec!["revision", "list", &id],
        vec!["write", &id, "--title", "updated draft"],
        vec!["group", "set", &id, &group],
        vec!["group", "unset", &id],
        vec!["dep", "add", &id, "--needs", &other],
        vec!["dep", "rm", &id, "--needs", &other],
    ] {
        assert_success(&repo.axon(&args));
        assert_eq!(calls(&repo), 0);
    }
    assert_success(&repo.axon(&["when", "after", &id, &other]));
    assert_success(&repo.axon(&["when", "at", &id, "2100-01-01T00:00:00Z"]));
    assert_success(&repo.axon(&["when", "command", &id, "exit 0"]));
    assert_success(&repo.axon(&["when", "clear", &id]));
    assert_eq!(calls(&repo), 0);
}

#[test]
fn group_observation_is_shared_and_closing_gate_preserves_claims() {
    let repo = TestRepo::new();
    repo.init("test");
    let group = repo.group_plan("group");
    let first = repo.plan("first");
    let second = repo.plan("second");
    repo.set_parent(&first, &group);
    repo.set_parent(&second, &group);
    fs::write(repo.root().join("satisfied"), "").unwrap();
    assert_success(&repo.axon(&[
        "when",
        "command",
        &group,
        "echo call >> calls; test -f satisfied",
    ]));
    assert_success(&repo.axon(&["start", &group]));
    assert_eq!(calls(&repo), 1);
    assert_success(&repo.axon(&["start", &first]));
    assert_eq!(calls(&repo), 2);
    let before = repo.snapshot(&first);
    let claims = stdout(&repo.axon(&["claims"]));
    let show = repo.axon(&["show", &group]);
    assert_success(&show);
    assert!(stdout(&show).contains("Ready"));
    assert_eq!(calls(&repo), 3);
    fs::remove_file(repo.root().join("satisfied")).unwrap();
    let closed = repo.axon(&["show", &group]);
    assert_success(&closed);
    assert!(stdout(&closed).contains(&format!("Descendant gate closed: {group}")));
    assert!(stdout(&closed).contains("Not surfaced: Command"));
    assert_eq!(calls(&repo), 4);
    assert_eq!(repo.snapshot(&first), before);
    assert_eq!(stdout(&repo.axon(&["claims"])), claims);
    assert!(stdout(&repo.axon(&["ready"])).is_empty());
    assert_eq!(calls(&repo), 5);
    assert_success(&repo.axon(&["when", "clear", &group]));
    assert!(stdout(&repo.axon(&["ready"])).contains(&second));
}

#[test]
fn command_uses_worktree_root_and_inherits_environment_without_shell_startup() {
    let repo = TestRepo::new();
    repo.init("test");
    let id = repo.plan("environment");
    let worktree = repo.add_worktree();
    assert_failure(&repo.axon_in(&worktree, &["init"]));
    let nested = worktree.join("nested");
    fs::create_dir(&nested).unwrap();
    let startup = repo.root().join("startup.sh");
    fs::write(&startup, "echo unwanted > startup-ran\n").unwrap();
    let script = "pwd > observed-pwd; test \"$AXON_TEST_VALUE\" = inherited";
    assert_success(&repo.axon(&["when", "command", &id, script]));
    let dir = TestDir::new("environment");
    let output = dir
        .axon_command_in(&nested)
        .env("AXON_TEST_VALUE", "inherited")
        .env("ENV", &startup)
        .env("BASH_ENV", &startup)
        .args(["ready"])
        .output()
        .unwrap();
    assert_success(&output);
    assert!(stdout(&output).contains(&id));
    assert_eq!(
        fs::read_to_string(worktree.join("observed-pwd"))
            .unwrap()
            .trim(),
        worktree.to_str().unwrap()
    );
    assert!(!worktree.join("startup-ran").exists());
    assert!(!repo.root().join("observed-pwd").exists());
}

#[test]
fn command_uses_management_root_outside_git() {
    let dir = TestDir::new("command-root");
    assert_success(&dir.axon_command().args(["init", "test"]).output().unwrap());
    let created = dir.axon_command().args(["plan", "root"]).output().unwrap();
    assert_success(&created);
    let id = stdout(&created)
        .split_whitespace()
        .next()
        .unwrap()
        .to_owned();
    assert_success(
        &dir.axon_command()
            .args(["when", "command", &id, "pwd > observed-pwd"])
            .output()
            .unwrap(),
    );
    let nested = dir.path().join("nested");
    fs::create_dir(&nested).unwrap();
    assert_success(&dir.axon_in(&nested, &["show", &id]));
    assert_eq!(
        fs::read_to_string(dir.path().join("observed-pwd"))
            .unwrap()
            .trim(),
        dir.path().to_str().unwrap()
    );
}

#[test]
fn import_shares_observation_before_and_after_and_aborts_on_failure() {
    let repo = TestRepo::new();
    repo.init("test");
    let id = repo.capture("before title");
    let script = "echo call >> calls; if test -f broken; then exit 8; fi; exit 0";
    assert_success(&repo.axon(&["when", "command", &id, script]));
    let exported = repo.axon(&["export", &id]);
    assert_success(&exported);
    let yaml = stdout(&exported);
    assert!(yaml.contains("kind: command"));
    assert!(yaml.contains(script));
    assert_eq!(calls(&repo), 0);
    let path = repo.root().join("plan.yaml");
    fs::write(&path, yaml.replace("before title", "after title")).unwrap();
    let file = path.to_str().unwrap();
    assert_success(&repo.axon(&["import", "prepare", file]));
    assert_eq!(calls(&repo), 0);
    let checked = repo.axon(&["import", "check", file, "--trace-conditions"]);
    assert_success(&checked);
    assert!(stderr(&checked).is_empty());
    // An Undecided root has no ready or active-scope observation to perform.
    assert_eq!(calls(&repo), 0);
    let applied = repo.axon(&["import", "apply", file, "--trace-conditions"]);
    assert_success(&applied);
    assert!(stderr(&applied).is_empty());
    assert_eq!(calls(&repo), 0);

    let group = repo.group_plan("gate");
    assert_success(&repo.axon(&["when", "clear", &id]));
    repo.accept(&id);
    repo.set_parent(&id, &group);
    repo.undecide(&id);
    assert_success(&repo.axon(&["start", &group]));
    assert_success(&repo.axon(&["when", "command", &group, script]));
    let exported = repo.axon(&["export", &id]);
    fs::write(
        &path,
        stdout(&exported).replace("after title", "third title"),
    )
    .unwrap();
    let checked = repo.axon(&["import", "check", file, "--trace-conditions"]);
    assert_success(&checked);
    assert!(stderr(&checked).contains(&format!("Condition trace: {group}")));
    assert_eq!(calls(&repo), 1);
    let applied = repo.axon(&["import", "apply", file, "--trace-conditions"]);
    assert_success(&applied);
    assert!(stderr(&applied).contains(&format!("Condition trace: {group}")));
    assert_eq!(calls(&repo), 2);
    let current = fs::read_to_string(&path).unwrap();
    fs::write(&path, current.replace("third title", "fourth title")).unwrap();
    fs::write(repo.root().join("broken"), "").unwrap();
    let before = fs::read(repo.root().join(".axon/axon.db")).unwrap();
    let output = repo.axon(&["import", "apply", file]);
    assert_failure(&output);
    assert!(stderr(&output).contains("8"));
    assert_eq!(fs::read(repo.root().join(".axon/axon.db")).unwrap(), before);
    assert_eq!(calls(&repo), 3);
}

#[test]
fn kind_filters_skip_unrelated_commands_but_still_observe_ancestor_gates() {
    for query in ["ready", "triage"] {
        for kind in ["issue", "group"] {
            let repo = TestRepo::new();
            repo.init("test");
            let (issue, group) = if query == "ready" {
                (repo.plan("issue"), repo.group_plan("group"))
            } else {
                (repo.capture("issue"), repo.group_capture("group"))
            };
            let (wanted, excluded) = if kind == "issue" {
                (&issue, &group)
            } else {
                (&group, &issue)
            };
            assert_success(&repo.axon(&[
                "when",
                "command",
                excluded,
                "echo call >> calls; exit 7",
            ]));
            let output = repo.axon(&[query, "--kind", kind]);
            assert_success(&output);
            assert!(stdout(&output).contains(wanted));
            assert!(!stdout(&output).contains(excluded));
            assert_eq!(calls(&repo), 0);
        }
    }
    let repo = TestRepo::new();
    repo.init("test");
    let group = repo.group_plan("ancestor");
    let child = repo.plan("child");
    repo.set_parent(&child, &group);
    assert_success(&repo.axon(&["start", &group]));
    assert_success(&repo.axon(&["when", "command", &group, "echo call >> calls; exit 7"]));
    let output = repo.axon(&["ready", "--kind", "issue"]);
    assert_failure(&output);
    assert!(stderr(&output).contains(&group));
    assert_eq!(calls(&repo), 1);
}

#[test]
fn skipped_reads_never_execute_commands_across_the_graph_and_preserve_records() {
    let repo = TestRepo::new();
    repo.init("test");
    let group = repo.group_plan("external gate");
    let child = repo.capture("saved child");
    assert_success(&repo.axon(&["group", "set", &child, &group]));
    assert_success(&repo.axon(&["write", &child, "-m", "saved declaration body"]));
    repo.accept(&child);
    let prerequisite = repo.capture("related prerequisite");
    let root = repo.plan("prerequisite root");
    assert_success(&repo.axon(&["dep", "add", &prerequisite, "--needs", &root]));
    repo.accept(&prerequisite);
    let dependent = repo.capture("dependent");
    assert_success(&repo.axon(&["dep", "add", &dependent, "--needs", &prerequisite]));
    repo.accept(&dependent);
    assert_success(&repo.axon(&["start", &group]));
    assert_success(&repo.axon(&["start", &child]));
    assert_success(&repo.axon(&["release", &child, "-r", "saved release reason"]));
    assert_success(&repo.axon(&["start", &child]));
    let ended = repo.plan("ended with condition");
    assert_success(&repo.axon(&["start", &ended]));
    assert_success(&repo.axon(&["done", &ended]));
    let rejected = repo.plan("rejected with condition");
    assert_success(&repo.axon(&["decide", "reject", &rejected, "-r", "not adopted"]));
    let note = repo.axon(&["note", "add", &child, "-m", "saved supplemental note"]);
    assert_success(&note);
    let ids = [
        &group,
        &child,
        &prerequisite,
        &root,
        &dependent,
        &ended,
        &rejected,
    ];
    for id in ids {
        assert_success(&repo.axon(&["when", "command", id, "echo called >> calls; exit 23"]));
    }
    let before = ids.map(|id| repo.snapshot(id));
    for trace in [false, true] {
        let mut args = vec!["list", "--skip-command-evaluation"];
        if trace {
            args.push("--trace-conditions");
        }
        let list = repo.axon(&args);
        assert_success(&list);
        assert!(stderr(&list).is_empty());
        for id in ids {
            assert!(stdout(&list).contains(id));
        }
        for id in ids {
            let mut args = vec!["show", id, "--skip-command-evaluation"];
            if trace {
                args.push("--trace-conditions");
            }
            let shown = repo.axon(&args);
            assert_success(&shown);
            assert!(stderr(&shown).is_empty());
            assert!(stdout(&shown).contains("Surfaced: unevaluated"));
            assert!(stdout(&shown).contains("Command("));
            assert!(stdout(&shown).contains("Plan declaration: fixed at Revision"));
        }
    }
    let shown = stdout(&repo.axon(&["show", &child, "--skip-command-evaluation"]));
    for expected in [
        "Active scope: unevaluated",
        "Claim:",
        "Worktree:",
        "saved declaration body",
        "saved supplemental note",
        "saved release reason",
        "Progress history",
        &group,
    ] {
        assert!(shown.contains(expected), "{expected}: {shown}");
    }
    let shown = stdout(&repo.axon(&["show", &dependent, "--skip-command-evaluation"]));
    assert!(shown.contains("Root causes: unevaluated"), "{shown}");
    assert!(shown.contains("Blocked: yes"));
    assert!(shown.contains("Ready: no"));
    let listed = repo.axon(&["list", "--kind", "group", "--skip-command-evaluation"]);
    assert_success(&listed);
    assert!(stdout(&listed).contains(&group));
    assert!(!stdout(&listed).contains(&child));
    assert_eq!(calls(&repo), 0);
    assert_eq!(ids.map(|id| repo.snapshot(id)), before);
    let failed = repo.axon(&["show", &child]);
    assert_failure(&failed);
    assert!(stderr(&failed).contains("--skip-command-evaluation"));
    assert_eq!(calls(&repo), 1);
}

#[test]
fn skipped_observations_keep_known_conditions_and_closed_gates_definitive() {
    let repo = TestRepo::new();
    repo.init("test");
    let group = repo.group_plan("unknown gate");
    let child = repo.capture("child");
    assert_success(&repo.axon(&["group", "set", &child, &group]));
    repo.accept(&child);
    assert_success(&repo.axon(&["start", &group]));
    assert_success(&repo.axon(&["when", "command", &group, "echo called >> calls; exit 23"]));
    let shown = stdout(&repo.axon(&["show", &child, "--skip-command-evaluation"]));
    assert!(shown.contains("Active scope: unevaluated"), "{shown}");
    assert!(shown.contains("Surfaced: yes"));
    assert!(shown.contains("Ready: unevaluated"));
    assert_success(&repo.axon(&["when", "manual", &child]));
    let shown = stdout(&repo.axon(&["show", &child, "--skip-command-evaluation"]));
    assert!(shown.contains("Surfaced: no"));
    assert!(shown.contains("Ready: no"));
    assert_success(&repo.axon(&["release", &group]));
    assert_success(&repo.axon(&["when", "command", &child, "echo called >> calls; exit 23"]));
    let shown = stdout(&repo.axon(&["show", &child, "--skip-command-evaluation"]));
    assert!(shown.contains("Active scope: no"), "{shown}");
    assert!(shown.contains("Ready: no"));
    assert!(shown.contains("Surfaced: unevaluated"));
    let date = repo.plan("date condition");
    for (value, expected) in [
        ("2000-01-01T00:00:00Z", "yes"),
        ("2999-01-01T00:00:00Z", "no"),
    ] {
        assert_success(&repo.axon(&["when", "at", &date, value]));
        let shown = repo.axon(&["show", &date, "--skip-command-evaluation"]);
        assert_success(&shown);
        assert!(stdout(&shown).contains(&format!("Surfaced: {expected}")));
    }
    let waiter = repo.plan("after condition");
    assert_success(&repo.axon(&["when", "after", &waiter, &date]));
    let shown = stdout(&repo.axon(&["show", &waiter, "--skip-command-evaluation"]));
    assert!(shown.contains("Surfaced: no"));
    assert_success(&repo.axon(&["decide", "reject", &date, "-r", "finished deciding"]));
    let shown = stdout(&repo.axon(&["show", &waiter, "--skip-command-evaluation"]));
    assert!(shown.contains("Surfaced: yes"));
    assert!(shown.contains("Ready: yes"));
    assert_eq!(calls(&repo), 0);
}
