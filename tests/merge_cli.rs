mod common;
use common::{TestDir, assert_failure, assert_success, stderr, stdout};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};
fn run(d: &TestDir, root: &Path, args: &[&str]) -> String {
    let o = d.axon_in(root, args);
    assert_success(&o);
    stdout(&o)
}
fn git(root: &Path, args: &[&str]) -> Output {
    let mut c = Command::new("git");
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            c.env_remove(key);
        }
    }
    c.current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.excludesFile=/dev/null",
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
        ])
        .args(args)
        .output()
        .unwrap()
}
fn g(root: &Path, args: &[&str]) {
    assert_success(&git(root, args));
}
fn register_driver(root: &Path) {
    let executable = env!("CARGO_BIN_EXE_axon").replace('\'', "'\\''");
    g(
        root,
        &[
            "config",
            "merge.axon.driver",
            &format!("'{executable}' merge driver %O %A %B"),
        ],
    );
    g(root, &["config", "merge.axon.recursive", "binary"]);
}
fn state(root: &Path) -> PathBuf {
    root.join(".axon/state.jsonl")
}
fn init(d: &TestDir) -> (String, String) {
    run(d, d.path(), &["init", "--backend", "file", "t"]);
    let id = run(d, d.path(), &["capture", "task"])
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    let other = run(d, d.path(), &["capture", "other"])
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    (id, other)
}
fn prep(d: &TestDir, w: &str) -> Output {
    d.axon_in(
        d.path(),
        &[
            "merge",
            "prepare",
            "--base",
            "base",
            "--ours",
            "ours",
            "--theirs",
            "theirs",
            "--output",
            ".axon/state.jsonl",
            "--workspace",
            w,
        ],
    )
}
fn fixtures(d: &TestDir, conflict: bool) -> String {
    let (id, other) = init(d);
    let base = fs::read(state(d.path())).unwrap();
    fs::write(d.path().join("base"), &base).unwrap();
    run(d, d.path(), &["write", &id, "--title", "ours title"]);
    fs::copy(state(d.path()), d.path().join("ours")).unwrap();
    fs::write(state(d.path()), base).unwrap();
    run(
        d,
        d.path(),
        &[
            "write",
            if conflict { &id } else { &other },
            "--title",
            "theirs title",
        ],
    );
    fs::copy(state(d.path()), d.path().join("theirs")).unwrap();
    id
}
fn resolve(d: &TestDir, w: &str, id: &str) {
    let choices: Value =
        serde_json::from_slice(&fs::read(d.path().join(w).join("choices.json")).unwrap()).unwrap();
    let choice = choices
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["owner"] == id)
        .unwrap();
    let manifest: Value =
        serde_json::from_slice(&fs::read(d.path().join(w).join("manifest.json")).unwrap()).unwrap();
    fs::write(d.path().join(w).join("resolution.json"),serde_json::to_vec(&json!({"choices":[{"conflict":choice["id"],"input":manifest["inputs"][1]["digest"]}],"repairs":[]})).unwrap()).unwrap();
}
#[test]
fn explicit_conflict_marker_resolution_and_checked_drift() {
    let d = TestDir::new("merge-resolve");
    let id = fixtures(&d, true);
    fs::write(
        state(d.path()),
        b"<<<<<<< marker\n=======\n>>>>>>> marker\n",
    )
    .unwrap();
    let original = fs::read(state(d.path())).unwrap();
    assert_failure(&prep(&d, "work"));
    assert_eq!(original, fs::read(state(d.path())).unwrap());
    resolve(&d, "work", &id);
    run(&d, d.path(), &["merge", "check", "work"]);
    let candidate = fs::read(d.path().join("work/candidate.jsonl")).unwrap();
    fs::write(d.path().join("work/candidate.jsonl"), b"invalid").unwrap();
    assert!(stderr(&d.axon_in(d.path(), &["merge", "apply", "work"])).contains("drift"));
    fs::write(d.path().join("work/candidate.jsonl"), &candidate).unwrap();
    run(&d, d.path(), &["merge", "apply", "work"]);
    run(&d, d.path(), &["merge", "apply", "work"]);
    assert_eq!(candidate, fs::read(state(d.path())).unwrap());
    assert!(run(&d, d.path(), &["show", &id]).contains("ours title"));
    assert!(run(&d, d.path(), &["storage", "check", ".axon/state.jsonl"]).contains("Valid"));
}
#[test]
fn source_context_and_resolution_drifts_preserve_destination() {
    for file in [
        "base",
        "work/base.jsonl",
        "work/context.json",
        "work/resolution.json",
    ] {
        let d = TestDir::new("merge-drift");
        fixtures(&d, false);
        assert_success(&prep(&d, "work"));
        let before = fs::read(state(d.path())).unwrap();
        let path = d.path().join(file);
        let mut bytes = fs::read(&path).unwrap();
        bytes.push(b' ');
        fs::write(path, bytes).unwrap();
        assert_failure(&d.axon_in(d.path(), &["merge", "apply", "work"]));
        assert_eq!(before, fs::read(state(d.path())).unwrap());
    }
}

