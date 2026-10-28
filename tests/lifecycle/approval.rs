//! Conditions run only when this clone or management root approved their command: a
//! condition someone else wrote, arriving by merge, checkout or clone, is not run by a read.
use super::*;

const NOTICE: &str = "Conditions not run: 1 unapproved (treated as satisfied); review with axon condition unapproved and allow with axon condition allow ID --digest DIGEST\n";

fn created(output: String) -> String {
    output.split_whitespace().next().unwrap().into()
}
/// A condition that leaves `marker` behind when it runs and is unsatisfied, so it would hide
/// its Entity without a trace if it ran unnoticed.
fn planted(marker: &str) -> String {
    format!("touch {marker}; exit 1")
}
/// A tracked store with one commit, holding a NotStarted Issue and an Undecided Issue whose
/// conditions leave a marker when run.
fn origin() -> (Fixture, String, String) {
    let f = Fixture::new();
    git(&f.0, &["init", "-q", "-b", "main"]);
    git_commit(&f.0, &["-q", "--allow-empty", "-m", "base"]);
    f.init();
    let task = created(f.ok(&[
        "capture",
        "--label",
        "chore",
        "--accept",
        "--title",
        "task",
        "--command",
        &planted("task.marker"),
    ]));
    let proposal = created(f.ok(&[
        "capture",
        "--label",
        "chore",
        "--title",
        "proposal",
        "--command",
        &planted("proposal.marker"),
    ]));
    track_store(&f.0);
    git_commit(&f.0, &["-qm", "plan"]);
    (f, task, proposal)
}
fn clone_of(origin: &Fixture) -> Fixture {
    let f = Fixture::new();
    git(
        &origin.0,
        &[
            "clone",
            "-q",
            origin.0.to_str().unwrap(),
            f.0.to_str().unwrap(),
        ],
    );
    f
}
fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}
/// What a read shows for the two conditions written elsewhere: neither runs, both rows are
/// marked, and each read says once on stderr that a condition was not run.
fn assert_not_run(f: &Fixture, task: &str, proposal: &str) {
    let tasks = f.run(&["tasks"]);
    let rows = success(tasks.clone());
    assert!(
        rows.contains(&format!("{task}  Issue  Ready+Unapproved  chore  task")),
        "{rows}"
    );
    assert_eq!(stderr(&tasks), NOTICE);
    let proposals = f.run(&["proposals"]);
    let rows = success(proposals.clone());
    assert!(
        rows.contains(&format!(
            "{proposal}  Issue  Undecided+Unapproved  chore  proposal"
        )),
        "{rows}"
    );
    assert_eq!(stderr(&proposals), NOTICE);
    let show = f.run(&["show", task]);
    assert!(success(show.clone()).starts_with(&format!("{task}  Issue  Ready+Unapproved")));
    assert_eq!(stderr(&show), NOTICE);
    // Undecided is not evaluated by show, so its situation carries no mark and no notice.
    let show = f.run(&["show", proposal, "--details"]);
    let text = success(show.clone());
    assert!(
        text.starts_with(&format!("{proposal}  Issue  Undecided  ")),
        "{text}"
    );
    assert!(text.contains("\nApproval: unapproved\n"), "{text}");
    assert!(stderr(&show).is_empty());
    for marker in ["task.marker", "proposal.marker"] {
        assert!(!f.0.join(marker).exists(), "{marker} ran");
    }
}

