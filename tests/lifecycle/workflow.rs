use super::*;

fn created(output: String) -> String {
    output.split_whitespace().next().unwrap().into()
}

#[test]
fn both_backends_support_the_daily_workflow() {
    for backend in ["sqlite", "file"] {
        let f = Fixture::new();
        f.ok(&["init", "trial", "--backend", backend]);
        let group = created(f.ok(&[
            "capture",
            "--kind",
            "group",
            "--accept",
            "--title",
            "納品",
            "-m",
            "全成果を検証",
        ]));
        let first = created(f.ok(&["capture", "--title", "調査", "--parent", &group]));
        let second = created(f.ok(&[
            "capture", "--accept", "--title", "実装", "--parent", &group, "--needs", &first,
        ]));
        assert!(f.ok(&["proposals"]).contains(&first));
        f.ok(&["accept", &first]);
        assert!(f.ok(&["proposals"]).is_empty());
        f.ok(&["condition", "set", &second, "--command", "exit 1"]);
        assert!(!f.ok(&["tasks"]).contains(&second));
        assert!(f.ok(&["list"]).contains(&second));
        f.ok(&["condition", "unset", &second]);
        assert!(f.ok(&["tasks"]).contains(&second));
        failure(f.run(&["start", &first]));
        f.ok(&["start", &group]);
        let mut attempts = (0..4)
            .map(|_| {
                f.command()
                    .args(["start", &first])
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap()
            })
            .collect::<Vec<_>>();
        let statuses = attempts
            .drain(..)
            .map(|p| p.wait_with_output().unwrap().status.success())
            .collect::<Vec<_>>();
        assert_eq!(statuses.iter().filter(|s| **s).count(), 1);
        failure(f.run(&["start", &second]));
        let notes = (0..4)
            .map(|n| {
                f.command()
                    .args(["note", "add", &first, "-m", &format!("結果 {n}")])
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap()
            })
            .collect::<Vec<_>>();
        for child in notes {
            success(child.wait_with_output().unwrap());
        }
        let saved = f.ok(&["note", "list", &first, "--recorder-details"]);
        for n in 0..4 {
            assert!(saved.contains(&format!("結果 {n}")));
        }
        f.ok(&["complete", &first]);
        f.ok(&["start", &second]);
        failure(f.run(&["complete", &group]));
        f.ok(&["note", "add", &second, "-m", "成果を統合・検証済み"]);
        f.ok(&["complete", &second]);
        let review = f.ok(&["show", &group]);
        assert!(review.contains("2/2 terminal (2 completed, 0 cancelled)"));
        assert!(review.contains("Awaiting final confirmation"));
        assert!(f.ok(&["tasks"]).contains(&group));
        assert!(
            f.ok(&["note", "list", &second])
                .contains("成果を統合・検証済み")
        );
        f.ok(&["complete", &group]);
        assert!(f.ok(&["tasks"]).is_empty());
        assert!(f.ok(&["log", &group]).contains("InProgress → Completed"));
    }
}

#[test]
fn sqlite_worktrees_share_parallel_work_and_notes() {
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
            "--allow-empty",
            "-qm",
            "base",
        ],
    );
    f.init();
    let issue = f.accepted("共有する仕事");
    let linked = Fixture::new();
    git(
        &f.0,
        &[
            "worktree",
            "add",
            "-qb",
            "worker",
            linked.0.to_str().unwrap(),
        ],
    );
    assert!(linked.ok(&["tasks"]).contains(&issue));
    let workers = [&f, &linked].map(|w| {
        w.command()
            .args(["start", &issue])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    });
    let successes = workers
        .into_iter()
        .filter_map(|p| p.wait_with_output().ok())
        .filter(|o| o.status.success())
        .count();
    assert_eq!(successes, 1);
    linked.ok(&["note", "add", &issue, "-m", "linked worktreeからの記録"]);
    assert!(
        f.ok(&["note", "list", &issue])
            .contains("linked worktreeからの記録")
    );
    f.ok(&["complete", &issue]);
    assert!(linked.ok(&["tasks"]).is_empty());
    assert!(!linked.db().exists());
}

#[test]
fn help_exposes_single_lifecycle_commands() {
    let f = Fixture::new();
    let help = f.ok(&["--help"]);
    for name in [
        "proposals",
        "tasks",
        "accept",
        "withdraw",
        "cancel",
        "reconsider",
        "merge",
        "storage",
        "export",
        "import",
    ] {
        assert!(help.contains(name));
    }
    for absent in ["ready", "claims", "decide", "migrate", "triage"] {
        assert!(
            !help
                .lines()
                .any(|line| line.trim_start().starts_with(&format!("{absent} ")))
        );
        failure(f.run(&[absent]));
    }
    let rejected = f.run(&["triage"]);
    assert_eq!(rejected.status.code(), Some(2));
    assert!(failure(rejected).contains("unrecognized subcommand"));
    let proposals_help = f.ok(&["proposals", "--help"]);
    for option in [
        "--kind",
        "--search",
        "--condition-timeout",
        "--trace-conditions",
    ] {
        assert!(proposals_help.contains(option));
    }
    let docs = f.ok(&["docs"]);
    assert!(docs.contains("axon proposals"));
    assert!(!docs.contains("triage"));
    for args in [
        vec!["init", "--help"],
        vec!["merge", "--help"],
        vec!["note", "list", "--help"],
    ] {
        f.ok(&args);
    }
    let init_help = f.ok(&["init", "--help"]);
    for text in [
        "SQLite creates only .axon/axon.db",
        "does not change Git integration files",
        "File creates .axon/state.jsonl",
        ".axon/.gitignore",
        "root .gitattributes",
        "preserving unrelated lines",
        "does not stage or commit",
    ] {
        assert!(init_help.contains(text), "missing from init help: {text}");
    }
}
