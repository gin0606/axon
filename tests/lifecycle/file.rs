use super::*;
use axon::{file, file_merge, location::Location};

fn init_file(f: &Fixture) {
    f.ok(&["init", "t", "--backend", "file"]);
}

fn file_init_output(f: &Fixture, gitignore: &str, attributes: &str) -> String {
    format!(
        "Initialized file at {}\n{gitignore}: {}\n{attributes}: {}\n",
        f.0.join(".axon/state.jsonl").display(),
        f.0.join(".axon/.gitignore").display(),
        f.0.join(".gitattributes").display(),
    )
}

#[test]
fn init_reports_git_integration_changes_for_each_backend_and_environment() {
    let created_in_git = Fixture::new();
    git(&created_in_git.0, &["init", "-q"]);
    assert_eq!(
        created_in_git.ok(&["init", "t", "--backend", "file"]),
        file_init_output(&created_in_git, "Created", "Created")
    );

    let appended = Fixture::new();
    fs::write(appended.0.join(".gitattributes"), "*.txt text\n").unwrap();
    assert_eq!(
        appended.ok(&["init", "t", "--backend", "file"]),
        file_init_output(&appended, "Created", "Appended")
    );
    assert_eq!(
        fs::read_to_string(appended.0.join(".gitattributes")).unwrap(),
        "*.txt text\n/.axon/state.jsonl merge=axon\n"
    );

    let unchanged = Fixture::new();
    fs::create_dir(unchanged.0.join(".axon")).unwrap();
    let ignore = b"*\n!.gitignore\n!state.jsonl\n";
    let attributes = b"*.txt text\n/.axon/state.jsonl merge=axon\n";
    fs::write(unchanged.0.join(".axon/.gitignore"), ignore).unwrap();
    fs::write(unchanged.0.join(".gitattributes"), attributes).unwrap();
    assert_eq!(
        unchanged.ok(&["init", "t", "--backend", "file"]),
        file_init_output(&unchanged, "Unchanged", "Unchanged")
    );
    assert_eq!(
        fs::read(unchanged.0.join(".axon/.gitignore")).unwrap(),
        ignore
    );
    assert_eq!(
        fs::read(unchanged.0.join(".gitattributes")).unwrap(),
        attributes
    );

    let created_outside_git = Fixture::new();
    assert_eq!(
        created_outside_git.ok(&["init", "t", "--backend", "file"]),
        file_init_output(&created_outside_git, "Created", "Created")
    );

    let sqlite = Fixture::new();
    assert_eq!(
        sqlite.ok(&["init", "t"]),
        format!("Initialized SQLite at {}\n", sqlite.db().display())
    );
    assert!(!sqlite.0.join(".axon/.gitignore").exists());
    assert!(!sqlite.0.join(".gitattributes").exists());
}
fn state(f: &Fixture) -> PathBuf {
    f.0.join(".axon/state.jsonl")
}
fn snapshot(f: &Fixture) -> Snapshot {
    file::decode(&fs::read(state(f)).unwrap()).unwrap().1
}
fn ctx() -> Context {
    Context {
        at: "2026-09-12T00:00:00Z".parse().unwrap(),
        recorder: None,
    }
}

#[test]
fn file_cli_roundtrip_and_atomic_concurrency() {
    let f = Fixture::new();
    init_file(&f);
    let group = f
        .ok(&["capture", "--kind", "group", "--accept", "--title", "group"])
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    let issue = f
        .ok(&[
            "capture", "--accept", "--title", "child", "--parent", &group,
        ])
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    failure(f.run(&["start", &issue]));
    f.ok(&["start", &group]);
    let mut processes = (0..6)
        .map(|_| f.command().args(["start", &issue]).spawn().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        processes
            .iter_mut()
            .filter_map(|p| p.wait().ok())
            .filter(|s| s.success())
            .count(),
        1
    );
    let mut processes = (0..8)
        .map(|i| {
            f.command()
                .args(["note", "add", &issue, "-m", &format!("note {i}")])
                .spawn()
                .unwrap()
        })
        .collect::<Vec<_>>();
    for p in &mut processes {
        assert!(p.wait().unwrap().success());
    }
    assert_eq!(
        snapshot(&f)
            .notes(&issue.clone().try_into().unwrap())
            .unwrap()
            .len(),
        8
    );
    f.ok(&["release", &issue]);
    f.ok(&["start", &issue]);
    f.ok(&["complete", &issue]);
    assert!(
        f.ok(&["show", &group])
            .contains("Awaiting final confirmation")
    );
    f.ok(&["complete", &group]);
    assert!(f.ok(&["log", &issue]).contains("Completed"));
    assert!(f.ok(&["tasks"]).is_empty());
    let before = fs::read(state(&f)).unwrap();
    failure(f.run(&["start", &issue]));
    assert_eq!(fs::read(state(&f)).unwrap(), before);
}

