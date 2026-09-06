mod common;
use common::{TestDir, assert_failure, assert_success, stderr, stdout};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
fn run(dir: &TestDir, root: &Path, args: &[&str]) -> String {
    let out = dir.axon_in(root, args);
    assert_success(&out);
    stdout(&out)
}
fn make(dir: &TestDir, root: &Path) -> String {
    run(dir, root, &["plan", "task"])
        .split_whitespace()
        .next()
        .unwrap()
        .into()
}
fn git_command(root: &Path) -> Command {
    let mut command = Command::new("git");
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    command
        .current_dir(root)
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
        ]);
    command
}
fn git(root: &Path, args: &[&str]) {
    assert_success(&git_command(root).args(args).output().unwrap());
}
fn config(root: &Path) -> PathBuf {
    root.join(".axon/config.json")
}
fn state(root: &Path) -> PathBuf {
    root.join(".axon/state.jsonl")
}
#[test]
fn all_normal_commands_and_import_use_file_without_sqlite() {
    let dir = TestDir::new("file-commands");
    let root = dir.path();
    run(&dir, root, &["init", "--backend", "file", "t"]);
    let group = run(&dir, root, &["group", "plan", "group"])
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    run(&dir, root, &["start", &group]);
    let id = make(&dir, root);
    let other = make(&dir, root);
    for args in [
        vec!["decide", "undecide", &id],
        vec!["write", &id, "--title", "changed"],
        vec!["group", "set", &id, &group],
        vec!["dep", "add", &id, "--needs", &other],
        vec!["dep", "rm", &id, "--needs", &other],
        vec!["decide", "accept", &id],
        vec!["when", "manual", &id],
        vec!["when", "clear", &id],
        vec!["start", &id],
        vec!["release", &id],
        vec!["start", &id],
        vec!["done", &id],
        vec!["note", "add", &id, "-m", "note 日本語"],
    ] {
        run(&dir, root, &args);
    }
    for args in [
        vec!["list"],
        vec!["ready"],
        vec!["triage"],
        vec!["claims"],
        vec!["status"],
        vec!["show", &group],
        vec!["show", &id],
        vec!["log", &id],
        vec!["note", "list", &id],
        vec!["revision", "list", &id],
        vec!["export", &id],
    ] {
        run(&dir, root, &args);
    }
    run(&dir, root, &["show", id.rsplit('-').next().unwrap()]);
    let plan = root.join("plan.yml");
    fs::write(&plan,"schema: axon-plan/v2\nissues:\n  - id: null\n    key: new\n    base: null\n    title: Imported\n    description: null\n    observed:\n      progress: not_started\n      claim: null\n      disposition: accepted\n      resurface:\n        kind: always\ngroups: []\nrelations:\n  editable:\n    parents: []\n    dependencies: []\n  readonly:\n    parents: []\n    dependencies: []\nreferences:\n  entities: []\n").unwrap();
    for command in ["prepare", "check", "apply"] {
        run(&dir, root, &["import", command, plan.to_str().unwrap()]);
    }
    let bytes = fs::read(state(root)).unwrap();
    run(&dir, root, &["import", "apply", plan.to_str().unwrap()]);
    assert_eq!(fs::read(state(root)).unwrap(), bytes);
    assert!(run(&dir, root, &["list"]).contains("Imported"));
    assert!(!root.join(".axon/state.db").exists());
    assert!(!root.join(".axon/axon.db").exists());
}
#[test]
fn parallel_writers_keep_every_note_and_one_start() {
    let dir = TestDir::new("file-writers");
    let root = dir.path();
    run(&dir, root, &["init", "--backend", "file"]);
    let id = make(&dir, root);
    let children: Vec<_> = (0..12)
        .map(|i| {
            dir.axon_command()
                .args(["note", "add", &id, "-m", &format!("note-{i}")])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    for child in children {
        assert_success(&child.wait_with_output().unwrap());
    }
    let notes = run(&dir, root, &["note", "list", &id]);
    for i in 0..12 {
        assert!(notes.contains(&format!("note-{i}")));
    }
    let children: Vec<_> = (0..6)
        .map(|_| {
            dir.axon_command()
                .args(["start", &id])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    assert_eq!(
        children
            .into_iter()
            .map(|child| child.wait_with_output().unwrap())
            .filter(|o| o.status.success())
            .count(),
        1
    );
}
#[test]
fn worktrees_clone_and_shared_sqlite_coexist_without_file_propagation() {
    let dir = TestDir::new("file-worktrees");
    let main = dir.path().join("main");
    fs::create_dir(&main).unwrap();
    dir.init_git(&main);
    run(&dir, &main, &["init", "--backend", "file"]);
    let id = make(&dir, &main);
    git(&main, &["add", ".axon/config.json", ".axon/state.jsonl"]);
    git(&main, &["commit", "-qm", "seed"]);
    let a = dir.path().join("a");
    let b = dir.path().join("b");
    git(&main, &["worktree", "add", "-qb", "a", a.to_str().unwrap()]);
    git(&main, &["worktree", "add", "-qb", "b", b.to_str().unwrap()]);
    let original = fs::read(state(&main)).unwrap();
    run(&dir, &a, &["start", &id]);
    run(&dir, &a, &["note", "add", &id, "-m", "isolated"]);
    run(&dir, &a, &["decide", "undecide", &id]);
    let second = make(&dir, &a);
    run(&dir, &a, &["dep", "add", &id, "--needs", &second]);
    run(&dir, &a, &["decide", "accept", &id]);
    assert_eq!(fs::read(state(&main)).unwrap(), original);
    assert_eq!(fs::read(state(&b)).unwrap(), original);
    run(&dir, &b, &["start", &id]);
    let clone = dir.path().join("clone");
    git(
        dir.path(),
        &[
            "clone",
            "-q",
            main.to_str().unwrap(),
            clone.to_str().unwrap(),
        ],
    );
    run(&dir, &clone, &["show", &id]);
    fs::remove_file(config(&b)).unwrap();
    fs::remove_file(state(&b)).unwrap();
    run(&dir, &b, &["init", "--backend", "sqlite"]);
    let sql_id = make(&dir, &b);
    run(&dir, &main, &["show", &id]);
    assert_eq!(fs::read(state(&main)).unwrap(), original);
    fs::remove_file(config(&clone)).unwrap();
    assert_failure(&dir.axon_in(&clone, &["show", &id]));
    let c = dir.path().join("c");
    git(&main, &["worktree", "add", "-qb", "c", c.to_str().unwrap()]);
    fs::remove_file(config(&c)).unwrap();
    fs::remove_file(state(&c)).unwrap();
    run(&dir, &c, &["init", "--backend", "sqlite"]);
    run(&dir, &c, &["show", &sql_id]);
}
#[test]
fn invalid_missing_partial_and_unmerged_never_fall_back() {
    let dir = TestDir::new("file-invalid");
    let root = dir.path();
    run(&dir, root, &["init", "--backend", "file"]);
    let original = fs::read(state(root)).unwrap();
    let cfg = fs::read(config(root)).unwrap();
    run(&dir, root, &["init"]);
    assert_eq!(fs::read(state(root)).unwrap(), original);
    fs::write(config(root), b"{}").unwrap();
    assert_failure(&dir.axon_in(root, &["list"]));
    assert_failure(&dir.axon_in(root, &["init"]));
    fs::write(config(root), &cfg).unwrap();
    fs::remove_file(state(root)).unwrap();
    assert_failure(&dir.axon_in(root, &["list"]));
    assert_failure(&dir.axon_in(root, &["init"]));
    assert!(!state(root).exists());
    fs::write(state(root), &original).unwrap();
    fs::remove_file(config(root)).unwrap();
    assert_failure(&dir.axon_in(root, &["init", "--backend", "file"]));
    assert_eq!(fs::read(state(root)).unwrap(), original);
    fs::write(config(root), cfg).unwrap();
    dir.init_git(root);
    git(root, &["add", ".axon/config.json", ".axon/state.jsonl"]);
    git(root, &["commit", "-qm", "seed"]);
    let hash = git_command(root)
        .args(["hash-object", ".axon/state.jsonl"])
        .output()
        .unwrap();
    let hash = stdout(&hash).trim().to_string();
    use std::io::Write;
    use std::process::Stdio;
    let mut child = git_command(root)
        .args(["update-index", "--index-info"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(format!("0 {}\t.axon/state.jsonl\n100644 {hash} 1\t.axon/state.jsonl\n100644 {hash} 2\t.axon/state.jsonl\n", "0".repeat(40)).as_bytes()).unwrap();
    assert!(child.wait().unwrap().success());
    let result = dir.axon_in(root, &["list"]);
    assert_failure(&result);
    assert!(stderr(&result).contains("unmerged"));
    assert_eq!(fs::read(state(root)).unwrap(), original);
}

#[test]
fn shared_registration_input_failure_can_be_corrected_without_recovery() {
    let dir = TestDir::new("file-registration-retry");
    let main = dir.path().join("main");
    fs::create_dir(&main).unwrap();
    dir.init_git(&main);
    run(&dir, &main, &["init", "original"]);
    git(&main, &["commit", "--allow-empty", "-qm", "seed"]);
    let other = dir.path().join("other");
    git(
        &main,
        &["worktree", "add", "-qb", "other", other.to_str().unwrap()],
    );
    let before = fs::read(main.join(".git/axon/state.db")).unwrap();
    assert_failure(&dir.axon_in(&other, &["init", "wrong"]));
    assert!(!other.join(".axon/init.pending").exists());
    assert!(!config(&other).exists());
    run(&dir, &other, &["init", "original"]);
    assert_eq!(fs::read(main.join(".git/axon/state.db")).unwrap(), before);
}

#[cfg(unix)]
#[test]
fn dangling_configuration_never_escapes_to_an_ancestor_store() {
    use std::os::unix::fs::symlink;
    let dir = TestDir::new("file-dangling-config");
    let outer = dir.path();
    run(&dir, outer, &["init", "--backend", "file"]);
    let bytes = fs::read(state(outer)).unwrap();
    let inner = outer.join("inner");
    fs::create_dir_all(inner.join(".axon")).unwrap();
    symlink("absent.json", config(&inner)).unwrap();
    assert_failure(&dir.axon_in(&inner, &["plan", "must not reach outer"]));
    assert_failure(&dir.axon_in(&inner, &["init", "--backend", "file"]));
    assert_eq!(fs::read(state(outer)).unwrap(), bytes);
    assert!(!state(&inner).exists());
}

#[test]
fn incomplete_nested_root_does_not_open_outer_config() {
    let dir = TestDir::new("file-partial-boundary");
    let outer = dir.path();
    run(&dir, outer, &["init", "--backend", "file"]);
    let bytes = fs::read(state(outer)).unwrap();
    let inner = outer.join("inner");
    fs::create_dir_all(inner.join(".axon")).unwrap();
    fs::write(state(&inner), &bytes).unwrap();
    assert_failure(&dir.axon_in(&inner, &["plan", "must not reach outer"]));
    assert_failure(&dir.axon_in(&inner, &["init", "--backend", "file"]));
    assert_eq!(fs::read(state(outer)).unwrap(), bytes);
}