#[test]
fn own_conditions_run_and_the_same_conditions_from_another_clone_do_not() {
    let (origin, task, proposal) = origin();
    // The clone that set the conditions approved them and runs them.
    let tasks = origin.run(&["tasks"]);
    assert!(!success(tasks.clone()).contains(&task));
    assert!(
        stderr(&tasks).starts_with("No task candidates."),
        "{}",
        stderr(&tasks)
    );
    assert!(success(origin.run(&["proposals"])).is_empty());
    assert!(origin.0.join("task.marker").exists());
    assert!(origin.0.join("proposal.marker").exists());

    // A new clone has the records and none of the approvals.
    let fresh = clone_of(&origin);
    assert_not_run(&fresh, &task, &proposal);

    // A clone that merges a later change by the origin. It approved the task's condition
    // as it was; the origin then replaces it with another command.
    let merging = clone_of(&origin);
    merging.ok(&[
        "condition",
        "set",
        &task,
        "--command",
        &planted("task.marker"),
    ]);
    origin.ok(&[
        "condition",
        "set",
        &task,
        "--command",
        &planted("swapped.marker"),
    ]);
    let later = created(origin.ok(&[
        "capture",
        "--label",
        "chore",
        "--accept",
        "--title",
        "later",
        "--command",
        &planted("later.marker"),
    ]));
    track_store(&origin.0);
    git_commit(&origin.0, &["-qm", "later"]);
    // The merging clone sets its own condition first; it keeps running after the merge.
    let own = created(merging.ok(&[
        "capture",
        "--label",
        "chore",
        "--accept",
        "--title",
        "own",
        "--command",
        "touch own.marker; exit 0",
    ]));
    track_store(&merging.0);
    git_commit(&merging.0, &["-qm", "own"]);
    success(git_integration(
        &merging.0,
        &["pull", "-q", "--no-rebase", "--no-edit", "origin", "main"],
    ));
    let tasks = merging.run(&["tasks"]);
    let rows = success(tasks.clone());
    assert!(
        rows.contains(&format!("{later}  Issue  Ready+Unapproved")),
        "{rows}"
    );
    assert!(
        rows.contains(&format!("{own}  Issue  Ready  chore  own")),
        "{rows}"
    );
    assert!(
        rows.contains(&format!("{task}  Issue  Ready+Unapproved")),
        "{rows}"
    );
    assert!(
        stderr(&tasks).starts_with("Conditions not run: 2 unapproved"),
        "{}",
        stderr(&tasks)
    );
    assert_received(&merging, &task, &proposal);
    assert!(merging.0.join("own.marker").exists());
    for marker in ["later.marker", "swapped.marker"] {
        assert!(!merging.0.join(marker).exists(), "{marker} ran");
    }

    // A repository that checks out a branch it fetched for review.
    let reviewing = Fixture::new();
    git(&reviewing.0, &["init", "-q", "-b", "local"]);
    git(
        &reviewing.0,
        &["fetch", "-q", origin.0.to_str().unwrap(), "main:review"],
    );
    git(&reviewing.0, &["checkout", "-q", "review"]);
    let tasks = reviewing.run(&["tasks"]);
    let rows = success(tasks.clone());
    for id in [&task, &later] {
        assert!(
            rows.contains(&format!("{id}  Issue  Ready+Unapproved")),
            "{rows}"
        );
    }
    assert!(
        stderr(&tasks).starts_with("Conditions not run: 2 unapproved"),
        "{}",
        stderr(&tasks)
    );
    assert_received(&reviewing, &task, &proposal);
    for marker in ["swapped.marker", "later.marker", "proposal.marker"] {
        assert!(!reviewing.0.join(marker).exists(), "{marker} ran");
    }
}

/// `proposals` and `show` on a clone that received the origin's later commit: the proposal and
/// the replaced task condition are not run.
fn assert_received(f: &Fixture, task: &str, proposal: &str) {
    let proposals = f.run(&["proposals"]);
    assert!(
        success(proposals.clone()).contains(&format!("{proposal}  Issue  Undecided+Unapproved"))
    );
    assert_eq!(stderr(&proposals), NOTICE);
    let show = f.run(&["show", task]);
    assert!(success(show.clone()).starts_with(&format!("{task}  Issue  Ready+Unapproved")));
    assert_eq!(stderr(&show), NOTICE);
}

#[test]
fn a_condition_taken_by_resolve_is_not_approved() {
    let (origin, task, _) = origin();
    let other = clone_of(&origin);
    // Both clones change the task: the origin its condition, the other clone its title.
    origin.ok(&[
        "condition",
        "set",
        &task,
        "--command",
        &planted("taken.marker"),
    ]);
    track_store(&origin.0);
    git_commit(&origin.0, &["-qm", "condition"]);
    // The other clone approved the task's original condition, so only the taken one differs.
    other.ok(&[
        "condition",
        "set",
        &task,
        "--command",
        &planted("task.marker"),
    ]);
    other.ok(&["write", &task, "--title", "renamed"]);
    track_store(&other.0);
    git_commit(&other.0, &["-qm", "title"]);
    success(git_integration(
        &other.0,
        &["pull", "-q", "--no-rebase", "--no-edit", "origin", "main"],
    ));
    // Take the origin's head, whose condition this clone never approved.
    let heads = success(other.run(&["resolve", &task]));
    let head = heads
        .lines()
        .find(|line| line.contains("  condition  "))
        .and_then(|line| line.split_whitespace().find(|word| word.len() == 64))
        .unwrap_or_else(|| panic!("{heads}"))
        .to_owned();
    other.ok(&["resolve", &task, "--head", &head]);
    let tasks = other.run(&["tasks"]);
    assert!(success(tasks.clone()).contains(&format!("{task}  Issue  Ready+Unapproved")));
    assert_eq!(stderr(&tasks), NOTICE);
    assert!(!other.0.join("taken.marker").exists());
    let details = success(other.run(&["show", &task, "--details", "--skip-conditions"]));
    assert!(
        details.contains(&format!(
            "\nApproval: unapproved\nDigest: {}\nCommand: touch taken.marker; exit 1\n",
            blake3::hash(planted("taken.marker").as_bytes()).to_hex()
        )),
        "{details}"
    );
}