#[test]
fn file_init_preserves_rules_rejects_conflicts_and_existing_store() {
    let f = Fixture::new();
    fs::create_dir(f.0.join(".axon")).unwrap();
    fs::write(f.0.join(".axon/.gitignore"), "# keep\n!state.jsonl\n").unwrap();
    fs::write(f.0.join(".gitattributes"), "*.txt text\n").unwrap();
    init_file(&f);
    assert_eq!(
        fs::read_to_string(f.0.join(".axon/.gitignore")).unwrap(),
        "*\n# keep\n!.gitignore\n!state.jsonl\n"
    );
    assert_eq!(
        fs::read_to_string(f.0.join(".gitattributes")).unwrap(),
        "*.txt text\n/.axon/state.jsonl merge=axon\n"
    );
    let before = fs::read(state(&f)).unwrap();
    failure(f.run(&["init", "--backend", "file"]));
    assert_eq!(fs::read(state(&f)).unwrap(), before);
    let broken = Fixture::new();
    fs::write(
        broken.0.join(".gitattributes"),
        "/.axon/state.jsonl merge=other\n",
    )
    .unwrap();
    assert!(failure(broken.run(&["init", "--backend", "file"])).contains("conflicting rule"));
    assert!(broken.0.join(".axon/init.pending").exists());
    assert!(failure(broken.run(&["list"])).contains("incomplete initialization"));
    assert_eq!(
        fs::read_to_string(broken.0.join(".gitattributes")).unwrap(),
        "/.axon/state.jsonl merge=other\n"
    );
}

#[test]
fn writer_checks_drift_corruption_backend_and_preserves_noop_bytes() {
    let f = Fixture::new();
    init_file(&f);
    f.accepted("one");
    let mut bytes = fs::read(state(&f)).unwrap();
    let split = bytes.iter().position(|b| *b == b'\n').unwrap();
    bytes.insert(split, b' ');
    fs::write(state(&f), &bytes).unwrap();
    let mut store = Location::discover(&f.0, false).unwrap().open().unwrap();
    store.update(|_, _| Ok(())).unwrap();
    assert_eq!(fs::read(state(&f)).unwrap(), bytes);
    let error = store
        .update(|_, s| {
            let id = s.entities().next().unwrap().id.clone();
            s.write(&id, Some("changed".into()), None)?;
            fs::write(state(&f), b"foreign")?;
            Ok(())
        })
        .unwrap_err();
    assert!(error.to_string().contains("not applied"));
    assert_eq!(fs::read(state(&f)).unwrap(), b"foreign");
    failure(f.run(&["list"]));
    fs::write(state(&f), &bytes).unwrap();
    let error = store
        .update(|_, s| {
            let id = s.entities().next().unwrap().id.clone();
            s.write(&id, Some("changed".into()), None)?;
            fs::write(f.db(), b"mixed")?;
            Ok(())
        })
        .unwrap_err();
    assert!(error.to_string().contains("mixed"));
    assert_eq!(fs::read(state(&f)).unwrap(), bytes);
}

