use super::*;
use axon::{file, file_merge, location::Location};

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
    f.init();
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
    failure(f.run(&["start", &group]));
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
fn writer_checks_drift_and_corruption_and_preserves_noop_bytes() {
    let f = Fixture::new();
    f.init();
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
    // A snapshot that disappears mid-change is not recreated by the write.
    let error = store
        .update(|_, s| {
            let id = s.entities().next().unwrap().id.clone();
            s.write(&id, Some("changed".into()), None)?;
            fs::remove_file(state(&f))?;
            Ok(())
        })
        .unwrap_err();
    assert!(error.to_string().contains("not applied"), "{error}");
    assert!(!state(&f).exists());
}

#[test]
fn init_refuses_to_replace_an_existing_store() {
    let f = Fixture::new();
    f.init();
    let id = f.accepted("kept");
    let before = fs::read(state(&f)).unwrap();
    assert!(failure(f.run(&["init"])).contains("already initialized"));
    assert!(failure(f.run(&["init", "other"])).contains("already initialized"));
    assert_eq!(fs::read(state(&f)).unwrap(), before);
    assert!(f.ok(&["list"]).contains(&id));
}

fn branch_inputs(f: &Fixture) -> (PathBuf, PathBuf, PathBuf, EntityId) {
    f.init();
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
        let candidate = fs::read(workspace.join("candidate.jsonl")).unwrap();
        file_merge::check(&workspace).unwrap();
        assert_eq!(
            candidate,
            fs::read(workspace.join("candidate.jsonl")).unwrap(),
            "checking unchanged inputs again yields the same candidate"
        );
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
fn tracked_worktrees_are_isolated_and_the_git_driver_merges_notes() {
    let f = Fixture::new();
    git(&f.0, &["init", "-q"]);
    f.init();
    let id = f.accepted("job");
    track_store(&f.0);
    git_commit(&f.0, &["-qm", "base"]);
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
        git_commit(&branch.0, &["-qm", "branch"]);
    }
    git(
        &a.0,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "core.hooksPath=/dev/null",
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
    git_commit(&a.0, &["-qm", "invalid snapshot"]);
    git(&f.0, &["merge", "--ff-only", "a"]);
    failure(f.run(&["list"]));
    assert_eq!(
        fs::read(state(&f)).unwrap(),
        b"broken fast-forward snapshot"
    );
}

#[test]
fn valid_snapshot_with_unmerged_index_rejects_normal_operations() {
    let f = Fixture::new();
    git(&f.0, &["init", "-q"]);
    f.init();
    let id = f.accepted("job");
    track_store(&f.0);
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
    git_commit(&f.0, &["-q", "--allow-empty", "-m", "base"]);
    f.init();
    let id = f.accepted("counted");
    let linked = Fixture::new();
    git(
        &f.0,
        &[
            "worktree",
            "add",
            "-qb",
            "linked",
            linked.0.to_str().unwrap(),
        ],
    );
    let shim = Fixture::new();
    let log = git_call_log(&shim.0);
    let calls = |cwd: &Path, args: &[&str]| -> Vec<String> {
        fs::write(&log, "").unwrap();
        success(
            command(cwd)
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
    // Discovery runs one rev-parse, and one more only from a linked worktree without a store of
    // its own, which has to look at the main worktree. Reads check the index once, writes twice.
    for (worktree, rev_parse) in [(&f.0, 1), (&linked.0, 2)] {
        for (args, unmerged) in [
            (vec!["list"], 1),
            (vec!["proposals"], 1),
            (vec!["tasks"], 1),
            (vec!["show", &id], 1),
            (vec!["capture", "--title", "written"], 2),
            (vec!["note", "add", &id, "-m", "evidence"], 2),
        ] {
            let observed = calls(worktree, &args);
            assert_eq!(
                observed.len(),
                rev_parse + unmerged,
                "{args:?}: {observed:?}"
            );
            assert_eq!(
                observed
                    .iter()
                    .filter(|call| call.starts_with("rev-parse"))
                    .count(),
                rev_parse,
                "{args:?}: {observed:?}"
            );
            assert_eq!(
                observed
                    .iter()
                    .filter(|call| call.contains("ls-files --unmerged"))
                    .count(),
                unmerged,
                "{args:?}: {observed:?}"
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn newline_in_a_git_worktree_path_fails_discovery_explicitly() {
    let f = Fixture::new();
    let repo = f.0.join("repo\nline");
    fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    for args in [vec!["init", "t"], vec!["list"]] {
        let error = failure(command(&repo).args(args).output().unwrap());
        assert!(error.contains("newline"), "{error}");
    }
    assert!(!repo.join(".axon").exists());
}

#[test]
fn corrupt_canonical_rejects_reads_and_writes_without_changing_bytes() {
    let f = Fixture::new();
    f.init();
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
fn merge_rejects_foreign_store_unreviewed_destination_and_symlink_directory() {
    for extra_work in [false, true] {
        let f = Fixture::new();
        let (base, ours, theirs, id) = branch_inputs(&f);
        let target = Fixture::new();
        target.init();
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
fn init_refuses_an_unmerged_index_without_creating_a_store() {
    let f = Fixture::new();
    git(&f.0, &["init", "-q"]);
    f.init();
    track_store(&f.0);
    git_commit(&f.0, &["-qm", "base"]);
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
    assert!(failure(f.run(&["init"])).contains("unmerged"));
    assert!(!state(&f).exists());
    assert!(!f.0.join(".axon/init.pending").exists());
}

#[test]
fn git_index_fixtures_do_not_touch_an_inherited_hook_index() {
    let f = Fixture::new();
    let index = f.0.join("foreign-index");
    fs::write(&index, b"foreign index must remain untouched").unwrap();
    for test in [
        "file_lifecycle::valid_snapshot_with_unmerged_index_rejects_normal_operations",
        "file_lifecycle::init_refuses_an_unmerged_index_without_creating_a_store",
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
    f.init();
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
    f.ok(&["start", &id]);
    track_store(&f.0);
    let commit = |path: &Path, message: &str| git_commit(path, &["-qam", message]);
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
    let merge = git_output(
        &a.0,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "core.hooksPath=/dev/null",
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