#[test]
fn a_store_copied_outside_git_does_not_bring_its_approvals() {
    let original = Fixture::new();
    original.init();
    let task = created(original.ok(&[
        "capture",
        "--label",
        "chore",
        "--accept",
        "--title",
        "task",
        "--command",
        "touch task.marker; exit 1",
    ]));
    let proposal = created(original.ok(&[
        "capture",
        "--label",
        "chore",
        "--title",
        "proposal",
        "--command",
        "touch proposal.marker; exit 1",
    ]));
    assert!(success(original.run(&["tasks"])).is_empty());
    assert!(original.0.join("task.marker").exists());
    // The same store, header included, received at another path.
    let copy = Fixture::new();
    success(
        Command::new("cp")
            .args(["-R"])
            .arg(original.0.join(".axon"))
            .arg(copy.0.join(".axon"))
            .output()
            .unwrap(),
    );
    assert_not_run(&copy, &task, &proposal);
    // Setting it here approves it for this management root.
    copy.ok(&[
        "condition",
        "set",
        &task,
        "--command",
        "touch task.marker; exit 1",
    ]);
    assert!(!success(copy.run(&["tasks"])).contains(&task));
    assert!(copy.0.join("task.marker").exists());
}

#[test]
fn same_command_set_again_approves_it_without_a_record() {
    let (origin, task, _) = origin();
    let fresh = clone_of(&origin);
    let before = fresh.record_files();
    let command = planted("task.marker");
    let set = fresh.run(&[
        "condition",
        "set",
        &task,
        "--command",
        &command,
        "-r",
        "checked",
    ]);
    let text = success(set.clone());
    assert_eq!(
        text,
        format!(
            "{task}  No changes  Reason not saved; use axon note add to record it.\n{task}  Allowed condition\n  Digest: {}\n  Command: {command}\n",
            blake3::hash(command.as_bytes()).to_hex()
        )
    );
    assert!(stderr(&set).is_empty());
    assert_eq!(fresh.record_files(), before);
    // Already approved: only the ordinary no-op output.
    assert_eq!(
        fresh.ok(&["condition", "set", &task, "--command", &command]),
        format!("{task}  No changes\n")
    );
    let tasks = fresh.run(&["tasks"]);
    assert!(!success(tasks.clone()).contains(&task));
    assert!(fresh.0.join("task.marker").exists());
}

#[test]
fn unreadable_approvals_run_nothing_and_say_so() {
    let (origin, task, proposal) = origin();
    let approvals = origin.0.join(".git/axon-condition-approvals");
    let digest = blake3::hash(planted("task.marker").as_bytes())
        .to_hex()
        .to_string();
    assert!(fs::read_to_string(&approvals).unwrap().contains(&digest));
    fs::write(&approvals, "not approvals\n").unwrap();
    for marker in ["task.marker", "proposal.marker"] {
        let _ = fs::remove_file(origin.0.join(marker));
    }
    let tasks = origin.run(&["tasks"]);
    assert!(success(tasks.clone()).contains(&format!("{task}  Issue  Ready+Unapproved")));
    let notice = stderr(&tasks);
    assert!(
        notice.starts_with(&format!(
            "Conditions not run: approvals could not be read ({}: the first line is not axon-condition-approvals/v1; fix or remove {} (removing it discards every approval)); every condition was treated as unapproved\n",
            approvals.display(),
            approvals.display()
        )),
        "{notice}"
    );
    assert_eq!(notice.lines().count(), 1);
    // A read whose range holds no command says nothing about approvals.
    let none = origin.run(&["tasks", "--search", "no such title"]);
    assert!(
        stderr(&none).starts_with("No task candidates."),
        "{}",
        stderr(&none)
    );
    // An evaluated show with details says it once, with the evaluating notice only.
    let show = origin.run(&["show", &task, "--details"]);
    assert!(success(show.clone()).contains("\nApproval: unknown\n"));
    assert!(stderr(&show).starts_with("Conditions not run: approvals could not be read ("));
    assert_eq!(stderr(&show).lines().count(), 1);
    // The details show the approval as unknown and say why once.
    let show = origin.run(&["show", &proposal, "--details", "--skip-conditions"]);
    assert!(success(show.clone()).contains("\nApproval: unknown\n"));
    assert!(stderr(&show).starts_with("Approvals could not be read ("));
    assert_eq!(stderr(&show).lines().count(), 1);
    // A new condition is saved, but the unreadable file is not overwritten.
    let set = origin.run(&["condition", "set", &task, "--command", "exit 0"]);
    assert_eq!(success(set.clone()), format!("{task}  Condition updated\n"));
    assert!(
        stderr(&set).starts_with("Warning: approval not saved ("),
        "{}",
        stderr(&set)
    );
    assert!(stderr(&set).contains(&format!(
        "the condition is saved but will not run until approved; after fixing that, approve it with axon condition allow {task} --digest {}",
        blake3::hash(b"exit 0").to_hex()
    )));
    assert_eq!(fs::read_to_string(&approvals).unwrap(), "not approvals\n");
    // The same command again cannot be approved: an error after the no-op output.
    let again = origin.run(&["condition", "set", &task, "--command", "exit 0"]);
    assert!(!again.status.success());
    assert_eq!(
        String::from_utf8(again.stdout.clone()).unwrap(),
        format!("{task}  No changes\n")
    );
    assert!(stderr(&again).starts_with("Error: approval not saved ("));
    for marker in ["task.marker", "proposal.marker"] {
        assert!(!origin.0.join(marker).exists(), "{marker} ran");
    }
}