fn branch_inputs(f: &Fixture) -> (PathBuf, PathBuf, PathBuf, EntityId) {
    init_file(f);
    let id: EntityId = f.accepted("job").try_into().unwrap();
    let mut base = snapshot(f);
    base.perform(&id, Operation::Start, None, ctx()).unwrap();
    let mut ours = base.clone();
    let mut theirs = base.clone();
    ours.perform(&id, Operation::Complete, Some("finished".into()), ctx())
        .unwrap();
    theirs
        .perform(&id, Operation::Release, Some("more work".into()), ctx())
        .unwrap();
    ours.add_note(&id, "ours".into(), ctx()).unwrap();
    theirs.add_note(&id, "theirs".into(), ctx()).unwrap();
    let paths = (f.0.join("base"), f.0.join("ours"), f.0.join("theirs"));
    for (p, s) in [(&paths.0, &base), (&paths.1, &ours), (&paths.2, &theirs)] {
        fs::write(p, file::encode("t", s).unwrap()).unwrap();
    }
    fs::write(state(f), file::encode("t", &ours).unwrap()).unwrap();
    (paths.0, paths.1, paths.2, id)
}
fn resolve(workspace: &Path, id: &EntityId) {
    fs::write(workspace.join("resolution.json"), serde_json::to_vec(&serde_json::json!({"choices": {&id.to_string(): "Right"}, "reason": "continue remaining work", "repairs": []})).unwrap()).unwrap();
}

#[test]
fn merge_workspace_preserves_both_branches_and_rejects_drift() {
    for drift in [
        "none",
        "conflict_preimage",
        "input",
        "destination",
        "resolution",
        "candidate",
        "preserved",
    ] {
        let f = Fixture::new();
        let (base, ours, theirs, id) = branch_inputs(&f);
        let workspace = f.0.join("review");
        if drift == "conflict_preimage" {
            fs::write(state(&f), b"<<<<<<< ours\n=======\n>>>>>>> theirs\n").unwrap();
        }
        let before = fs::read(state(&f)).unwrap();
        assert!(file_merge::prepare(&base, &ours, &theirs, &state(&f), &workspace, ctx()).is_err());
        assert_eq!(fs::read(state(&f)).unwrap(), before);
        assert!(workspace.join("choices.json").is_file());
        resolve(&workspace, &id);
        file_merge::check(&workspace).unwrap();
        match drift {
            "input" => fs::write(&theirs, b"changed").unwrap(),
            "destination" => fs::write(state(&f), b"changed").unwrap(),
            "resolution" => fs::write(workspace.join("resolution.json"), b"{}").unwrap(),
            "candidate" => fs::write(workspace.join("candidate.jsonl"), before.clone()).unwrap(),
            "preserved" => fs::write(workspace.join("ours.jsonl"), b"changed").unwrap(),
            _ => {}
        }
        let preapply = fs::read(state(&f)).unwrap();
        let result = file_merge::apply(&workspace);
        if drift == "none" || drift == "conflict_preimage" {
            result.unwrap();
            let s = snapshot(&f);
            assert_eq!(
                s.entity(&id).unwrap().current.lifecycle,
                Lifecycle::NotStarted
            );
            assert_eq!(s.notes(&id).unwrap().len(), 2);
            assert!(
                s.history(&id)
                    .unwrap()
                    .iter()
                    .any(|r| matches!(r.event, StateEvent::Integration { .. }))
            );
            f.ok(&["start", &id.to_string()]);
        } else {
            assert!(result.is_err(), "{drift}");
            assert_eq!(fs::read(state(&f)).unwrap(), preapply);
        }
    }
}

