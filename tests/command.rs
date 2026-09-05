mod common;

use common::{TestDir, TestRepo, assert_failure, assert_success, stderr, stdout};
use std::fs;

fn calls(repo: &TestRepo) -> usize {
    fs::read_to_string(repo.root().join("calls"))
        .unwrap_or_default()
        .lines()
        .count()
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
fn evaluation_failure_is_diagnostic_and_start_is_not_written() {
    let repo = TestRepo::new();
    repo.init("test");
    let id = repo.plan("failing wait");
    let script = "echo call >> calls; echo diagnostic-out; echo diagnostic-err >&2; exit 23";
    assert_success(&repo.axon(&["when", "command", &id, script]));
    let before = repo.snapshot(&id);
    for args in [
        vec!["show", &id],
        vec!["list"],
        vec!["ready"],
        vec!["start", &id],
    ] {
        let output = repo.axon(&args);
        assert_failure(&output);
        assert!(stdout(&output).is_empty());
        let error = stderr(&output);
        for expected in [&id, script, "23", "diagnostic-out", "diagnostic-err"] {
            assert!(error.contains(expected), "{error}");
        }
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
    assert_success(&repo.axon(&["when", "at", &id, "2100-01-01"]));
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
    assert_success(&repo.axon(&["import", "check", file]));
    // An Undecided root has no ready or active-scope observation to perform.
    assert_eq!(calls(&repo), 0);
    assert_success(&repo.axon(&["import", "apply", file]));
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
    assert_success(&repo.axon(&["import", "check", file]));
    assert_eq!(calls(&repo), 1);
    assert_success(&repo.axon(&["import", "apply", file]));
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