#[test]
fn details_show_the_approval_digest_and_an_unambiguous_command() {
    let f = Fixture::new();
    f.init();
    let command = "printf '\\n'\ttest \u{201c}x\u{201d} # 済\r\u{1b}";
    let id = created(f.ok(&[
        "capture",
        "--label",
        "chore",
        "--title",
        "shown",
        "--command",
        command,
    ]));
    let text = f.ok(&["show", &id, "--details", "--skip-conditions"]);
    assert!(
        text.contains(&format!(
            "\nApproval: approved\nDigest: {}\nCommand: printf '\\\\n'\\ttest \\u{{201c}}x\\u{{201d}} # \\u{{6e08}}\\r\\x1b\n",
            blake3::hash(command.as_bytes()).to_hex()
        )),
        "{text}"
    );
    // Without --details, --skip-conditions does not look at approvals.
    assert!(
        !f.ok(&["show", &id, "--skip-conditions"])
            .contains("Approval:")
    );
}

/// Approvals are not read from a place a received directory can supply: a directory shaped
/// like a bare repository whose `core.worktree` names the worktree, or a Git directory kept
/// inside the worktree, is not the worktree's own `.git`.
#[test]
fn approvals_in_a_git_directory_the_worktree_does_not_own_are_not_read() {
    let f = Fixture::new();
    let shaped = f.0.join("received");
    git(&f.0, &["init", "-q", "--bare", "received"]);
    let work = shaped.join("work");
    fs::create_dir(&work).unwrap();
    git(&shaped, &["config", "core.bare", "false"]);
    git(
        &shaped,
        &["config", "core.worktree", work.to_str().unwrap()],
    );
    let separate = Fixture::new();
    git(
        &separate.0,
        &[
            "init",
            "-q",
            "--separate-git-dir",
            separate.0.join("inner").to_str().unwrap(),
        ],
    );
    // Git skips an empty `.git` and takes a bare-shaped parent whose core.worktree names the
    // directory: only the Git directory not coming from `.git` gives that away.
    let masked = Fixture::new();
    let outer = masked.0.join("received");
    git(&masked.0, &["init", "-q", "--bare", "received"]);
    let inner = outer.join("work");
    fs::create_dir_all(inner.join(".git")).unwrap();
    git(&outer, &["config", "core.bare", "false"]);
    git(
        &outer,
        &["config", "core.worktree", inner.to_str().unwrap()],
    );
    // A `.git` that links to a Git directory elsewhere inside the worktree.
    let linked = Fixture::new();
    git(&linked.0, &["init", "-q"]);
    fs::rename(linked.0.join(".git"), linked.0.join("hidden")).unwrap();
    std::os::unix::fs::symlink("hidden", linked.0.join(".git")).unwrap();
    for (root, git_dir, reason) in [
        (
            work.as_path(),
            shaped.clone(),
            "the worktree root has no .git",
        ),
        (
            inner.as_path(),
            outer.clone(),
            "the Git directory does not come from .git in the worktree root",
        ),
        (
            linked.0.as_path(),
            linked.0.join("hidden"),
            "the common Git directory is inside the worktree but is not its .git",
        ),
        (
            separate.0.as_path(),
            separate.0.join("inner"),
            "the common Git directory is inside the worktree but is not its .git",
        ),
    ] {
        success(command(root).args(["init", "t"]).output().unwrap());
        let condition = "touch planted.marker; exit 1";
        let capture = command(root)
            .args([
                "capture",
                "--label",
                "chore",
                "--accept",
                "--title",
                "t",
                "--command",
                condition,
            ])
            .output()
            .unwrap();
        let id = created(success(capture.clone()));
        assert!(
            stderr(&capture).starts_with(&format!("Warning: approval not saved ({reason} (")),
            "{}",
            stderr(&capture)
        );
        // An approvals file planted where Git points is not read.
        fs::write(
            git_dir.join("axon-condition-approvals"),
            format!(
                "axon-condition-approvals/v1\n{}\n",
                blake3::hash(condition.as_bytes()).to_hex()
            ),
        )
        .unwrap();
        let tasks = command(root).args(["tasks"]).output().unwrap();
        assert!(success(tasks.clone()).contains(&format!("{id}  Issue  Ready+Unapproved")));
        assert!(
            stderr(&tasks).starts_with(&format!(
                "Conditions not run: approvals could not be read ({reason} ("
            )),
            "{}",
            stderr(&tasks)
        );
        assert!(!root.join("planted.marker").exists());
    }
}