#[test]
fn merge_cli_and_driver_share_engine() {
    let f = Fixture::new();
    let (base, ours, theirs, id) = branch_inputs(&f);
    let before = fs::read(&ours).unwrap();
    failure(f.run(&[
        "merge",
        "driver",
        base.to_str().unwrap(),
        ours.to_str().unwrap(),
        theirs.to_str().unwrap(),
    ]));
    assert_eq!(fs::read(&ours).unwrap(), before);
    let workspace = f.0.join("review");
    failure(f.run(&[
        "merge",
        "prepare",
        "--base",
        base.to_str().unwrap(),
        "--ours",
        ours.to_str().unwrap(),
        "--theirs",
        theirs.to_str().unwrap(),
        "--output",
        state(&f).to_str().unwrap(),
        "--workspace",
        workspace.to_str().unwrap(),
    ]));
    resolve(&workspace, &id);
    f.ok(&["merge", "check", workspace.to_str().unwrap()]);
    f.ok(&["merge", "apply", workspace.to_str().unwrap()]);
    f.ok(&["storage", "check", state(&f).to_str().unwrap()]);
    fs::write(&ours, fs::read(&base).unwrap()).unwrap();
    f.ok(&[
        "merge",
        "driver",
        base.to_str().unwrap(),
        ours.to_str().unwrap(),
        theirs.to_str().unwrap(),
    ]);
    let merged = file::decode(&fs::read(&ours).unwrap()).unwrap().1;
    let source = file::decode(&fs::read(&theirs).unwrap()).unwrap().1;
    assert_eq!(
        merged.entity(&id).unwrap().current,
        source.entity(&id).unwrap().current
    );
    for record in source.history(&id).unwrap() {
        assert_eq!(merged.state_record(&record.id).unwrap(), record);
    }
    assert_eq!(merged.notes(&id).unwrap(), source.notes(&id).unwrap());
}

#[test]
fn file_worktrees_are_isolated_and_git_driver_merges_notes() {
    let f = Fixture::new();
    git(&f.0, &["init", "-q"]);
    init_file(&f);
    let id = f.accepted("job");
    git(
        &f.0,
        &[
            "add",
            ".axon/state.jsonl",
            ".axon/.gitignore",
            ".gitattributes",
        ],
    );
    git(
        &f.0,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-qm",
            "base",
        ],
    );
    let a = Fixture::new();
    let b = Fixture::new();
    git(
        &f.0,
        &["worktree", "add", "-qb", "a", a.0.to_str().unwrap()],
    );
    git(
        &f.0,
        &["worktree", "add", "-qb", "b", b.0.to_str().unwrap()],
    );
    let before = fs::read(state(&f)).unwrap();
    a.ok(&["start", &id]);
    a.ok(&["note", "add", &id, "-m", "A"]);
    assert_eq!(fs::read(state(&f)).unwrap(), before);
    assert_eq!(fs::read(state(&b)).unwrap(), before);
    b.ok(&["start", &id]);
    b.ok(&["note", "add", &id, "-m", "B"]);
    for branch in [&a, &b] {
        git(&branch.0, &["add", ".axon/state.jsonl"]);
        git(
            &branch.0,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-qm",
                "branch",
            ],
        );
    }
    let driver = format!("'{}' merge driver %O %A %B", env!("CARGO_BIN_EXE_axon"));
    git(&f.0, &["config", "merge.axon.driver", &driver]);
    git(
        &a.0,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "merge",
            "--no-edit",
            "b",
        ],
    );
    assert_eq!(
        snapshot(&a)
            .notes(&id.clone().try_into().unwrap())
            .unwrap()
            .len(),
        2
    );
    assert_eq!(fs::read(state(&f)).unwrap(), before);
    git(&f.0, &["merge", "--ff-only", "a"]);
    assert_eq!(
        snapshot(&f).notes(&id.try_into().unwrap()).unwrap().len(),
        2
    );
    fs::write(state(&a), b"broken fast-forward snapshot").unwrap();
    git(&a.0, &["add", ".axon/state.jsonl"]);
    git(
        &a.0,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-qm",
            "invalid snapshot",
        ],
    );
    git(&f.0, &["merge", "--ff-only", "a"]);
    failure(f.run(&["list"]));
    assert_eq!(
        fs::read(state(&f)).unwrap(),
        b"broken fast-forward snapshot"
    );
}

#[test]
fn init_from_a_linked_worktree_refuses_the_directory_of_an_existing_file_store() {
    let f = Fixture::new();
    git(&f.0, &["init", "-q"]);
    git(
        &f.0,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "base",
        ],
    );
    git(&f.0, &["branch", "without-store"]);
    init_file(&f);
    let id = f.accepted("job");
    let before = fs::read(state(&f)).unwrap();
    let linked = Fixture::new();
    fs::remove_dir(&linked.0).unwrap();
    git(
        &f.0,
        &[
            "worktree",
            "add",
            "-q",
            linked.0.to_str().unwrap(),
            "without-store",
        ],
    );
    let error = failure(linked.run(&["init", "demo"]));
    assert!(error.contains("already holds a store"), "{error}");
    assert!(!f.0.join(".axon/axon.db").exists());
    assert_eq!(before, fs::read(state(&f)).unwrap());
    assert!(f.ok(&["list"]).contains(&id));
}

