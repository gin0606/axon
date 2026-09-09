use crate::common::{TestDir, assert_failure, assert_success, stderr, stdout};
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

fn state(root: &Path) -> PathBuf {
    root.join(".axon/state.jsonl")
}

#[test]
fn ordinary_open_migrates_v13_dates_and_preserves_file_records() {
    let dir = TestDir::new("file-date-upgrade");
    let root = dir.path();
    run(&dir, root, &["init", "--backend", "file", "legacy"]);
    let issue = make(&dir, root);
    run(
        &dir,
        root,
        &["note", "add", &issue, "-m", "AtDate(2026-01-02)"],
    );
    run(&dir, root, &["when", "at", &issue, "2099-01-02T00:00:00Z"]);

    let current = fs::read_to_string(state(root)).unwrap();
    let record_count = current.lines().count();
    let legacy = current
        .replace("\"schema\":14", "\"schema\":13")
        .replace(
            "\"metadata\":{",
            "\"metadata\":{\"AtDate\":{\"Text\":\"kept metadata\"},",
        )
        .replace("2099-01-02T00:00:00Z", "2099-01-02");
    fs::write(state(root), &legacy).unwrap();

    let result = dir.axon_in(root, &["show", &issue, "--skip-command-evaluation"]);
    assert_success(&result);
    assert!(stderr(&result).contains("(1, 13) -> (1, 14)"));
    let updated = fs::read_to_string(state(root)).unwrap();
    assert_eq!(updated.lines().count(), record_count);
    assert!(updated.contains("AtDate(2026-01-02)"));
    assert!(updated.contains("\"AtDate\":{\"Text\":\"kept metadata\"}"));
    assert!(updated.contains("2099-01-02T00:00:00Z"));
    assert!(!updated.contains("\"AtDate\":\"2099-01-02\""));
    let backups = fs::read_dir(root.join(".axon/migration-backups"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(backups.len(), 1);
    assert_eq!(fs::read_to_string(&backups[0]).unwrap(), legacy);
    assert_success(&dir.axon_in(root, &["storage", "check", state(root).to_str().unwrap()]));
}

#[test]
fn v13_file_with_non_date_at_date_payload_is_rejected_without_writing() {
    let dir = TestDir::new("file-invalid-v13-date");
    let root = dir.path();
    run(&dir, root, &["init", "--backend", "file", "legacy"]);
    let issue = make(&dir, root);
    run(&dir, root, &["when", "at", &issue, "2099-01-02T00:00:00Z"]);

    let invalid = fs::read_to_string(state(root))
        .unwrap()
        .replace("\"schema\":14", "\"schema\":13");
    fs::write(state(root), &invalid).unwrap();

    let result = dir.axon_in(root, &["show", &issue, "--skip-command-evaluation"]);
    assert_failure(&result);
    assert!(stderr(&result).contains("invalid v13 AtDate payload"));
    assert_eq!(fs::read_to_string(state(root)).unwrap(), invalid);
    assert!(!root.join(".axon/migration-backups").exists());
}

#[test]
fn file_init_integrates_git_files_without_config_or_registration() {
    for git_repo in [false, true] {
        let dir = TestDir::new("backend-ignore");
        let root = dir.path();
        if git_repo {
            dir.init_git(root);
        }
        fs::create_dir(root.join(".axon")).unwrap();
        fs::write(root.join(".axon/.gitignore"), "# user rules\n").unwrap();
        fs::write(root.join(".gitattributes"), "*.txt text\n").unwrap();
        run(&dir, root, &["init", "--backend", "file", "t"]);
        assert_eq!(
            fs::read_to_string(root.join(".axon/.gitignore")).unwrap(),
            "# user rules\n*\n!.gitignore\n!state.jsonl\n"
        );
        assert_eq!(
            fs::read_to_string(root.join(".gitattributes")).unwrap(),
            "*.txt text\n/.axon/state.jsonl merge=axon\n"
        );
        assert!(!root.join(".axon/config.json").exists());
        let before = fs::read(state(root)).unwrap();
        assert_failure(&dir.axon_in(root, &["init", "--backend", "file"]));
        assert_eq!(fs::read(state(root)).unwrap(), before);
        if git_repo {
            fs::write(root.join(".axon/private"), "private").unwrap();
            git(root, &["add", ".axon"]);
            let out = git_command(root)
                .args(["ls-files", ".axon"])
                .output()
                .unwrap();
            assert_success(&out);
            assert_eq!(stdout(&out), ".axon/.gitignore\n.axon/state.jsonl\n");
            assert!(
                !git_command(root)
                    .args(["config", "--get", "merge.axon.driver"])
                    .output()
                    .unwrap()
                    .status
                    .success()
            );
        }
    }
}
#[test]
fn sqlite_init_does_not_change_git_integration() {
    for git_repo in [false, true] {
        let dir = TestDir::new("sqlite-ignore");
        let root = dir.path();
        if git_repo {
            dir.init_git(root);
        }
        let exclude = root.join(".git/info/exclude");
        let before = git_repo.then(|| fs::read(&exclude).unwrap());
        run(&dir, root, &["init", "t"]);
        assert!(root.join(".axon/axon.db").exists());
        for name in [".axon/.gitignore", ".axon/config.json", ".gitattributes"] {
            assert!(!root.join(name).exists());
        }
        if let Some(before) = before {
            assert_eq!(fs::read(exclude).unwrap(), before);
        }
    }
}
#[test]
fn outer_ignore_policy_is_not_changed_or_rejected() {
    let dir = TestDir::new("external-ignore");
    dir.init_git(dir.path());
    fs::write(dir.path().join(".gitignore"), ".axon/\n").unwrap();
    run(&dir, dir.path(), &["init", "--backend", "file"]);
    assert_eq!(
        fs::read(dir.path().join(".gitignore")).unwrap(),
        b".axon/\n"
    );
    make(&dir, dir.path());
}

#[test]
fn integration_conflicts_preserve_state_rules_and_pending_marker() {
    for (name, content) in [
        (".axon/.gitignore", "state.jsonl\n"),
        (".axon/.gitignore", "!state.jsonl\n*\n"),
        (".gitattributes", "/.axon/state.jsonl merge=other\n"),
    ] {
        let dir = TestDir::new("integration-conflict");
        fs::create_dir(dir.path().join(".axon")).unwrap();
        let path = dir.path().join(name);
        fs::write(&path, content).unwrap();
        let out = dir.axon_in(dir.path(), &["init", "--backend", "file"]);
        assert_failure(&out);
        assert!(stderr(&out).contains("conflicting rule"));
        assert!(stderr(&out).contains(name));
        assert_eq!(fs::read(&path).unwrap(), content.as_bytes());
        assert!(state(dir.path()).exists());
        assert!(dir.path().join(".axon/init.pending").exists());
        let before = fs::read(state(dir.path())).unwrap();
        assert_failure(&dir.axon_in(dir.path(), &["list"]));
        assert_failure(&dir.axon_in(dir.path(), &["init"]));
        assert_eq!(fs::read(state(dir.path())).unwrap(), before);
    }
}

#[test]
fn missing_ignore_wildcard_precedes_existing_exceptions_without_duplicates() {
    let dir = TestDir::new("integration-existing");
    dir.init_git(dir.path());
    fs::create_dir(dir.path().join(".axon")).unwrap();
    fs::write(
        dir.path().join(".axon/.gitignore"),
        "# keep\n!/state.jsonl\n!/.gitignore\n",
    )
    .unwrap();
    let attrs = ".axon/state.jsonl text merge=axon\n";
    fs::write(dir.path().join(".gitattributes"), attrs).unwrap();
    run(&dir, dir.path(), &["init", "--backend", "file"]);
    assert_eq!(
        fs::read_to_string(dir.path().join(".axon/.gitignore")).unwrap(),
        "*\n# keep\n!/state.jsonl\n!/.gitignore\n"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join(".gitattributes")).unwrap(),
        attrs
    );
    git(dir.path(), &["add", ".axon"]);
    let out = git_command(dir.path())
        .args(["ls-files", ".axon"])
        .output()
        .unwrap();
    assert_success(&out);
    assert_eq!(stdout(&out), ".axon/.gitignore\n.axon/state.jsonl\n");
}

#[test]
fn simultaneous_init_has_one_winner_across_backends() {
    use std::process::Stdio;
    for git_repo in [false, true] {
        let dir = TestDir::new("concurrent-init");
        if git_repo {
            dir.init_git(dir.path());
        }
        let children: Vec<_> = (0..8)
            .map(|i| {
                dir.axon_command()
                    .args([
                        "init",
                        "--backend",
                        if i % 2 == 0 { "sqlite" } else { "file" },
                    ])
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap()
            })
            .collect();
        assert_eq!(
            children
                .into_iter()
                .map(|c| c.wait_with_output().unwrap())
                .filter(|o| o.status.success())
                .count(),
            1
        );
        assert_ne!(
            state(dir.path()).exists(),
            dir.path().join(".axon/axon.db").exists()
        );
        assert!(!dir.path().join(".axon/init.pending").exists());
        make(&dir, dir.path());
    }
}

#[test]
fn empty_nested_directory_is_not_a_boundary_but_cannot_be_initialized() {
    let dir = TestDir::new("empty-nested");
    run(&dir, dir.path(), &["init", "--backend", "file"]);
    let inner = dir.path().join("inner");
    fs::create_dir_all(inner.join(".axon")).unwrap();
    fs::write(inner.join(".axon/write.lock"), "").unwrap();
    let id = make(&dir, &inner);
    run(&dir, dir.path(), &["show", &id]);
    assert_failure(&dir.axon_in(&inner, &["init"]));
    assert!(!state(&inner).exists());
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
    fs::write(&plan,"schema: axon-plan/v3\nissues:\n  - id: null\n    key: new\n    base: null\n    title: Imported\n    description: null\n    observed:\n      progress: not_started\n      claim: null\n      disposition: accepted\n      resurface:\n        kind: always\ngroups: []\nrelations:\n  editable:\n    parents: []\n    dependencies: []\n  readonly:\n    parents: []\n    dependencies: []\nreferences:\n  entities: []\n").unwrap();
    for command in ["prepare", "check", "apply"] {
        run(&dir, root, &["import", command, plan.to_str().unwrap()]);
    }
    let bytes = fs::read(state(root)).unwrap();
    run(&dir, root, &["import", "apply", plan.to_str().unwrap()]);
    assert_eq!(fs::read(state(root)).unwrap(), bytes);
    assert!(run(&dir, root, &["list"]).contains("Imported"));
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
fn worktrees_and_clone_have_independent_file_state() {
    let dir = TestDir::new("file-worktrees");
    let main = dir.path().join("main");
    fs::create_dir(&main).unwrap();
    dir.init_git(&main);
    run(&dir, &main, &["init", "--backend", "file"]);
    let id = make(&dir, &main);
    git(
        &main,
        &[
            "add",
            ".axon/.gitignore",
            ".axon/state.jsonl",
            ".gitattributes",
        ],
    );
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
}
#[test]
fn invalid_missing_partial_and_unmerged_never_fall_back() {
    let dir = TestDir::new("file-invalid");
    let root = dir.path();
    run(&dir, root, &["init", "--backend", "file"]);
    let original = fs::read(state(root)).unwrap();
    assert_failure(&dir.axon_in(root, &["init"]));
    fs::write(state(root), b"corrupt").unwrap();
    assert_failure(&dir.axon_in(root, &["list"]));
    assert_failure(&dir.axon_in(root, &["init"]));
    assert_eq!(fs::read(state(root)).unwrap(), b"corrupt");
    fs::write(state(root), &original).unwrap();
    fs::write(root.join(".axon/axon.db"), b"other backend").unwrap();
    assert!(stderr(&dir.axon_in(root, &["list"])).contains("mixed backends"));
    fs::remove_file(root.join(".axon/axon.db")).unwrap();
    fs::write(root.join(".axon/init.pending"), b"incomplete").unwrap();
    fs::remove_file(state(root)).unwrap();
    assert_failure(&dir.axon_in(root, &["list"]));
    assert_failure(&dir.axon_in(root, &["init"]));
    assert!(!state(root).exists());
    fs::remove_file(root.join(".axon/init.pending")).unwrap();
    fs::write(state(root), &original).unwrap();
    dir.init_git(root);
    git(
        root,
        &[
            "add",
            ".axon/.gitignore",
            ".axon/state.jsonl",
            ".gitattributes",
        ],
    );
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
fn sqlite_worktrees_share_state_without_registration() {
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
    let before = fs::read(main.join(".axon/axon.db")).unwrap();
    assert_failure(&dir.axon_in(&other, &["init", "wrong"]));
    assert!(!other.join(".axon/init.pending").exists());
    assert!(!other.join(".axon").exists());
    assert_failure(&dir.axon_in(&other, &["init", "original"]));
    assert_failure(&dir.axon_in(&other, &["init", "--backend", "file"]));
    run(&dir, &other, &["list"]);
    assert_eq!(fs::read(main.join(".axon/axon.db")).unwrap(), before);
}

#[cfg(unix)]
#[test]
fn dangling_state_never_escapes_to_an_ancestor_store() {
    use std::os::unix::fs::symlink;
    let dir = TestDir::new("file-dangling-config");
    let outer = dir.path();
    run(&dir, outer, &["init", "--backend", "file"]);
    let bytes = fs::read(state(outer)).unwrap();
    let inner = outer.join("inner");
    fs::create_dir_all(inner.join(".axon")).unwrap();
    symlink("absent.json", state(&inner)).unwrap();
    assert_failure(&dir.axon_in(&inner, &["plan", "must not reach outer"]));
    assert_failure(&dir.axon_in(&inner, &["init", "--backend", "file"]));
    assert_eq!(fs::read(state(outer)).unwrap(), bytes);
    assert!(!state(&inner).exists());
}

#[test]
fn incomplete_nested_root_does_not_open_outer_state() {
    let dir = TestDir::new("file-partial-boundary");
    let outer = dir.path();
    run(&dir, outer, &["init", "--backend", "file"]);
    let bytes = fs::read(state(outer)).unwrap();
    let inner = outer.join("inner");
    fs::create_dir_all(inner.join(".axon")).unwrap();
    fs::write(inner.join(".axon/init.pending"), "incomplete").unwrap();
    assert_failure(&dir.axon_in(&inner, &["plan", "must not reach outer"]));
    assert_failure(&dir.axon_in(&inner, &["init", "--backend", "file"]));
    assert_eq!(fs::read(state(outer)).unwrap(), bytes);
}