#[test]
fn an_unapproved_ancestor_marks_only_its_own_row() {
    let origin = Fixture::new();
    git(&origin.0, &["init", "-q", "-b", "main"]);
    git_commit(&origin.0, &["-q", "--allow-empty", "-m", "base"]);
    origin.init();
    let group = created(origin.ok(&[
        "capture",
        "--label",
        "chore",
        "--kind",
        "group",
        "--accept",
        "--title",
        "group",
        "--command",
        "exit 0",
    ]));
    let child = created(origin.ok(&[
        "capture", "--label", "chore", "--accept", "--title", "child", "--parent", &group,
    ]));
    let gated = created(origin.ok(&[
        "capture",
        "--label",
        "chore",
        "--accept",
        "--title",
        "gated",
        "--parent",
        &group,
        "--command",
        "touch gated.marker; exit 1",
    ]));
    track_store(&origin.0);
    git_commit(&origin.0, &["-qm", "plan"]);
    let fresh = clone_of(&origin);
    // The child's condition is approved here; the Group's is not.
    fresh.ok(&[
        "condition",
        "set",
        &gated,
        "--command",
        "touch gated.marker; exit 1",
    ]);
    let tasks = fresh.run(&["tasks"]);
    let rows = success(tasks.clone());
    assert!(
        rows.contains(&format!("{group}  Group  Ready+Unapproved")),
        "{rows}"
    );
    assert!(
        rows.contains(&format!("{child}  Issue  Ready  chore  child")),
        "{rows}"
    );
    // Below the unapproved Group the approved condition still runs and hides its Issue.
    assert!(!rows.contains(&gated), "{rows}");
    assert!(fresh.0.join("gated.marker").exists());
    assert_eq!(stderr(&tasks), NOTICE);
    // Only Issues are listed; the Group they evaluate is counted though not shown.
    let issues = fresh.run(&["tasks", "--kind", "issue"]);
    assert!(!success(issues.clone()).contains(&group));
    assert_eq!(stderr(&issues), NOTICE);
    // The child's own situation carries no mark from its unapproved parent.
    let show = success(fresh.run(&["show", &child, "--details"]));
    assert!(
        show.starts_with(&format!("{child}  Issue  Ready  chore")),
        "{show}"
    );
    // A child whose own condition goes unapproved is marked in the Group's tree.
    origin.ok(&["condition", "set", &child, "--command", "exit 0"]);
    track_store(&origin.0);
    git_commit(&origin.0, &["-qm", "child condition"]);
    success(git_integration(
        &fresh.0,
        &["pull", "-q", "--no-rebase", "--no-edit", "origin", "main"],
    ));
    let show = fresh.run(&["show", &group]);
    let text = success(show.clone());
    assert!(
        text.starts_with(&format!("{group}  Group  Ready+Unapproved")),
        "{text}"
    );
    assert!(
        text.contains(&format!("{child}  Issue  Ready+Unapproved  chore  child")),
        "{text}"
    );
    assert!(
        stderr(&show).starts_with("Conditions not run: 2 unapproved"),
        "{}",
        stderr(&show)
    );
}

#[test]
fn outside_git_a_relative_home_leaves_every_condition_unapproved() {
    let f = Fixture::new();
    f.init();
    let id = f.accepted("task");
    // Set it with a usable home, so the store holds a condition this root approved.
    f.ok(&[
        "condition",
        "set",
        &id,
        "--command",
        "touch task.marker; exit 1",
    ]);
    let tasks = f
        .command()
        .env("HOME", "relative-home")
        .args(["tasks"])
        .output()
        .unwrap();
    assert!(success(tasks.clone()).contains(&format!("{id}  Issue  Ready+Unapproved")));
    let notice = stderr(&tasks);
    assert!(
        notice.starts_with("Conditions not run: approvals could not be read (no absolute state directory or local data directory is known; set HOME to an absolute path)"),
        "{notice}"
    );
    assert!(!f.0.join("task.marker").exists());
    assert!(!f.0.join("relative-home").exists());
}