#[test]
fn active_merge_rejects_an_unrelated_store_and_backend_drift() {
    let d = TestDir::new("merge-binding");
    fixtures(&d, false);
    let other = TestDir::new("merge-other-store");
    init(&other);
    let before = fs::read(state(d.path())).unwrap();
    let unrelated = fs::read(state(other.path())).unwrap();
    fs::write(state(d.path()), &unrelated).unwrap();
    assert_failure(&prep(&d, "unrelated"));
    assert_eq!(fs::read(state(d.path())).unwrap(), unrelated);
    fs::write(state(d.path()), &before).unwrap();
    assert_success(&prep(&d, "bound"));
    fs::write(d.path().join(".axon/axon.db"), b"another backend").unwrap();
    assert_failure(&d.axon_in(d.path(), &["merge", "apply", "bound"]));
    assert_eq!(fs::read(state(d.path())).unwrap(), before);
}
#[test]
fn cycle_requires_regular_repair_and_retains_stable_records() {
    let d = TestDir::new("merge-cycle");
    let (a, b) = init(&d);
    let base = fs::read(state(d.path())).unwrap();
    fs::write(d.path().join("base"), &base).unwrap();
    run(&d, d.path(), &["dep", "add", &a, "--needs", &b]);
    fs::copy(state(d.path()), d.path().join("ours")).unwrap();
    fs::write(state(d.path()), base).unwrap();
    run(&d, d.path(), &["dep", "add", &b, "--needs", &a]);
    fs::copy(state(d.path()), d.path().join("theirs")).unwrap();
    assert_failure(&prep(&d, "work"));
    fs::write(d.path().join("work/resolution.json"),serde_json::to_vec(&json!({"choices":[],"repairs":[{"operation":{"op":"dependency","source":b,"target":a,"present":false},"reason":"remove cycle"}]})).unwrap()).unwrap();
    run(&d, d.path(), &["merge", "check", "work"]);
    let candidate = fs::read(d.path().join("work/candidate.jsonl")).unwrap();
    run(&d, d.path(), &["merge", "check", "work"]);
    assert_eq!(
        candidate,
        fs::read(d.path().join("work/candidate.jsonl")).unwrap()
    );
    run(&d, d.path(), &["merge", "apply", "work"]);
}
fn commit(root: &Path) {
    g(
        root,
        &[
            "add",
            ".axon/state.jsonl",
            ".gitattributes",
            ".axon/.gitignore",
        ],
    );
    g(root, &["commit", "-qm", "state"]);
}
#[test]
fn real_git_worktree_changes_propagate_only_on_explicit_merge() {
    let d = TestDir::new("merge-git");
    g(d.path(), &["init", "-q", "-b", "main"]);
    let (a, b) = init(&d);
    register_driver(d.path());
    g(d.path(), &["add", ".gitattributes"]);
    commit(d.path());
    let initial = fs::read(state(d.path())).unwrap();
    let work = d.path().join("branch-work");
    g(
        d.path(),
        &["worktree", "add", "-qb", "feature", work.to_str().unwrap()],
    );
    run(&d, &work, &["write", &a, "--title", "feature title"]);
    assert_eq!(initial, fs::read(state(d.path())).unwrap());
    commit(&work);
    run(&d, d.path(), &["write", &b, "--title", "main title"]);
    commit(d.path());
    g(d.path(), &["merge", "--no-edit", "feature"]);
    assert!(run(&d, d.path(), &["show", &a]).contains("feature title"));
    assert!(run(&d, d.path(), &["show", &b]).contains("main title"));
}
#[test]
fn git_conflicts_preserve_driver_inputs_and_resolve_without_driver() {
    for driver in [true, false] {
        let d = TestDir::new("merge-git-conflict");
        g(d.path(), &["init", "-q", "-b", "main"]);
        let (a, _) = init(&d);
        if driver {
            register_driver(d.path());
            g(d.path(), &["add", ".gitattributes"]);
        }
        commit(d.path());
        fs::copy(state(d.path()), d.path().join("base")).unwrap();
        g(d.path(), &["checkout", "-qb", "feature"]);
        run(&d, d.path(), &["write", &a, "--title", "feature title"]);
        fs::copy(state(d.path()), d.path().join("theirs")).unwrap();
        commit(d.path());
        g(d.path(), &["checkout", "-q", "main"]);
        run(&d, d.path(), &["write", &a, "--title", "main title"]);
        fs::copy(state(d.path()), d.path().join("ours")).unwrap();
        commit(d.path());
        assert_failure(&git(d.path(), &["merge", "--no-edit", "feature"]));
        assert_failure(&d.axon_in(d.path(), &["list"]));
        if driver {
            let w = fs::read_dir(d.path().join(".axon/merge"))
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path();
            assert_eq!(
                fs::read(w.join("base.jsonl")).unwrap(),
                fs::read(d.path().join("base")).unwrap()
            );
        }
        assert_failure(&prep(&d, "manual"));
        resolve(&d, "manual", &a);
        run(&d, d.path(), &["merge", "check", "manual"]);
        run(&d, d.path(), &["merge", "apply", "manual"]);
        assert_failure(&d.axon_in(d.path(), &["list"]));
        g(d.path(), &["add", ".axon/state.jsonl"]);
        assert!(run(&d, d.path(), &["show", &a]).contains("main title"));
    }
}
#[test]
fn driver_rejects_empty_ancestor_without_discovering_backend() {
    let d = TestDir::new("merge-empty");
    fixtures(&d, false);
    fs::write(d.path().join("base"), b"").unwrap();
    fs::write(d.path().join(".axon/axon.db"), b"broken").unwrap();
    assert_failure(&d.axon_in(d.path(), &["merge", "driver", "base", "ours", "theirs"]));
    assert!(
        fs::read_to_string(d.path().join("ours"))
            .unwrap()
            .contains("<<<<<<<")
    );
    let w = fs::read_dir(d.path().join(".axon/merge"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(fs::read(w.join("base.jsonl")).unwrap(), b"");
    assert!(w.join("theirs.jsonl").exists());
}

#[test]
fn merge_prepare_rejects_a_v13_common_ancestor_without_rewriting_inputs() {
    let d = TestDir::new("merge-old-schema");
    fixtures(&d, false);
    let legacy = fs::read_to_string(d.path().join("base"))
        .unwrap()
        .replace("\"schema\":14", "\"schema\":13");
    fs::write(d.path().join("base"), &legacy).unwrap();
    let ours = fs::read(d.path().join("ours")).unwrap();
    let theirs = fs::read(d.path().join("theirs")).unwrap();

    let result = prep(&d, "old-schema");
    assert_failure(&result);
    assert!(stderr(&result).contains("unsupported snapshot header"));
    assert_eq!(fs::read_to_string(d.path().join("base")).unwrap(), legacy);
    assert_eq!(fs::read(d.path().join("ours")).unwrap(), ours);
    assert_eq!(fs::read(d.path().join("theirs")).unwrap(), theirs);
}

#[test]
fn git_add_add_and_delete_modify_are_explicit_conflicts() {
    for add_add in [true, false] {
        let d = TestDir::new("merge-git-edge");
        g(d.path(), &["init", "-q", "-b", "main"]);
        let (a, _) = init(&d);
        register_driver(d.path());
        g(d.path(), &["add", ".gitattributes", ".axon/.gitignore"]);
        if !add_add {
            g(d.path(), &["add", ".axon/state.jsonl"]);
        }
        g(d.path(), &["commit", "-qm", "initial"]);
        let original = fs::read(state(d.path())).unwrap();
        g(d.path(), &["checkout", "-qb", "feature"]);
        run(&d, d.path(), &["write", &a, "--title", "feature title"]);
        commit(d.path());
        g(d.path(), &["checkout", "-q", "main"]);
        if add_add {
            fs::write(state(d.path()), &original).unwrap();
            run(&d, d.path(), &["write", &a, "--title", "main title"]);
            commit(d.path());
        } else {
            g(d.path(), &["rm", ".axon/state.jsonl"]);
            g(d.path(), &["commit", "-qm", "remove file"]);
        }
        assert_failure(&git(d.path(), &["merge", "--no-edit", "feature"]));
        assert!(stderr(&d.axon_in(d.path(), &["list"])).contains("unmerged"));
    }
}
#[test]
fn multiple_merge_bases_do_not_guess_an_ancestor() {
    let d = TestDir::new("merge-git-bases");
    g(d.path(), &["init", "-q", "-b", "main"]);
    let (a, b) = init(&d);
    register_driver(d.path());
    g(d.path(), &["add", ".gitattributes"]);
    commit(d.path());
    g(d.path(), &["branch", "right"]);
    run(&d, d.path(), &["write", &a, "--title", "left"]);
    commit(d.path());
    let left = stdout(&git(d.path(), &["rev-parse", "HEAD"]))
        .trim()
        .to_string();
    g(d.path(), &["checkout", "-q", "right"]);
    run(&d, d.path(), &["write", &b, "--title", "right"]);
    commit(d.path());
    let right = stdout(&git(d.path(), &["rev-parse", "HEAD"]))
        .trim()
        .to_string();
    g(d.path(), &["merge", "--no-edit", &left]);
    run(&d, d.path(), &["note", "add", &a, "-m", "right note"]);
    commit(d.path());
    g(d.path(), &["checkout", "-q", "main"]);
    g(d.path(), &["merge", "--no-edit", &right]);
    run(&d, d.path(), &["note", "add", &b, "-m", "left note"]);
    commit(d.path());
    assert_eq!(
        stdout(&git(d.path(), &["merge-base", "--all", "main", "right"]))
            .lines()
            .count(),
        2
    );
    let merged = git(d.path(), &["merge", "--no-edit", "right"]);
    if merged.status.success() {
        run(&d, d.path(), &["storage", "check", ".axon/state.jsonl"]);
    } else {
        assert!(stderr(&d.axon_in(d.path(), &["list"])).contains("unmerged"));
        assert!(
            fs::read_to_string(state(d.path()))
                .unwrap()
                .contains("<<<<<<<")
        );
    }
}

#[test]
fn driver_preserves_every_available_original_on_missing_input() {
    for missing in ["base", "ours", "theirs"] {
        let d = TestDir::new("merge-missing");
        fixtures(&d, false);
        let ours = fs::read(d.path().join("ours")).unwrap();
        let theirs = fs::read(d.path().join("theirs")).unwrap();
        fs::remove_file(d.path().join(missing)).unwrap();
        assert_failure(&d.axon_in(d.path(), &["merge", "driver", "base", "ours", "theirs"]));
        let w = fs::read_dir(d.path().join(".axon/merge"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        if missing != "ours" {
            assert_eq!(fs::read(w.join("ours.jsonl")).unwrap(), ours);
        } else {
            assert!(!d.path().join("ours").exists());
        }
        if missing != "theirs" {
            assert_eq!(fs::read(w.join("theirs.jsonl")).unwrap(), theirs);
        }
    }
}
#[cfg(unix)]
#[test]
fn driver_marker_write_failure_retains_original_failure_and_workspace() {
    use std::os::unix::fs::PermissionsExt;
    let d = TestDir::new("merge-marker-write-fault");
    fixtures(&d, false);
    fs::remove_file(d.path().join("base")).unwrap();
    let directory = d.path().join("readonly");
    fs::create_dir(&directory).unwrap();
    fs::rename(d.path().join("ours"), directory.join("ours")).unwrap();
    let ours = fs::read(directory.join("ours")).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o555)).unwrap();
    let output = d.axon_in(
        d.path(),
        &["merge", "driver", "base", "readonly/ours", "theirs"],
    );
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let workspace = fs::read_dir(d.path().join(".axon/merge"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let diagnostic = stderr(&output);
    assert!(diagnostic.contains("input preservation failed"));
    assert!(diagnostic.contains("Applied: workspace directory created at"));
    assert!(diagnostic.contains(workspace.to_str().unwrap()));
    assert!(diagnostic.contains("Conflict marker handling failed:"));
    assert!(diagnostic.contains("Not applied: file replacement at readonly/ours"));
    assert_eq!(diagnostic.matches("Help:").count(), 1);
    assert!(diagnostic.lines().last().unwrap().starts_with("Help:"));
    assert_eq!(fs::read(directory.join("ours")).unwrap(), ours);
    assert_eq!(fs::read(workspace.join("ours.jsonl")).unwrap(), ours);
    assert_eq!(
        fs::read(workspace.join("theirs.jsonl")).unwrap(),
        fs::read(d.path().join("theirs")).unwrap()
    );
}

#[test]
fn repair_evaluates_conditions_at_management_root_from_subdirectory() {
    let d = TestDir::new("merge-context");
    let (a, _) = init(&d);
    run(&d, d.path(), &["decide", "accept", &a]);
    run(&d, d.path(), &["when", "command", &a, "test -f ready-flag"]);
    fs::write(d.path().join("ready-flag"), b"").unwrap();
    for name in ["base", "ours", "theirs"] {
        fs::copy(state(d.path()), d.path().join(name)).unwrap();
    }
    let sub = d.path().join("sub");
    fs::create_dir(&sub).unwrap();
    assert!(run(&d, &sub, &["ready"]).contains(&a));
    run(
        &d,
        &sub,
        &[
            "merge",
            "prepare",
            "--base",
            "../base",
            "--ours",
            "../ours",
            "--theirs",
            "../theirs",
            "--output",
            "../.axon/state.jsonl",
            "--workspace",
            "../work",
        ],
    );
    let context: Value =
        serde_json::from_slice(&fs::read(d.path().join("work/context.json")).unwrap()).unwrap();
    assert_eq!(context["root"], d.path().to_str().unwrap());
    fs::write(
        d.path().join("work/resolution.json"),
        serde_json::to_vec(
            &json!({"choices":[],"repairs":[{"operation":{"op":"start","owner":a},"reason":null}]}),
        )
        .unwrap(),
    )
    .unwrap();
    run(&d, &sub, &["merge", "check", "../work"]);
    run(&d, &sub, &["merge", "apply", "../work"]);
    assert!(run(&d, &sub, &["show", &a]).contains("InProgress"));
}
#[test]
fn deleted_original_is_reported_as_input_drift() {
    let d = TestDir::new("merge-deleted-original");
    fixtures(&d, false);
    assert_success(&prep(&d, "work"));
    fs::remove_file(d.path().join("base")).unwrap();
    assert_failure(&d.axon_in(d.path(), &["merge", "check", "work"]));
    let report: Value =
        serde_json::from_slice(&fs::read(d.path().join("work/report.json")).unwrap()).unwrap();
    assert_eq!(report["status"], "input_drift");
}

#[test]
fn check_report_failure_retains_the_input_drift_cause() {
    let d = TestDir::new("merge-report-fault");
    fixtures(&d, false);
    assert_success(&prep(&d, "work"));
    let before = fs::read(state(d.path())).unwrap();
    fs::remove_file(d.path().join("work/base.jsonl")).unwrap();
    fs::remove_file(d.path().join("work/report.json")).unwrap();
    fs::create_dir(d.path().join("work/report.json")).unwrap();
    let output = d.axon_in(d.path(), &["merge", "check", "work"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let diagnostic = stderr(&output);
    assert!(diagnostic.contains("input drift"));
    assert!(diagnostic.contains("base.jsonl"));
    assert!(diagnostic.contains("Error report update failed:"));
    assert!(diagnostic.contains("work/report.json publish:"));
    assert!(diagnostic.contains("Not applied: file replacement at work/report.json"));
    assert_eq!(diagnostic.matches("Help:").count(), 1);
    assert!(diagnostic.lines().last().unwrap().starts_with("Help:"));
    assert_eq!(fs::read(state(d.path())).unwrap(), before);
    assert!(d.path().join("work/report.json").is_dir());
    assert!(!d.path().join("work/checked.json").exists());
}

#[test]
fn conflict_report_failure_retains_the_unresolved_result() {
    let d = TestDir::new("merge-conflict-report-fault");
    fixtures(&d, true);
    assert_failure(&prep(&d, "work"));
    let before = fs::read(state(d.path())).unwrap();
    fs::remove_file(d.path().join("work/report.json")).unwrap();
    fs::create_dir(d.path().join("work/report.json")).unwrap();
    let output = d.axon_in(d.path(), &["merge", "check", "work"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let diagnostic = stderr(&output);
    assert!(diagnostic.contains("unresolved conflicts"));
    assert!(diagnostic.contains("Conflict report update failed:"));
    assert!(diagnostic.contains("Error report update failed:"));
    assert!(diagnostic.contains("Not applied: file replacement at work/report.json"));
    assert_eq!(diagnostic.matches("Help:").count(), 1);
    assert!(diagnostic.lines().last().unwrap().starts_with("Help:"));
    assert_eq!(fs::read(state(d.path())).unwrap(), before);
    assert!(!d.path().join("work/checked.json").exists());
}

#[test]
fn destination_symlinks_cannot_bypass_store_binding() {
    use std::os::unix::fs::symlink;
    for directory_link in [false, true] {
        let d = TestDir::new("merge-output-symlink");
        fixtures(&d, false);
        let before = fs::read(state(d.path())).unwrap();
        if directory_link {
            fs::rename(d.path().join(".axon"), d.path().join("storage")).unwrap();
            symlink("storage", d.path().join(".axon")).unwrap();
        } else {
            fs::rename(state(d.path()), d.path().join("stored.jsonl")).unwrap();
            symlink("../stored.jsonl", state(d.path())).unwrap();
        }
        let result = prep(&d, "work");
        assert_failure(&result);
        assert!(stderr(&result).contains("symlink"));
        assert_eq!(fs::read(state(d.path())).unwrap(), before);
        assert!(d.path().join("work/ours.jsonl").exists());
        assert_failure(&d.axon_in(d.path(), &["merge", "apply", "work"]));
    }
}
#[test]
fn relative_driver_output_preserves_original_conflict_diagnostic() {
    let d = TestDir::new("merge-relative-driver");
    fixtures(&d, true);
    let output = d.axon_in(d.path(), &["merge", "driver", "base", "ours", "theirs"]);
    assert_failure(&output);
    assert!(stderr(&output).contains("unresolved conflicts"));
    assert!(!stderr(&output).contains("No such file or directory"));
    let diagnostic = stderr(&output);
    assert_eq!(diagnostic.matches("Help:").count(), 1);
    assert!(
        diagnostic.find("Applied: conflict marker").unwrap() < diagnostic.find("Help:").unwrap()
    );
    assert!(
        diagnostic
            .lines()
            .last()
            .unwrap()
            .starts_with("Help: Resolve using preserved inputs")
    );
}
#[test]
fn apply_rejects_destination_symlink_introduced_after_check() {
    use std::os::unix::fs::symlink;
    let d = TestDir::new("merge-output-symlink-drift");
    fixtures(&d, false);
    assert_success(&prep(&d, "work"));
    let before = fs::read(state(d.path())).unwrap();
    fs::rename(state(d.path()), d.path().join("stored.jsonl")).unwrap();
    symlink("../stored.jsonl", state(d.path())).unwrap();
    let output = d.axon_in(d.path(), &["merge", "apply", "work"]);
    assert_failure(&output);
    assert!(stderr(&output).contains("symlink"));
    assert_eq!(fs::read(d.path().join("stored.jsonl")).unwrap(), before);
    assert!(
        fs::symlink_metadata(state(d.path()))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn workspace_artifacts_cannot_be_publication_targets() {
    for artifact in [
        "base.jsonl",
        "ours.jsonl",
        "theirs.jsonl",
        "preimage",
        "manifest.json",
        "manifest.digest",
        "context.json",
        "resolution.json",
        "candidate.jsonl",
        "checked.json",
        "report.json",
        "workspace.lock",
    ] {
        let d = TestDir::new("merge-workspace-output");
        fixtures(&d, false);
        let before = fs::read(state(d.path())).unwrap();
        let result = d.axon_in(
            d.path(),
            &[
                "merge",
                "prepare",
                "--base",
                "base",
                "--ours",
                "ours",
                "--theirs",
                "theirs",
                "--output",
                &format!("work/{artifact}"),
                "--workspace",
                "work",
            ],
        );
        assert_failure(&result);
        assert!(stderr(&result).contains("outside the workspace"));
        for name in ["base", "ours", "theirs"] {
            assert_eq!(
                fs::read(d.path().join(name)).unwrap(),
                fs::read(d.path().join(format!("work/{name}.jsonl"))).unwrap()
            );
        }
        assert_failure(&d.axon_in(d.path(), &["merge", "apply", "work"]));
        assert_eq!(before, fs::read(state(d.path())).unwrap());
    }
}
#[test]
fn workspace_alias_is_rejected_after_relocation() {
    for artifact in [
        "ours.jsonl",
        "report.json",
        "checked.json",
        "resolution.json",
        "context.json",
        "manifest.json",
    ] {
        let d = TestDir::new("merge-workspace-relocated");
        fixtures(&d, false);
        let output = d.path().join("publication");
        fs::create_dir(&output).unwrap();
        fs::copy(d.path().join("ours"), output.join(artifact)).unwrap();
        run(
            &d,
            d.path(),
            &[
                "merge",
                "prepare",
                "--base",
                "base",
                "--ours",
                "ours",
                "--theirs",
                "theirs",
                "--output",
                &format!("publication/{artifact}"),
                "--workspace",
                "work",
            ],
        );
        fs::remove_dir_all(&output).unwrap();
        fs::rename(d.path().join("work"), &output).unwrap();
        let original = fs::read(output.join(artifact)).unwrap();
        let apply = d.axon_in(d.path(), &["merge", "apply", "publication"]);
        assert_failure(&apply);
        assert!(stderr(&apply).contains("outside the workspace"));
        let check = d.axon_in(d.path(), &["merge", "check", "publication"]);
        assert_failure(&check);
        assert!(stderr(&check).contains("outside the workspace"));
        assert_eq!(original, fs::read(output.join(artifact)).unwrap());
    }
}

#[test]
fn merge_setup_is_removed() {
    let d = TestDir::new("removed-setup");
    let output = d.axon_in(d.path(), &["merge", "setup"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("unrecognized subcommand"));
}

#[test]
fn prepare_destination_resolution_failure_reports_preserved_workspace() {
    let d = TestDir::new("merge-prepare-partial");
    init(&d);
    let bytes = fs::read(state(d.path())).unwrap();
    for name in ["base", "ours", "theirs"] {
        fs::write(d.path().join(name), &bytes).unwrap();
    }
    let output = d.axon_in(
        d.path(),
        &[
            "merge",
            "prepare",
            "--base",
            "base",
            "--ours",
            "ours",
            "--theirs",
            "theirs",
            "--output",
            "missing/out",
            "--workspace",
            "partial",
        ],
    );
    assert_eq!(output.status.code(), Some(1));
    let diagnostic = stderr(&output);
    assert!(diagnostic.contains("missing/out resolve destination:"));
    assert!(diagnostic.contains("Applied: workspace directory created at partial"));
    assert!(diagnostic.contains("Not applied: merge candidate publication at missing/out"));
    assert!(diagnostic.contains("use a new workspace path"));
    for name in ["base", "ours", "theirs"] {
        assert_eq!(
            fs::read(d.path().join(format!("partial/{name}.jsonl"))).unwrap(),
            bytes
        );
    }
    assert!(!d.path().join("missing/out").exists());
}