#[test]
fn valid_snapshot_with_unmerged_index_rejects_normal_operations() {
    let f = Fixture::new();
    git(&f.0, &["init", "-q"]);
    init_file(&f);
    let id = f.accepted("job");
    git(&f.0, &["add", ".axon/state.jsonl"]);
    let blob =
        String::from_utf8(git_output(&f.0, &["hash-object", ".axon/state.jsonl"]).stdout).unwrap();
    git(
        &f.0,
        &["update-index", "--force-remove", ".axon/state.jsonl"],
    );
    let line = format!(
        "100644 {} 1\t.axon/state.jsonl\n100644 {} 2\t.axon/state.jsonl\n100644 {} 3\t.axon/state.jsonl\n",
        blob.trim(),
        blob.trim(),
        blob.trim()
    );
    let mut p = isolated_git(&f.0)
        .args(["update-index", "--index-info"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    p.stdin.take().unwrap().write_all(line.as_bytes()).unwrap();
    assert!(p.wait().unwrap().success());
    let before = fs::read(state(&f)).unwrap();
    assert!(failure(f.run(&["list"])).contains("unmerged"));
    failure(f.run(&["start", &id]));
    assert_eq!(fs::read(state(&f)).unwrap(), before);
    f.ok(&["storage", "check", state(&f).to_str().unwrap()]);
    git(&f.0, &["add", ".axon/state.jsonl"]);
    f.ok(&["start", &id]);
}
fn git_output(path: &Path, args: &[&str]) -> Output {
    isolated_git(path).args(args).output().unwrap()
}

#[cfg(unix)]
fn git_on_path() -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|dir| dir.join("git"))
        .find(|path| path.is_file())
        .expect("git on PATH")
}

/// A `git` that logs its arguments and hands over to the real one.
#[cfg(unix)]
fn git_call_log(shim: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let log = shim.join("git-calls");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nexec '{}' \"$@\"\n",
        log.display(),
        git_on_path().display()
    );
    let binary = shim.join("git");
    fs::write(&binary, script).unwrap();
    fs::set_permissions(&binary, PermissionsExt::from_mode(0o755)).unwrap();
    log
}

#[cfg(unix)]
#[test]
fn git_worktree_operations_start_few_git_processes() {
    let f = Fixture::new();
    git(&f.0, &["init", "-q"]);
    init_file(&f);
    let id = f.accepted("counted");
    let pending = f
        .ok(&["capture", "--title", "undecided"])
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    let shim = Fixture::new();
    let log = git_call_log(&shim.0);
    let calls = |args: &[&str]| -> Vec<String> {
        fs::write(&log, "").unwrap();
        success(
            f.command()
                .env(
                    "PATH",
                    format!(
                        "{}:{}",
                        shim.0.display(),
                        std::env::var("PATH").unwrap_or_default()
                    ),
                )
                .args(args)
                .output()
                .unwrap(),
        );
        fs::read_to_string(&log)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect()
    };
    for args in [
        vec!["list"],
        vec!["proposals"],
        vec!["tasks"],
        vec!["show", &id],
    ] {
        let observed = calls(&args);
        assert!(observed.len() <= 2, "{args:?}: {observed:?}");
        assert_eq!(
            observed
                .iter()
                .filter(|call| call.starts_with("rev-parse"))
                .count(),
            1,
            "{args:?}: {observed:?}"
        );
        assert_eq!(
            observed
                .iter()
                .filter(|call| call.contains("ls-files --unmerged"))
                .count(),
            1,
            "{args:?}: {observed:?}"
        );
    }
    for args in [
        vec!["capture", "--title", "written"],
        vec!["note", "add", &id, "-m", "evidence"],
        vec!["accept", &pending],
    ] {
        let observed = calls(&args);
        assert!(observed.len() <= 3, "{args:?}: {observed:?}");
        assert_eq!(
            observed
                .iter()
                .filter(|call| call.starts_with("rev-parse"))
                .count(),
            1,
            "{args:?}: {observed:?}"
        );
        assert_eq!(
            observed
                .iter()
                .filter(|call| call.contains("ls-files --unmerged"))
                .count(),
            2,
            "{args:?}: {observed:?}"
        );
    }
}