#[test]
fn approval_diagnostics_escape_terminal_controls_in_paths() {
    let f = Fixture::new();
    let root = f.0.join("evil\u{1b}[31mred");
    fs::create_dir(&root).unwrap();
    git(&root, &["init", "-q"]);
    success(command(&root).args(["init", "t"]).output().unwrap());
    let id = created(success(
        command(&root)
            .args([
                "capture",
                "--label",
                "chore",
                "--accept",
                "--title",
                "t",
                "--command",
                "exit 0",
            ])
            .output()
            .unwrap(),
    ));
    fs::write(root.join(".git/axon-condition-approvals"), "bad\n").unwrap();
    for args in [
        vec!["tasks"],
        vec!["show", &id, "--details", "--skip-conditions"],
    ] {
        let output = command(&root).args(&args).output().unwrap();
        let text = stderr(&output);
        assert!(!text.contains('\u{1b}'), "{args:?}: {text:?}");
        assert!(text.contains("evil\\x1b[31mred"), "{args:?}: {text:?}");
        assert_eq!(text.lines().count(), 1, "{args:?}: {text:?}");
    }
    let set = command(&root)
        .args(["condition", "set", &id, "--command", "exit 0"])
        .output()
        .unwrap();
    assert!(!stderr(&set).contains('\u{1b}'));
}

#[test]
fn outside_git_another_store_at_the_same_path_does_not_inherit_approvals() {
    let f = Fixture::new();
    f.init();
    let command = "touch planted.marker; exit 1";
    f.ok(&[
        "capture",
        "--label",
        "chore",
        "--accept",
        "--title",
        "mine",
        "--command",
        command,
    ]);
    // Another store, made elsewhere with the same command, is put at this path.
    let other = Fixture::new();
    other.init();
    let theirs = created(other.ok(&[
        "capture",
        "--label",
        "chore",
        "--accept",
        "--title",
        "theirs",
        "--command",
        command,
    ]));
    fs::remove_dir_all(f.0.join(".axon")).unwrap();
    fs::rename(other.0.join(".axon"), f.0.join(".axon")).unwrap();
    let tasks = f.run(&["tasks"]);
    assert!(success(tasks.clone()).contains(&format!("{theirs}  Issue  Ready+Unapproved")));
    assert_eq!(stderr(&tasks), NOTICE);
    assert!(!f.0.join("planted.marker").exists());
}

#[test]
fn a_dot_git_link_to_a_gitfile_is_trusted() {
    let f = Fixture::new();
    let root = f.0.join("work");
    fs::create_dir(&root).unwrap();
    git(
        &root,
        &[
            "init",
            "-q",
            "--separate-git-dir",
            f.0.join("store.git").to_str().unwrap(),
        ],
    );
    fs::rename(root.join(".git"), f.0.join("gitfile")).unwrap();
    // Git reads a relative gitdir against the worktree root, as it does for `.git` itself.
    fs::write(f.0.join("gitfile"), "gitdir: ../store.git\n").unwrap();
    std::os::unix::fs::symlink(f.0.join("gitfile"), root.join(".git")).unwrap();
    success(command(&root).args(["init", "t"]).output().unwrap());
    let capture = command(&root)
        .args([
            "capture",
            "--label",
            "chore",
            "--accept",
            "--title",
            "t",
            "--command",
            "touch ran.marker; exit 1",
        ])
        .output()
        .unwrap();
    assert!(stderr(&capture).is_empty(), "{}", stderr(&capture));
    let tasks = command(&root).args(["tasks"]).output().unwrap();
    assert!(success(tasks.clone()).is_empty());
    assert!(root.join("ran.marker").exists());
}

fn digest_of(command: &str) -> String {
    blake3::hash(command.as_bytes()).to_hex().to_string()
}

#[test]
fn unapproved_conditions_are_listed_and_run_once_allowed() {
    let (origin, task, proposal) = origin();
    let fresh = clone_of(&origin);
    let task_command = planted("task.marker");
    let proposal_command = planted("proposal.marker");
    let listed = fresh.ok(&["condition", "unapproved"]);
    assert_eq!(
        listed,
        format!(
            "{task}  Issue  Ready  chore  task\n  Digest: {}\n  Command: {task_command}\n{proposal}  Issue  Undecided  chore  proposal\n  Digest: {}\n  Command: {proposal_command}\n",
            digest_of(&task_command),
            digest_of(&proposal_command)
        )
    );
    let records = fresh.record_files();
    let unrun = |f: &Fixture| {
        for marker in ["task.marker", "proposal.marker"] {
            assert!(!f.0.join(marker).exists(), "{marker} ran");
        }
    };
    // Listing and approving run nothing.
    unrun(&fresh);
    let allowed = fresh.ok(&[
        "condition",
        "allow",
        &task,
        "--digest",
        &digest_of(&task_command),
    ]);
    assert_eq!(
        allowed,
        format!(
            "{task}  Allowed condition\n  Digest: {}\n  Command: {task_command}\n",
            digest_of(&task_command)
        )
    );
    assert_eq!(
        fresh.ok(&["condition", "allow", &task]),
        format!(
            "{task}  Condition already allowed\n  Digest: {}\n  Command: {task_command}\n",
            digest_of(&task_command)
        )
    );
    unrun(&fresh);
    // The approval is not a record and leaves no history.
    assert_eq!(fresh.record_files(), records);
    assert!(!fresh.ok(&["log", &task]).contains("llow"));
    let tasks = fresh.run(&["tasks"]);
    assert!(!success(tasks.clone()).contains(&task));
    assert!(fresh.0.join("task.marker").exists());
    assert!(stderr(&tasks).starts_with("No task candidates."));
    assert!(!fresh.ok(&["condition", "unapproved"]).contains(&task));
    // A terminal Entity's condition can be approved too.
    fresh.ok(&["cancel", &proposal]);
    fresh.ok(&["condition", "allow", &proposal]);
    let none = fresh.run(&["condition", "unapproved"]);
    assert!(success(none.clone()).is_empty());
    assert!(stderr(&none).starts_with("No unapproved conditions."));
}

#[test]
fn allow_refuses_what_it_cannot_approve_as_checked() {
    let (origin, task, proposal) = origin();
    let other = clone_of(&origin);
    let approvals = other.0.join(".git/axon-condition-approvals");
    let command = planted("task.marker");
    // A suffix is not resolved.
    let suffix = &task[task.len() - 6..];
    let refused = failure(other.run(&["condition", "allow", suffix]));
    assert!(
        refused.contains("condition allow takes only a complete ID"),
        "{refused}"
    );
    // A digest of another command shows the current one and approves nothing.
    let refused = failure(other.run(&[
        "condition",
        "allow",
        &task,
        "--digest",
        &digest_of("something else"),
    ]));
    assert!(
        refused.contains(&format!(
            "  Digest: {}\n  Command: {command}",
            digest_of(&command)
        )),
        "{refused}"
    );
    assert!(!approvals.exists());
    // A malformed digest is a usage error.
    let usage = other.run(&["condition", "allow", &task, "--digest", "ABC"]);
    assert_eq!(usage.status.code(), Some(2));
    // An Entity without a condition.
    let bare = other.accepted("bare");
    assert!(failure(other.run(&["condition", "allow", &bare])).contains("has no condition"));
    // A conflicted Entity is refused; another Entity can still be approved meanwhile.
    origin.ok(&["write", &task, "--title", "origin"]);
    track_store(&origin.0);
    git_commit(&origin.0, &["-qm", "origin title"]);
    other.ok(&["write", &task, "--title", "other"]);
    track_store(&other.0);
    git_commit(&other.0, &["-qm", "other title"]);
    success(git_integration(
        &other.0,
        &["pull", "-q", "--no-rebase", "--no-edit", "origin", "main"],
    ));
    let refused = failure(other.run(&["condition", "allow", &task]));
    assert!(
        refused.contains(&format!(
            "is conflicted; pick a head with axon resolve {task}"
        )),
        "{refused}"
    );
    assert!(
        other
            .ok(&["condition", "allow", &proposal])
            .contains("Allowed condition")
    );
    // The conflicted Entity is not listed.
    let listed = other.ok(&["condition", "unapproved"]);
    assert!(!listed.contains(&task), "{listed}");
    // Unreadable approvals: both commands refuse and the file is kept.
    fs::write(&approvals, "bad\n").unwrap();
    assert!(
        failure(other.run(&["condition", "unapproved"])).contains("approvals could not be read")
    );
    assert!(failure(other.run(&["condition", "allow", &proposal])).contains("not applied"));
    assert_eq!(fs::read_to_string(&approvals).unwrap(), "bad\n");
}

#[test]
fn unapproved_shows_controls_in_commands_escaped() {
    let origin = Fixture::new();
    git(&origin.0, &["init", "-q", "-b", "main"]);
    git_commit(&origin.0, &["-q", "--allow-empty", "-m", "base"]);
    origin.init();
    let command = "printf '\u{1b}]0;title\u{7}'\nexit 0";
    let id = created(origin.ok(&[
        "capture",
        "--label",
        "chore",
        "--title",
        "t",
        "--command",
        command,
    ]));
    track_store(&origin.0);
    git_commit(&origin.0, &["-qm", "plan"]);
    let fresh = clone_of(&origin);
    let listed = fresh.ok(&["condition", "unapproved"]);
    assert!(!listed.contains('\u{1b}') && !listed.contains('\u{7}'));
    assert!(
        listed.ends_with("  Command: printf '\\x1b]0;title\\x07'\\nexit 0\n"),
        "{listed}"
    );
    let escaped = "  Command: printf '\\x1b]0;title\\x07'\\nexit 0";
    // The refusal names the current command in the same escaped form.
    let refused = failure(fresh.run(&["condition", "allow", &id, "--digest", &digest_of("other")]));
    assert!(!refused.contains('\u{7}'));
    assert!(!refused.contains("\\x1b["), "{refused}");
    assert!(refused.trim_end().ends_with(escaped), "{refused}");
    let allowed = fresh.ok(&["condition", "allow", &id, "--digest", &digest_of(command)]);
    assert!(!allowed.contains('\u{1b}') && !allowed.contains('\u{7}'));
    assert!(allowed.starts_with(&format!("{id}  Allowed condition\n")));
    assert!(allowed.ends_with(&format!("{escaped}\n")), "{allowed}");
}