#[cfg(unix)]
#[test]
fn newline_in_a_git_worktree_path_fails_discovery_explicitly() {
    let f = Fixture::new();
    let repo = f.0.join("repo\nline");
    fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    for args in [
        vec!["init", "t", "--backend", "file"],
        vec!["init", "t"],
        vec!["list"],
    ] {
        let error = failure(command(&repo).args(args).output().unwrap());
        assert!(error.contains("newline"), "{error}");
    }
    assert!(!repo.join(".axon").exists());
}

#[test]
fn corrupt_canonical_rejects_reads_and_writes_without_changing_bytes() {
    let f = Fixture::new();
    init_file(&f);
    let id = f.accepted("job");
    fs::write(state(&f), b"not a snapshot\n").unwrap();
    let before = fs::read(state(&f)).unwrap();
    for args in [
        vec!["list"],
        vec!["show", &id],
        vec!["start", &id],
        vec!["note", "add", &id, "-m", "rejected"],
        vec!["capture", "--title", "rejected"],
    ] {
        failure(f.run(&args));
        assert_eq!(fs::read(state(&f)).unwrap(), before, "{args:?}");
    }
}

#[test]
fn adapters_preserve_identical_snapshot_records_and_failures() {
    let f = Fixture::new();
    init_file(&f);
    let id: EntityId = f.accepted("same").try_into().unwrap();
    let mut expected = snapshot(&f);
    let sqlpath = f.0.join("parity.db");
    let mut sql = Store::create(&sqlpath, "t", &expected).unwrap();
    let mut local = Location::discover(&f.0, false).unwrap().open().unwrap();
    for op in [
        Operation::Start,
        Operation::Release,
        Operation::Start,
        Operation::Complete,
    ] {
        expected
            .perform(&id, op, Some("same reason".into()), ctx())
            .unwrap();
        expected.add_note(&id, "same note".into(), ctx()).unwrap();
        sql.update(|_, s| {
            *s = expected.clone();
            Ok(())
        })
        .unwrap();
        local
            .update(|_, s| {
                *s = expected.clone();
                Ok(())
            })
            .unwrap();
        assert_eq!(sql.read().unwrap(), local.read().unwrap());
    }
    let sql_error = sql
        .update(|_, s| Ok(s.perform(&id, Operation::Start, None, ctx())?))
        .unwrap_err()
        .to_string();
    let file_error = local
        .update(|_, s| Ok(s.perform(&id, Operation::Start, None, ctx())?))
        .unwrap_err()
        .to_string();
    assert_eq!(sql_error, file_error);
    assert_eq!(sql.read().unwrap(), local.read().unwrap());
}

#[test]
fn merge_rejects_foreign_store_unreviewed_destination_and_symlink_directory() {
    for extra_work in [false, true] {
        let f = Fixture::new();
        let (base, ours, theirs, id) = branch_inputs(&f);
        let target = Fixture::new();
        init_file(&target);
        if extra_work {
            let mut s = file::decode(&fs::read(&ours).unwrap()).unwrap().1;
            s.add_note(&id, "unreviewed work".into(), ctx()).unwrap();
            fs::write(state(&target), file::encode("t", &s).unwrap()).unwrap();
        } else {
            target.accepted("must survive");
        }
        let before = fs::read(state(&target)).unwrap();
        let workspace = target.0.join("review");
        assert!(
            file_merge::prepare(&base, &ours, &theirs, &state(&target), &workspace, ctx()).is_err()
        );
        assert!(file_merge::apply(&workspace).is_err());
        assert_eq!(fs::read(state(&target)).unwrap(), before);
    }
    #[cfg(unix)]
    {
        let f = Fixture::new();
        let (base, ours, theirs, _) = branch_inputs(&f);
        let target = Fixture::new();
        std::os::unix::fs::symlink(f.0.join(".axon"), target.0.join(".axon")).unwrap();
        let before = fs::read(state(&f)).unwrap();
        assert!(
            file_merge::prepare(
                &base,
                &ours,
                &theirs,
                &state(&target),
                &target.0.join("review"),
                ctx()
            )
            .is_err()
        );
        assert_eq!(fs::read(state(&f)).unwrap(), before);
    }
}