#[test]
fn unapproved_needs_readable_approvals_even_without_conditions() {
    let f = Fixture::new();
    f.init();
    f.accepted("plain");
    let listed = f
        .command()
        .env("HOME", "relative-home")
        .args(["condition", "unapproved"])
        .output()
        .unwrap();
    assert!(failure(listed).contains("approvals could not be read"));
}

#[test]
fn allow_refuses_an_id_that_also_ends_another_id() {
    let f = Fixture::new();
    f.ok(&["init", "my-proj"]);
    let mine = f.accepted("mine");
    f.ok(&["condition", "set", &mine, "--command", "exit 0"]);
    // Someone else's Entity whose complete ID is the end of mine, as a record could bring.
    let theirs = mine.strip_prefix("my-").unwrap().to_owned();
    f.publish(vec![registration(&f.records(), &theirs)]);
    let refused = failure(f.run(&["condition", "allow", &theirs]));
    assert!(
        refused.contains(&format!("{theirs} is also the end of {mine}")),
        "{refused}"
    );
    // The longer ID is not the end of anything else and is approved.
    assert!(
        f.ok(&["condition", "allow", &mine])
            .contains("Condition already allowed")
    );
}

#[test]
fn one_line_values_show_line_and_paragraph_separators() {
    let f = Fixture::new();
    f.init();
    let fake = "\u{2028}  Digest: 0000\u{2029}  Command: true";
    let group = created(f.ok(&[
        "capture",
        "--label",
        "chore",
        "--kind",
        "group",
        "--accept",
        "--title",
        &format!("group{fake}"),
    ]));
    let dependency = created(f.ok(&[
        "capture",
        "--label",
        "chore",
        "--accept",
        "--title",
        &format!("dep{fake}"),
    ]));
    let issue = created(f.ok(&[
        "capture",
        "--label",
        "chore",
        "--accept",
        "--title",
        &format!("issue{fake}"),
        "--parent",
        &group,
        "--needs",
        &dependency,
        "--command",
        &format!("exit 0 #{fake}"),
    ]));
    created(f.ok(&[
        "capture",
        "--label",
        "chore",
        "--title",
        &format!("proposal{fake}"),
    ]));
    f.ok(&["label", "set", &issue, "feat", "-r", &format!("why{fake}")]);
    success(
        f.command()
            .env("AXON_ACTOR", "tester")
            .env("AXON_SESSION_ID", format!("session{fake}"))
            .args(["note", "add", &issue, "-m", "n"])
            .output()
            .unwrap(),
    );
    let records = f.record_files();
    // Another home has no approvals, so the condition is listed as unapproved there.
    let elsewhere = Fixture::new();
    let reads: Vec<(Vec<&str>, bool)> = vec![
        (vec!["list"], false),
        (vec!["tasks"], false),
        (vec!["proposals"], false),
        (vec!["show", &group], false),
        (vec!["show", &issue], false),
        (vec!["show", &issue, "--details"], false),
        (
            vec!["show", &dependency, "--details", "--skip-conditions"],
            false,
        ),
        (vec!["log", &issue, "--recorder-details"], false),
        (vec!["note", "list", &issue, "--recorder-details"], false),
        (vec!["condition", "unapproved"], true),
    ];
    for (args, other_home) in reads {
        let mut command = f.command();
        if other_home {
            command.env("HOME", &elsewhere.0);
        }
        let output = command.args(&args).output().unwrap();
        let stdout = String::from_utf8(output.stdout.clone()).unwrap();
        assert!(!stdout.is_empty(), "{args:?}: nothing to check");
        let text = stdout + &stderr(&output);
        assert!(
            !text.contains('\u{2028}') && !text.contains('\u{2029}'),
            "{args:?}: {text:?}"
        );
    }
    let shown = f.ok(&["show", &issue, "--details"]);
    for expected in [
        "issue\\u{2028}  Digest: 0000\\u{2029}  Command: true",
        "Condition: exit 0 #\\u{2028}",
    ] {
        assert!(shown.contains(expected), "{shown}");
    }
    let log = f.ok(&["log", &issue, "--recorder-details"]);
    assert!(log.contains("why\\u{2028}"), "{log}");
    let notes = f.ok(&["note", "list", &issue, "--recorder-details"]);
    assert!(notes.contains("session\\u{2028}"), "{notes}");
    assert!(f.ok(&["proposals"]).contains("proposal\\u{2028}"));
    // The stored values keep the separators.
    assert_eq!(f.record_files(), records);
    assert!(f.current(&issue).title.contains('\u{2028}'));
}