#[test]
fn init_honors_final_rules_and_refuses_unmerged_missing_state() {
    let f = Fixture::new();
    git(&f.0, &["init", "-q"]);
    fs::create_dir(f.0.join(".axon")).unwrap();
    fs::write(
        f.0.join(".gitattributes"),
        "/.axon/state.jsonl merge=axon\n*.jsonl merge=other\n",
    )
    .unwrap();
    fs::write(f.0.join(".axon/.gitignore"), "!state.jsonl\n/*\n").unwrap();
    init_file(&f);
    assert!(
        String::from_utf8(
            git_output(&f.0, &["check-attr", "merge", "--", ".axon/state.jsonl"]).stdout
        )
        .unwrap()
        .contains("merge: axon")
    );
    assert!(
        !git_output(&f.0, &["check-ignore", "-q", ".axon/state.jsonl"])
            .status
            .success()
    );
    let blob =
        String::from_utf8(git_output(&f.0, &["hash-object", "-w", ".axon/state.jsonl"]).stdout)
            .unwrap();
    let line = format!("100644 {} 2\t.axon/state.jsonl\n", blob.trim());
    let mut p = isolated_git(&f.0)
        .args(["update-index", "--index-info"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    p.stdin.take().unwrap().write_all(line.as_bytes()).unwrap();
    assert!(p.wait().unwrap().success());
    fs::remove_file(state(&f)).unwrap();
    for backend in ["file", "sqlite"] {
        assert!(failure(f.run(&["init", "--backend", backend])).contains("unmerged"));
    }
    assert!(!state(&f).exists());
    assert!(!f.db().exists());
    assert!(!f.0.join(".axon/init.pending").exists());
}

#[test]
fn file_init_rejects_higher_precedence_merge_attributes() {
    for info in [false, true] {
        let f = Fixture::new();
        git(&f.0, &["init", "-q"]);
        fs::create_dir(f.0.join(".axon")).unwrap();
        let path = if info {
            f.0.join(".git/info/attributes")
        } else {
            f.0.join(".axon/.gitattributes")
        };
        let rule = if info {
            ".axon/state.jsonl merge=other\n"
        } else {
            "state.jsonl merge=other\n"
        };
        fs::write(&path, rule).unwrap();
        assert!(
            failure(f.run(&["init", "--backend", "file"]))
                .contains("effective Git merge attribute conflicts")
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), rule);
        assert!(f.0.join(".axon/init.pending").exists());
        assert!(failure(f.run(&["list"])).contains("incomplete initialization"));
    }
}

fn isolated_git(path: &Path) -> Command {
    let mut command = Command::new("git");
    command.current_dir(path);
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("GIT_") {
            command.env_remove(name);
        }
    }
    command
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null");
    command
}

#[test]
fn git_index_fixtures_do_not_touch_an_inherited_hook_index() {
    let f = Fixture::new();
    let index = f.0.join("foreign-index");
    fs::write(&index, b"foreign index must remain untouched").unwrap();
    for test in [
        "file_lifecycle::valid_snapshot_with_unmerged_index_rejects_normal_operations",
        "file_lifecycle::init_honors_final_rules_and_refuses_unmerged_missing_state",
    ] {
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test])
            .env("GIT_INDEX_FILE", &index)
            .env("GIT_DIR", &f.0)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            fs::read(&index).unwrap(),
            b"foreign index must remain untouched"
        );
    }
}

#[test]
fn worktree_conflict_resolution_preserves_operations_and_finishes_group() {
    let f = Fixture::new();
    git(&f.0, &["init", "-q"]);
    init_file(&f);
    let group = f
        .ok(&[
            "capture", "--kind", "group", "--accept", "--title", "delivery",
        ])
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    let id = f
        .ok(&["capture", "--accept", "--title", "job", "--parent", &group])
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    f.ok(&["start", &group]);
    f.ok(&["start", &id]);
    git(
        &f.0,
        &[
            "add",
            ".axon/state.jsonl",
            ".axon/.gitignore",
            ".gitattributes",
        ],
    );
    let commit = |path: &Path, message: &str| {
        git(
            path,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-qam",
                message,
            ],
        );
    };
    commit(&f.0, "base");
    let a = Fixture::new();
    let b = Fixture::new();
    git(
        &f.0,
        &["worktree", "add", "-qb", "finished", a.0.to_str().unwrap()],
    );
    git(
        &f.0,
        &["worktree", "add", "-qb", "remaining", b.0.to_str().unwrap()],
    );
    a.ok(&["complete", &id]);
    a.ok(&["note", "add", &id, "-m", "completed branch evidence"]);
    b.ok(&["release", &id, "-r", "remaining work"]);
    b.ok(&["note", "add", &id, "-m", "remaining branch evidence"]);
    commit(&a.0, "finish");
    commit(&b.0, "release");
    let driver = format!("'{}' merge driver %O %A %B", env!("CARGO_BIN_EXE_axon"));
    git(&f.0, &["config", "merge.axon.driver", &driver]);
    let merge = git_output(
        &a.0,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "merge",
            "--no-edit",
            "remaining",
        ],
    );
    assert!(!merge.status.success());
    failure(a.run(&["show", &id]));
    let inputs = Fixture::new();
    let paths = [
        inputs.0.join("base"),
        inputs.0.join("ours"),
        inputs.0.join("theirs"),
    ];
    for (stage, path) in paths.iter().enumerate() {
        let out = git_output(
            &a.0,
            &["show", &format!(":{}:.axon/state.jsonl", stage + 1)],
        );
        assert!(out.status.success());
        fs::write(path, out.stdout).unwrap();
    }
    let workspace = a.0.join(".axon/review");
    failure(a.run(&[
        "merge",
        "prepare",
        "--base",
        paths[0].to_str().unwrap(),
        "--ours",
        paths[1].to_str().unwrap(),
        "--theirs",
        paths[2].to_str().unwrap(),
        "--output",
        state(&a).to_str().unwrap(),
        "--workspace",
        workspace.to_str().unwrap(),
    ]));
    resolve(&workspace, &eid(&id));
    a.ok(&["merge", "check", workspace.to_str().unwrap()]);
    a.ok(&["merge", "apply", workspace.to_str().unwrap()]);
    a.ok(&["storage", "check", state(&a).to_str().unwrap()]);
    failure(a.run(&["start", &id]));
    git(&a.0, &["add", ".axon/state.jsonl"]);
    commit(&a.0, "resolve remaining work");
    assert!(git_output(&a.0, &["ls-files", "-u"]).stdout.is_empty());
    let merged = snapshot(&a);
    let entity_id = eid(&id);
    assert_eq!(
        merged.entity(&entity_id).unwrap().current.lifecycle,
        Lifecycle::NotStarted
    );
    assert_eq!(merged.notes(&entity_id).unwrap().len(), 2);
    let log = a.ok(&["log", &id]);
    assert!(log.contains("InProgress → Completed"));
    assert!(log.contains("InProgress → NotStarted"));
    assert!(log.contains("Integrated"));
    a.ok(&["start", &id]);
    a.ok(&["complete", &id]);
    assert!(
        a.ok(&["show", &group])
            .contains("Awaiting final confirmation")
    );
    let notes = a.ok(&["note", "list", &id]);
    assert!(notes.contains("completed branch evidence"));
    assert!(notes.contains("remaining branch evidence"));
    a.ok(&["complete", &group]);
    commit(&a.0, "verify delivery");
    git(&f.0, &["merge", "--ff-only", "finished"]);
    assert!(f.ok(&["tasks"]).is_empty());
    assert_eq!(
        snapshot(&b).entity(&entity_id).unwrap().current.lifecycle,
        Lifecycle::NotStarted
    );
}
