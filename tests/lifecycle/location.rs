//! Where `axon init` puts a store, and which store an operation reaches inside Git.
use super::*;

fn header(root: &Path) -> PathBuf {
    root.join(".axon/header.json")
}
/// The literal line the init output shows for the ignored operation.
fn ignore_line(output: &str) -> String {
    output
        .lines()
        .skip_while(|line| !line.starts_with("To keep the store out of Git"))
        .find(|line| line.starts_with("       "))
        .expect("a displayed ignore line")
        .trim_start()
        .to_owned()
}
/// The literal lines the init output shows for the tracked operation.
fn track_lines(output: &str) -> Vec<String> {
    output
        .lines()
        .skip_while(|line| !line.starts_with("To track the store in Git"))
        .skip(1)
        .skip_while(|line| !line.starts_with("       "))
        .take_while(|line| line.starts_with("       "))
        .map(|line| line.trim_start().to_owned())
        .collect()
}
/// The names inside the management directory, sorted.
fn management_entries(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(root.join(".axon"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}
/// Run a displayed step as the line itself spells it. The identity a commit needs comes from
/// the isolated environment, never from editing the displayed command.
fn run_step(root: &Path, line: &str) {
    let mut command = Command::new("sh");
    command.current_dir(root).args(["-c", line]);
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("GIT_") {
            command.env_remove(name);
        }
    }
    success(
        command
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.com")
            .output()
            .unwrap(),
    );
}
/// A repository with one commit, so worktrees and branches can be added.
fn repository() -> Fixture {
    let f = Fixture::new();
    git(&f.0, &["init", "-q"]);
    git_commit(&f.0, &["-q", "--allow-empty", "-m", "base"]);
    f
}
fn created(output: &str) -> String {
    output.split_whitespace().next().unwrap().into()
}
/// The branch the test's own branches are cut from.
fn head_branch(root: &Path) -> String {
    String::from_utf8(git_output(root, &["rev-parse", "--abbrev-ref", "HEAD"]).stdout)
        .unwrap()
        .trim()
        .to_owned()
}
/// A merge that may leave conflicts, with a fixed identity and no hooks.
fn merge_branch(root: &Path, branch: &str) -> Output {
    git_integration(root, &["merge", "--no-edit", branch])
}
/// Replace the index entries for `paths` with unresolved stages of their working tree files.
fn make_index_unmerged(root: &Path, paths: &[&str]) {
    let mut stages = String::new();
    for path in paths {
        let blob =
            String::from_utf8(git_output(root, &["hash-object", "-w", path]).stdout).unwrap();
        let blob = blob.trim();
        for stage in 1..=3 {
            stages.push_str(&format!("100644 {blob} {stage}\t{path}\0"));
        }
        git(root, &["update-index", "--force-remove", path]);
    }
    let mut child = isolated_git(root)
        .args(["update-index", "-z", "--index-info"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stages.as_bytes())
        .unwrap();
    assert!(child.wait().unwrap().success());
}

#[test]
fn init_creates_the_store_files_only_and_leaves_them_untracked() {
    let f = repository();
    fs::write(f.0.join(".gitignore"), "/target\n").unwrap();
    fs::write(f.0.join(".gitattributes"), "*.txt text\n").unwrap();
    let repository_attributes = f.0.join(".git/info/attributes");
    let exclude = f.0.join(".git/info/exclude");
    fs::create_dir_all(repository_attributes.parent().unwrap()).unwrap();
    fs::write(&repository_attributes, "*.bin binary\n").unwrap();
    let exclude_before = fs::read(&exclude).ok();
    let output = f.ok(&["init", "t"]);
    assert!(
        output.starts_with(&format!("Initialized {}\n", header(&f.0).display())),
        "{output}"
    );
    // The records directory, the header, the ignore file and the attributes file are the
    // whole of what init leaves behind: no lock or temporary file from the initialization.
    assert_eq!(
        management_entries(&f.0),
        [".gitattributes", ".gitignore", "header.json", "records"]
    );
    assert!(header(&f.0).is_file());
    assert_eq!(
        fs::read_to_string(f.0.join(".axon/.gitignore")).unwrap(),
        "*.lock\n*.tmp\n"
    );
    assert_eq!(
        fs::read_to_string(f.0.join(".axon/.gitattributes")).unwrap(),
        "* -text\n"
    );
    assert_eq!(fs::read_dir(f.0.join(".axon/records")).unwrap().count(), 0);
    // Both operations are explained, and neither is applied.
    assert!(
        output.contains(&format!(
            "Unless an ignore rule of yours already covers it, Git sees .axon as untracked\nand git add -A would commit the store. Check which applies with:\n       git check-ignore -v .axon/header.json\nChoose one way to use the store; do not mix the two in one repository. Run the\nsteps in\n{}:",
            f.0.display()
        )),
        "{output}"
    );
    assert_eq!(ignore_line(&output), ".axon/");
    assert!(
        output.contains(
            "To track the store in Git and merge it between branches instead, first remove\nany ignore rule outside .axon that covers the directory, then stage and commit\nthe store; .axon/.gitignore already excludes locks and temporary files:"
        ),
        "{output}"
    );
    assert_eq!(
        track_lines(&output),
        ["git add .axon", "git commit -m \"Track the Axon store\""]
    );
    assert!(output.contains("not with\ngit revert"), "{output}");
    for absent in ["merge=axon", "merge.axon.driver", "union", "state.jsonl"] {
        assert!(!output.contains(absent), "{absent}: {output}");
    }
    // The only attributes file the output speaks of is the store's own.
    assert_eq!(
        output.matches(".gitattributes").count(),
        output.matches(".axon/.gitattributes").count(),
        "{output}"
    );
    assert!(
        output.contains(".axon/.gitattributes only keeps Git from converting their line endings"),
        "{output}"
    );
    // Files the user owns keep their bytes, and absent ones stay absent.
    assert_eq!(
        fs::read(f.0.join(".gitignore")).unwrap(),
        b"/target\n",
        "the repository .gitignore must not be edited"
    );
    assert_eq!(
        fs::read(f.0.join(".gitattributes")).unwrap(),
        b"*.txt text\n"
    );
    assert_eq!(
        fs::read(&repository_attributes).unwrap(),
        b"*.bin binary\n",
        "the repository attributes must not be edited"
    );
    assert_eq!(
        fs::read(&exclude).ok(),
        exclude_before,
        "the repository ignore file must not be edited"
    );
    assert!(
        git_output(&f.0, &["config", "--get-regexp", "^merge\\."])
            .stdout
            .is_empty()
    );
    let status = String::from_utf8(git_output(&f.0, &["status", "--porcelain"]).stdout).unwrap();
    assert!(
        status.lines().any(|line| line == "?? .axon/"),
        "the store is untracked, not ignored, after init: {status}"
    );

    // Nothing Git reads is created either, in a repository that has none of it.
    let fresh = repository();
    let status =
        String::from_utf8(git_output(&fresh.0, &["status", "--porcelain"]).stdout).unwrap();
    assert!(status.is_empty(), "{status}");
    fresh.ok(&["init", "t"]);
    assert!(!fresh.0.join(".gitignore").exists());
    assert!(!fresh.0.join(".gitattributes").exists());
    assert!(!fresh.0.join(".git/info/attributes").exists());
    let status =
        String::from_utf8(git_output(&fresh.0, &["status", "--porcelain"]).stdout).unwrap();
    assert_eq!(status, "?? .axon/\n");
}

#[test]
fn the_displayed_ignore_line_keeps_the_store_out_of_git() {
    let f = repository();
    let output = f.ok(&["init", "t"]);
    let line = ignore_line(&output);
    assert_eq!(line, ".axon/");
    ignore_store(&f.0, &line);
    let status = String::from_utf8(git_output(&f.0, &["status", "--porcelain"]).stdout).unwrap();
    assert!(status.is_empty(), "{status}");
    assert!(
        git_output(&f.0, &["check-ignore", "-q", ".axon/header.json"])
            .status
            .success()
    );
}

/// Git's own behaviour, and the reason the output warns about mixing the two operations: Git
/// refuses to overwrite an untracked store, and replaces an ignored one without warning.
#[test]
fn git_guards_an_untracked_store_against_a_checkout_but_not_an_ignored_one() {
    let f = repository();
    let base = head_branch(&f.0);
    git(&f.0, &["checkout", "-qb", "tracked"]);
    f.init();
    f.accepted("saved on the branch that tracks the store");
    track_store(&f.0);
    git_commit(&f.0, &["-qm", "track the store"]);
    let tracked = fs::read(header(&f.0)).unwrap();
    git(&f.0, &["checkout", "-q", &base]);
    assert!(!header(&f.0).exists());
    f.init();
    f.accepted("saved in the store this branch never committed");
    let untracked = fs::read(header(&f.0)).unwrap();
    assert_ne!(untracked, tracked);

    // Untracked, so Git stops instead of losing records it never saved.
    let refused = git_output(&f.0, &["checkout", "tracked"]);
    assert!(!refused.status.success());
    let reason = String::from_utf8_lossy(&refused.stderr).into_owned();
    assert!(reason.contains(".axon/header.json"), "{reason}");
    assert_eq!(fs::read(header(&f.0)).unwrap(), untracked);
    assert_eq!(head_branch(&f.0), base);

    // Ignored, so the same checkout succeeds and the store is the other branch's.
    ignore_store(&f.0, ".axon/");
    git(&f.0, &["checkout", "-q", "tracked"]);
    let after = fs::read(header(&f.0)).unwrap();
    assert_ne!(after, untracked);
    assert_eq!(after, tracked);
}

#[test]
fn the_displayed_steps_track_the_store_and_git_merges_branches_without_configuration() {
    let f = repository();
    let output = f.ok(&["init", "t"]);
    let mine = f.accepted("mine");
    let theirs = f.accepted("theirs");
    for line in track_lines(&output) {
        run_step(&f.0, &line);
    }
    // The commit is part of the steps, so they alone leave the tree clean and the files saved.
    let status = String::from_utf8(git_output(&f.0, &["status", "--porcelain"]).stdout).unwrap();
    assert!(status.is_empty(), "{status}");
    let tracked =
        String::from_utf8(git_output(&f.0, &["ls-tree", "-r", "--name-only", "HEAD"]).stdout)
            .unwrap();
    for path in [
        ".axon/header.json",
        ".axon/.gitignore",
        ".axon/.gitattributes",
    ] {
        assert!(tracked.lines().any(|line| line == path), "{tracked}");
    }
    assert_eq!(tracked.matches(".axon/records/").count(), 2);
    assert!(!tracked.contains(".lock") && !tracked.contains(".tmp"));
    assert!(!f.0.join(".gitattributes").exists());
    let base = head_branch(&f.0);
    git(&f.0, &["checkout", "-qb", "left"]);
    f.ok(&["start", &mine]);
    git(&f.0, &["add", ".axon"]);
    git_commit(&f.0, &["-qm", "start mine"]);
    git(&f.0, &["checkout", "-q", &base]);
    git(&f.0, &["checkout", "-qb", "right"]);
    f.ok(&["start", &theirs]);
    git(&f.0, &["add", ".axon"]);
    git_commit(&f.0, &["-qm", "start theirs"]);
    success(merge_branch(&f.0, "left"));
    assert!(git_output(&f.0, &["ls-files", "-u"]).stdout.is_empty());
    let check = f.run(&["storage", "check"]);
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
    for id in [&mine, &theirs] {
        assert!(f.ok(&["show", id]).contains("InProgress"), "{id}");
    }
}

const UNMERGED: &str = "unmerged Git index under .axon/ in the worktree";
/// The whole unmerged diagnostic for a header left unmerged in the index of `worktree`.
fn unmerged_header(worktree: &Path) -> String {
    format!(
        "{UNMERGED} {}; resolve and stage these paths before normal operations\nUnmerged: .axon/header.json\n",
        fs::canonicalize(worktree).unwrap().display()
    )
}
/// Branches `left` and `right`, each initializing a store of its own from the returned base
/// branch, so merging one into the other stops with the header added on both sides.
fn conflicting_stores(f: &Fixture) -> String {
    let base = head_branch(&f.0);
    for branch in ["left", "right"] {
        git(&f.0, &["checkout", "-q", &base]);
        git(&f.0, &["checkout", "-qb", branch]);
        f.init();
        f.accepted(branch);
        track_store(&f.0);
        git_commit(&f.0, &["-qm", branch]);
    }
    base
}

#[test]
fn storage_check_reports_an_unmerged_index_with_or_without_an_explicit_root() {
    let f = repository();
    conflicting_stores(&f);
    assert!(!merge_branch(&f.0, "left").status.success());
    let unmerged = String::from_utf8(git_output(&f.0, &["status", "--porcelain"]).stdout).unwrap();
    assert!(unmerged.contains("AA .axon/header.json"), "{unmerged}");
    let without_root = failure(f.run(&["storage", "check"]));
    assert!(
        without_root.ends_with(&unmerged_header(&f.0)),
        "{without_root}"
    );
    for root in [".", f.0.to_str().unwrap()] {
        assert_eq!(failure(f.run(&["storage", "check", root])), without_root);
    }
    let outside = Fixture::new();
    outside.init();
    // The index checked is ROOT's, whichever directory the command runs in.
    let from_outside = failure(
        command(&outside.0)
            .args(["storage", "check", f.0.to_str().unwrap()])
            .output()
            .unwrap(),
    );
    assert_eq!(from_outside, without_root);
    // A Git boundary that discovery rejects is rejected for an explicit root too.
    let error = failure(f.run(&["storage", "check", ".git"]));
    assert!(error.contains("Git discovery failed"), "{error}");
    // A root outside Git is not checked against the index of the directory the command runs in.
    let check = f.run(&["storage", "check", outside.0.to_str().unwrap()]);
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
}

#[test]
fn an_unmerged_index_is_reported_alone_when_it_removed_the_header_from_the_working_tree() {
    let f = repository();
    let base = conflicting_stores(&f);
    assert!(!merge_branch(&f.0, "left").status.success());
    // The header stays unmerged in the index while the working tree has only records.
    fs::remove_file(header(&f.0)).unwrap();
    let unmerged = String::from_utf8(git_output(&f.0, &["status", "--porcelain"]).stdout).unwrap();
    assert!(unmerged.contains("AA .axon/header.json"), "{unmerged}");
    let without_root = failure(f.run(&["storage", "check"]));
    // The header returns once Git resolves the index, so its absence is not reported as damage.
    let diagnostic = unmerged_header(&f.0);
    assert!(without_root.ends_with(&diagnostic), "{without_root}");
    assert!(!without_root.contains("Corrupt"), "{without_root}");
    for root in [".", f.0.to_str().unwrap()] {
        assert_eq!(failure(f.run(&["storage", "check", root])), without_root);
    }
    // The index checked is ROOT's, whichever directory the command runs in.
    let outside = Fixture::new();
    let from_outside = failure(
        command(&outside.0)
            .args(["storage", "check", f.0.to_str().unwrap()])
            .output()
            .unwrap(),
    );
    assert_eq!(from_outside, without_root);
    for args in [
        vec!["list"],
        vec!["capture", "--label", "chore", "--title", "rejected"],
    ] {
        let error = failure(f.run(&args));
        assert!(error.ends_with(&diagnostic), "{args:?}: {error}");
        assert!(!error.contains("not a store"), "{args:?}: {error}");
    }
    // A linked worktree without a store of its own reaches the main worktree's index, and
    // neither falls through past that store nor initializes a second one beside it. The
    // diagnostic names the main worktree, whose index has to be resolved.
    let linked = Fixture::new();
    git(
        &f.0,
        &["worktree", "add", "-q", linked.0.to_str().unwrap(), &base],
    );
    let in_linked = |args: &[&str]| failure(command(&linked.0).args(args).output().unwrap());
    assert_eq!(in_linked(&["storage", "check"]), without_root);
    for args in [vec!["list"], vec!["init", "t"]] {
        assert!(in_linked(&args).ends_with(&diagnostic), "{args:?}");
    }
    // Without any file under .axon there is no store to report on, only the index.
    fs::remove_dir_all(f.0.join(".axon")).unwrap();
    for root in [None, Some(".")] {
        let mut args = vec!["storage", "check"];
        args.extend(root);
        let error = failure(f.run(&args));
        assert!(error.ends_with(&diagnostic), "{args:?}: {error}");
    }
    assert!(failure(f.run(&["list"])).ends_with(&diagnostic));
    for args in [vec!["list"], vec!["init", "t"]] {
        assert!(in_linked(&args).ends_with(&diagnostic), "{args:?}");
    }
    assert!(!linked.0.join(".axon").exists());
}

#[cfg(unix)]
#[test]
fn unmerged_paths_are_named_from_the_worktree_top_one_line_each() {
    let f = repository();
    // A store below the worktree top, checked as an explicit root.
    let made = Fixture::new();
    made.init();
    let axon = f.0.join("sub/.axon");
    fs::create_dir_all(axon.join("records")).unwrap();
    fs::copy(header(&made.0), axon.join("header.json")).unwrap();
    let forged = "sub/.axon/x\nCorrupt: header.json: missing";
    fs::write(f.0.join(forged), "").unwrap();
    make_index_unmerged(&f.0, &["sub/.axon/header.json", forged]);
    let error = failure(f.run(&["storage", "check", "sub"]));
    // A newline in a path cannot pass for a line of the report.
    assert!(
        error.ends_with(&format!(
            "{UNMERGED} {}; resolve and stage these paths before normal operations\nUnmerged: sub/.axon/header.json\nUnmerged: sub/.axon/x\\nCorrupt: header.json: missing\n",
            fs::canonicalize(&f.0).unwrap().display()
        )),
        "{error}"
    );
    assert!(
        !error.lines().any(|line| line.starts_with("Corrupt")),
        "{error}"
    );
}

#[test]
fn an_unmerged_index_is_reported_before_a_damaged_store_with_a_header() {
    let f = repository();
    f.init();
    make_index_unmerged(&f.0, &[".axon/header.json"]);
    let records = f.0.join(".axon/records");
    fs::remove_dir_all(&records).unwrap();
    fs::write(&records, "").unwrap();
    for args in [vec!["storage", "check"], vec!["list"]] {
        let error = failure(f.run(&args));
        assert!(error.ends_with(&unmerged_header(&f.0)), "{args:?}: {error}");
    }
    // A write lock that cannot be taken is damage as well.
    fs::remove_file(&records).unwrap();
    fs::create_dir(&records).unwrap();
    fs::create_dir(f.0.join(".axon/write.lock")).unwrap();
    let error = failure(f.run(&["capture", "--label", "chore", "--title", "rejected"]));
    assert!(error.ends_with(&unmerged_header(&f.0)), "{error}");
}

#[cfg(unix)]
#[test]
fn unmerged_paths_that_are_not_utf8_stay_apart() {
    let f = repository();
    f.init();
    let blob =
        String::from_utf8(git_output(&f.0, &["hash-object", "-w", ".axon/header.json"]).stdout)
            .unwrap();
    let mut stages = Vec::new();
    for last in [0xfe_u8, 0xff] {
        for stage in 1..=3 {
            stages.extend(format!("100644 {} {stage}\t.axon/a", blob.trim()).as_bytes());
            stages.extend([last, 0]);
        }
    }
    let mut child = isolated_git(&f.0)
        .args(["update-index", "-z", "--index-info"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    child.stdin.take().unwrap().write_all(&stages).unwrap();
    assert!(child.wait().unwrap().success());
    let error = failure(f.run(&["list"]));
    assert_eq!(
        error
            .lines()
            .filter(|line| line.starts_with("Unmerged: "))
            .count(),
        2,
        "{error}"
    );
}

#[test]
fn a_linked_worktree_with_an_unmerged_headerless_store_does_not_fall_through_to_the_main_one() {
    let f = repository();
    let base = conflicting_stores(&f);
    let linked = Fixture::new();
    git(
        &f.0,
        &["worktree", "add", "-q", linked.0.to_str().unwrap(), &base],
    );
    git(&f.0, &["checkout", "-q", "left"]);
    git(&linked.0, &["checkout", "-q", "right"]);
    assert!(!merge_branch(&linked.0, "left").status.success());
    let run = |args: &[&str]| failure(command(&linked.0).args(args).output().unwrap());
    // The linked worktree's `.axon` first keeps its records, then only the residue of an
    // initialization, then nothing at all.
    fs::remove_file(header(&linked.0)).unwrap();
    for records in [true, false, false] {
        if !records {
            let axon = linked.0.join(".axon");
            let records = axon.join("records");
            fs::remove_dir_all(if records.exists() { records } else { axon }).unwrap();
        }
        let without_root = run(&["storage", "check"]);
        // The diagnostic names this worktree, whose index is unmerged, whether or not records
        // remain beside the missing header.
        assert!(
            without_root.ends_with(&unmerged_header(&linked.0)),
            "{records}: {without_root}"
        );
        assert!(
            !without_root.contains("Corrupt"),
            "{records}: {without_root}"
        );
        assert_eq!(run(&["storage", "check", "."]), without_root);
        for args in [
            vec!["list"],
            vec!["capture", "--label", "chore", "--title", "rejected"],
        ] {
            assert!(
                run(&args).ends_with(&unmerged_header(&linked.0)),
                "{args:?}"
            );
        }
    }
    assert!(!f.ok(&["list"]).contains("rejected"));
}

#[test]
fn init_outside_git_creates_the_store_without_git_guidance() {
    let f = Fixture::new();
    let output = f.ok(&["init", "t"]);
    // Outside Git neither operation applies, so nothing but the created path is reported.
    assert_eq!(output, format!("Initialized {}\n", header(&f.0).display()));
    // Outside Git the init lock has no common directory to live in and stays beside the store.
    assert_eq!(
        management_entries(&f.0),
        [
            ".gitattributes",
            ".gitignore",
            "axon-init.lock",
            "header.json",
            "records"
        ]
    );
}

#[test]
fn init_in_a_subdirectory_creates_the_store_at_the_repository_root() {
    let f = repository();
    let nested = f.0.join("deep/deeper");
    fs::create_dir_all(&nested).unwrap();
    let output = success(command(&nested).args(["init", "t"]).output().unwrap());
    assert!(header(&f.0).is_file());
    assert!(!nested.join(".axon").exists());
    assert!(
        output.starts_with(&format!("Initialized {}\n", header(&f.0).display())),
        "{output}"
    );
    // The steps are run where the tracked files belong, not where init was typed.
    assert!(
        output.contains(&format!("Run the\nsteps in\n{}:\n", f.0.display())),
        "{output}"
    );
    let id = created(&success(
        command(&nested)
            .args([
                "capture",
                "--label",
                "chore",
                "--accept",
                "--title",
                "from a subdirectory",
            ])
            .output()
            .unwrap(),
    ));
    assert!(f.ok(&["list"]).contains(&id));
}

#[test]
fn init_reuses_the_residue_of_an_interrupted_initialization_and_refuses_anything_else() {
    // Residue: locks, temporary files, an empty records directory and the same ignore and
    // attributes files.
    let f = repository();
    let directory = f.0.join(".axon");
    fs::create_dir_all(directory.join("records/ab")).unwrap();
    fs::write(directory.join(".gitignore"), "*.lock\r\n*.tmp\r\n").unwrap();
    fs::write(directory.join(".gitattributes"), "* -text\r\n").unwrap();
    fs::write(directory.join("write.lock"), "").unwrap();
    fs::write(directory.join("header.json.tmp"), "partial").unwrap();
    fs::write(directory.join("records/ab/something.tmp"), "partial").unwrap();
    fs::write(directory.join("other.tmp"), "partial").unwrap();
    assert!(failure(f.run(&["list"])).contains("not initialized"));
    f.ok(&["init", "t"]);
    assert!(header(&f.0).is_file());
    assert_eq!(
        fs::read_to_string(directory.join(".gitignore")).unwrap(),
        "*.lock\n*.tmp\n"
    );
    assert_eq!(
        fs::read_to_string(directory.join(".gitattributes")).unwrap(),
        "* -text\n"
    );
    assert!(f.ok(&["list"]).is_empty());
    // An initialization interrupted between the two files leaves the ignore file alone; the
    // retry adds the attributes file and keeps the ignore file.
    let f = repository();
    let directory = f.0.join(".axon");
    fs::create_dir_all(directory.join("records")).unwrap();
    fs::write(directory.join(".gitignore"), "*.lock\n*.tmp\n").unwrap();
    fs::write(directory.join("write.lock"), "").unwrap();
    f.ok(&["init", "t"]);
    assert_eq!(
        management_entries(&f.0),
        [
            ".gitattributes",
            ".gitignore",
            "header.json",
            "records",
            "write.lock"
        ]
    );
    assert_eq!(
        fs::read_to_string(directory.join(".gitattributes")).unwrap(),
        "* -text\n"
    );
    assert_eq!(
        fs::read_to_string(directory.join(".gitignore")).unwrap(),
        "*.lock\n*.tmp\n"
    );
    // Line endings do not make residue foreign: `core.autocrlf=true` turns a tracked ignore
    // file CRLF when no attributes file came with it, and an attributes file may arrive CRLF
    // too. The retry replaces them with init's bytes, so the store is not tracked with CRLF.
    for files in [
        &[(".gitignore", "*.lock\r\n*.tmp\r\n")][..],
        &[
            (".gitignore", "*.lock\r\n*.tmp\r\n"),
            (".gitattributes", "* -text\r\n"),
        ],
    ] {
        let f = repository();
        let directory = f.0.join(".axon");
        fs::create_dir_all(directory.join("records")).unwrap();
        for (name, content) in files {
            fs::write(directory.join(name), content).unwrap();
        }
        assert!(failure(f.run(&["list"])).contains("not initialized"));
        f.ok(&["init", "t"]);
        assert_eq!(
            management_entries(&f.0),
            [".gitattributes", ".gitignore", "header.json", "records"]
        );
        assert_eq!(
            fs::read_to_string(directory.join(".gitignore")).unwrap(),
            "*.lock\n*.tmp\n"
        );
        assert_eq!(
            fs::read_to_string(directory.join(".gitattributes")).unwrap(),
            "* -text\n"
        );
        assert!(f.ok(&["list"]).is_empty());
    }
    // Anything else, valid or not, is refused with its path and left as it is. Only a CRLF
    // pair counts as LF: a lone CR is a different content.
    for (name, content) in [
        (
            "state.jsonl",
            "{\"format\":\"axon-file/v1\",\"prefix\":\"t\"}\n",
        ),
        (".gitignore", "# mine\n*\n"),
        (".gitignore", "# mine\r\n*\r\n"),
        (".gitattributes", "* text=auto\n"),
        (".gitattributes", "* text=auto\r\n"),
        (".gitattributes", "* -text\r"),
        ("records/ab/not-a-record", "x"),
        ("axon.db", "\x00\x01leftover bytes"),
    ] {
        let f = repository();
        let path = f.0.join(".axon").join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
        let error = failure(f.run(&["init", "t"]));
        assert!(error.contains(name), "{name}: {error}");
        assert!(!header(&f.0).exists(), "{name}");
        assert_eq!(fs::read(&path).unwrap(), content.as_bytes(), "{name}");
        let error = failure(f.run(&["list"]));
        assert!(!error.contains("not initialized"), "{name}: {error}");
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(12))]
    #[test]
    fn generated_initialization_residue_is_reused_only_without_foreign_files(
        parts in prop::collection::vec(any::<bool>(), 4..5),
        crlf in any::<bool>(),
        foreign in 0u8..4,
    ) {

        let f = Fixture::new();
        let directory = f.0.join(".axon");
        fs::create_dir_all(directory.join("records/ab")).unwrap();
        fs::write(directory.join("write.lock"), b"").unwrap();
        fs::write(directory.join("header.json.tmp"), b"partial").unwrap();
        if parts[0] {
            let ignore = if crlf { "*.lock\r\n*.tmp\r\n" } else { "*.lock\n*.tmp\n" };
            fs::write(directory.join(".gitignore"), ignore).unwrap();
        }
        if parts[1] {
            let attributes = if crlf { "* -text\r\n" } else { "* -text\n" };
            fs::write(directory.join(".gitattributes"), attributes).unwrap();
        }
        if parts[2] {
            fs::write(directory.join("records/ab/leftover.tmp"), b"partial").unwrap();
        }
        if parts[3] {
            fs::write(directory.join("other.tmp"), b"partial").unwrap();
        }
        let obstruction = match foreign {
            0 => None,
            1 => Some(("records/ab/foreign", b"x".as_slice())),
            2 => Some(("state.jsonl", b"old\n".as_slice())),
            _ => Some((".gitignore", b"# foreign\n".as_slice())),
        };
        if let Some((name, bytes)) = obstruction {
            fs::write(directory.join(name), bytes).unwrap();
        }
        if let Some((name, bytes)) = obstruction {
            let error = failure(f.run(&["init", "t"]));
            prop_assert!(error.contains(name));
            prop_assert_eq!(fs::read(directory.join(name)).unwrap(), bytes);
            prop_assert!(!header(&f.0).exists());
        } else {
            f.ok(&["init", "t"]);
            prop_assert!(header(&f.0).is_file());
            prop_assert_eq!(fs::read(directory.join(".gitignore")).unwrap(), b"*.lock\n*.tmp\n");
            prop_assert_eq!(fs::read(directory.join(".gitattributes")).unwrap(), b"* -text\n");
            prop_assert!(f.ok(&["list"]).is_empty());
        }
    }
}

#[test]
fn storage_check_from_a_linked_worktree_reports_the_main_worktree_headerless_store() {
    let f = repository();
    f.init();
    ignore_store(&f.0, ".axon/");
    f.accepted("in the main worktree");
    fs::remove_file(header(&f.0)).unwrap();
    let linked = add_worktree(&f.0, "linked");
    let error = failure(
        command(&linked.0)
            .args(["storage", "check"])
            .output()
            .unwrap(),
    );
    assert!(error.contains("Corrupt: header.json: missing"), "{error}");
    assert!(
        error.contains(f.0.join(".axon").to_str().unwrap()),
        "{error}"
    );
}

#[cfg(unix)]
#[test]
fn init_outside_git_leaves_nothing_in_a_symlinked_management_directory() {
    let f = Fixture::new();
    let target = Fixture::new();
    std::os::unix::fs::symlink(&target.0, f.0.join(".axon")).unwrap();
    let error = failure(f.run(&["init", "t"]));
    assert!(
        error.contains("management directory is not a regular directory"),
        "{error}"
    );
    // Not even the lock the initialization would take reaches the directory behind the link.
    assert_eq!(fs::read_dir(&target.0).unwrap().count(), 0);
}

#[test]
fn a_repository_configured_bare_has_no_main_worktree_to_share_a_store() {
    let f = repository();
    f.init();
    let id = f.accepted("in the main worktree");
    let before = f.record_files();
    let linked = add_worktree(&f.0, "linked");
    assert!(success(command(&linked.0).args(["list"]).output().unwrap()).contains(&id));
    git(&f.0, &["config", "core.bare", "true"]);
    // The directory beside a bare repository's common directory is not its main worktree.
    let error = failure(command(&linked.0).args(["list"]).output().unwrap());
    assert!(error.contains("not initialized"), "{error}");
    assert_eq!(f.record_files(), before);
}

#[test]
fn a_main_worktree_that_cannot_be_determined_is_an_error_instead_of_an_absent_store() {
    let f = repository();
    f.init();
    f.accepted("in the main worktree");
    let before = f.record_files();
    let linked = add_worktree(&f.0, "linked");
    // Only the main worktree reads this file, so the question about it is the one left unanswered.
    git(&f.0, &["config", "core.repositoryformatversion", "1"]);
    git(&f.0, &["config", "extensions.worktreeConfig", "true"]);
    fs::write(f.0.join(".git/config.worktree"), "[core\n").unwrap();
    let error = failure(command(&linked.0).args(["list"]).output().unwrap());
    assert!(
        error.contains(&format!(
            "cannot determine the main worktree from {}",
            f.0.join(".git").display()
        )),
        "{error}"
    );
    assert!(!error.contains("not initialized"), "{error}");
    assert_eq!(f.record_files(), before);
}

#[test]
fn a_linked_worktree_without_a_store_uses_the_main_worktrees_store() {
    let f = repository();
    f.init();
    ignore_store(&f.0, ".axon/");
    let linked = add_worktree(&f.0, "linked");
    let from_linked = created(&success(
        command(&linked.0)
            .args([
                "capture",
                "--label",
                "chore",
                "--accept",
                "--title",
                "from linked",
            ])
            .output()
            .unwrap(),
    ));
    assert!(f.ok(&["list"]).contains(&from_linked));
    let from_main = f.accepted("from main");
    assert!(
        success(command(&linked.0).args(["list"]).output().unwrap()).contains(&from_main),
        "the linked worktree reads the main worktree's store"
    );
    success(
        command(&linked.0)
            .args(["note", "add", &from_main, "-m", "from the linked worktree"])
            .output()
            .unwrap(),
    );
    assert!(
        f.ok(&["note", "list", &from_main])
            .contains("from the linked worktree")
    );
    assert!(!linked.0.join(".axon").exists());
}

#[test]
fn concurrent_start_across_worktrees_has_one_winner() {
    let f = repository();
    f.init();
    ignore_store(&f.0, ".axon/");
    let id = f.accepted("contended");
    let linked = add_worktree(&f.0, "linked");
    let attempts: Vec<_> = (0..8)
        .map(|n| {
            let worktree = if n % 2 == 0 { &f.0 } else { &linked.0 };
            command(worktree)
                .args(["start", &id])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    let winners = attempts
        .into_iter()
        .map(|child| child.wait_with_output().unwrap())
        .filter(|out| out.status.success())
        .count();
    assert_eq!(winners, 1);
    assert!(!linked.0.join(".axon").exists());
}

#[test]
fn init_in_a_linked_worktree_is_refused_beside_the_main_worktrees_store() {
    let f = repository();
    f.init();
    let before = fs::read(header(&f.0)).unwrap();
    let linked = add_worktree(&f.0, "linked");
    let error = failure(command(&linked.0).args(["init", "demo"]).output().unwrap());
    assert!(error.contains("already holds a store"), "{error}");
    assert!(
        error.contains(&f.0.join(".axon").display().to_string()),
        "{error}"
    );
    assert!(!linked.0.join(".axon").exists());
    assert_eq!(fs::read(header(&f.0)).unwrap(), before);
}

#[test]
fn init_in_a_linked_worktree_is_allowed_while_the_main_worktree_has_none() {
    let f = repository();
    let linked = add_worktree(&f.0, "linked");
    let output = success(command(&linked.0).args(["init", "demo"]).output().unwrap());
    assert!(
        output
            .contains("This store belongs to this linked worktree; other worktrees do not see it."),
        "{output}"
    );
    assert!(header(&linked.0).is_file());
    assert!(!f.0.join(".axon").exists());
    let id = created(&success(
        command(&linked.0)
            .args([
                "capture",
                "--label",
                "chore",
                "--accept",
                "--title",
                "local only",
            ])
            .output()
            .unwrap(),
    ));
    assert!(success(command(&linked.0).args(["list"]).output().unwrap()).contains(&id));
    assert!(failure(f.run(&["list"])).contains("not initialized"));
    // `.git` is a file here, so the displayed way to find the exclude file has to resolve it.
    assert!(
        output.contains("git rev-parse --git-path info/exclude"),
        "{output}"
    );
    ignore_store(&linked.0, &ignore_line(&output));
    assert!(
        git_output(&linked.0, &["status", "--porcelain"])
            .stdout
            .is_empty()
    );
}

#[test]
fn a_worktree_of_a_bare_repository_does_not_reach_the_directory_beside_it() {
    for name in ["repo.git", ".git"] {
        let f = Fixture::new();
        let source = repository();
        let holder = f.0.join("holder");
        fs::create_dir(&holder).unwrap();
        success(command(&holder).args(["init", "holder"]).output().unwrap());
        let visible = created(&success(
            command(&holder)
                .args([
                    "capture",
                    "--label",
                    "chore",
                    "--accept",
                    "--title",
                    "beside the bare repository",
                ])
                .output()
                .unwrap(),
        ));
        assert!(success(command(&holder).args(["list"]).output().unwrap()).contains(&visible));
        let bare = holder.join(name);
        git(
            &f.0,
            &[
                "clone",
                "--bare",
                "-q",
                source.0.to_str().unwrap(),
                bare.to_str().unwrap(),
            ],
        );
        let worktree = f.0.join("worktree");
        git(
            &bare,
            &[
                "worktree",
                "add",
                "-q",
                "--detach",
                worktree.to_str().unwrap(),
                "HEAD",
            ],
        );
        let error = failure(command(&worktree).args(["list"]).output().unwrap());
        assert!(error.contains("not initialized"), "{name}: {error}");
        fs::remove_dir_all(&worktree).unwrap();
    }
}

#[test]
fn a_submodule_uses_neither_the_superprojects_store_nor_its_modules_directory() {
    let f = Fixture::new();
    let child = repository();
    let parent = f.0.join("super");
    fs::create_dir(&parent).unwrap();
    git(&parent, &["init", "-q"]);
    git_commit(&parent, &["-q", "--allow-empty", "-m", "base"]);
    success(command(&parent).args(["init", "sup"]).output().unwrap());
    let visible = created(&success(
        command(&parent)
            .args([
                "capture",
                "--label",
                "chore",
                "--accept",
                "--title",
                "superproject only",
            ])
            .output()
            .unwrap(),
    ));
    git(
        &parent,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            child.0.to_str().unwrap(),
            "sub",
        ],
    );
    // A store left in the common directory's parent is not a fallback either.
    let modules = parent.join(".git/modules/.axon");
    fs::create_dir_all(&modules).unwrap();
    fs::copy(header(&parent), modules.join("header.json")).unwrap();
    let error = failure(
        command(&parent.join("sub"))
            .args(["list"])
            .output()
            .unwrap(),
    );
    assert!(error.contains("not initialized"), "{error}");
    assert!(success(command(&parent).args(["list"]).output().unwrap()).contains(&visible));
}

#[test]
fn discovery_settles_on_a_header_and_never_falls_back_from_one() {
    for kind in [
        "empty",
        "lock",
        "residue",
        "crlf-residue",
        "earlier-format",
        "corrupt-header",
        "own-store",
    ] {
        let f = repository();
        f.init();
        ignore_store(&f.0, ".axon/");
        let id = f.accepted("in the main worktree");
        let before = f.record_files();
        let linked = add_worktree(&f.0, "linked");
        fs::create_dir(linked.0.join(".axon")).unwrap();
        match kind {
            "lock" => fs::write(linked.0.join(".axon/write.lock"), "").unwrap(),
            "residue" => {
                fs::create_dir(linked.0.join(".axon/records")).unwrap();
                fs::write(linked.0.join(".axon/.gitignore"), "*.lock\n*.tmp\n").unwrap();
                fs::write(linked.0.join(".axon/.gitattributes"), "* -text\n").unwrap();
                fs::write(linked.0.join(".axon/header.json.tmp"), "partial").unwrap();
            }
            // Both files after Git line-ending conversion.
            "crlf-residue" => {
                fs::create_dir(linked.0.join(".axon/records")).unwrap();
                fs::write(linked.0.join(".axon/.gitignore"), "*.lock\r\n*.tmp\r\n").unwrap();
                fs::write(linked.0.join(".axon/.gitattributes"), "* -text\r\n").unwrap();
            }
            "earlier-format" => fs::write(linked.0.join(".axon/state.jsonl"), b"old\n").unwrap(),
            "corrupt-header" => fs::write(header(&linked.0), b"not a header\n").unwrap(),
            // A store of its own, as a checkout of a commit that tracks one leaves it.
            "own-store" => {
                fs::copy(header(&f.0), header(&linked.0)).unwrap();
            }
            _ => {}
        }
        let read = command(&linked.0).args(["list"]).output().unwrap();
        match kind {
            // A store that cannot be used, or a directory that is not a store, stops the
            // operation instead of reaching the main worktree's store.
            "earlier-format" => {
                let error = failure(read);
                assert!(error.contains("not a store"), "{kind}: {error}");
            }
            "corrupt-header" => {
                let error = failure(read);
                assert!(error.contains("header"), "{kind}: {error}");
            }
            // The current worktree's own store is read before the main worktree's.
            "own-store" => assert!(!success(read).contains(&id), "{kind}"),
            // Neither an empty directory, a lock nor the residue of an initialization
            // settles discovery.
            _ => assert!(success(read).contains(&id), "{kind}"),
        }
        assert_eq!(f.record_files(), before, "{kind}");
    }
}

#[cfg(unix)]
#[test]
fn a_management_directory_that_is_a_symlink_is_rejected_from_a_linked_worktree() {
    let f = repository();
    let target = Fixture::new();
    target.init();
    target.accepted("kept");
    std::os::unix::fs::symlink(target.0.join(".axon"), f.0.join(".axon")).unwrap();
    let linked = add_worktree(&f.0, "linked");
    let before = target.record_files();
    for args in [
        vec!["list"],
        vec![
            "capture", "--label", "chore", "--accept", "--title", "rejected",
        ],
    ] {
        let error = failure(command(&linked.0).args(args).output().unwrap());
        assert!(
            error.contains("management directory is not a regular directory"),
            "{error}"
        );
    }
    assert_eq!(target.record_files(), before);
}

#[test]
fn a_branch_without_the_tracked_store_writes_the_main_worktrees_files() {
    let f = repository();
    git(&f.0, &["branch", "without-store"]);
    f.init();
    let id = f.accepted("job");
    track_store(&f.0);
    git_commit(&f.0, &["-qm", "track the store"]);
    let linked = Fixture::new();
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
    assert!(!linked.0.join(".axon").exists());
    success(command(&linked.0).args(["start", &id]).output().unwrap());
    assert!(f.ok(&["show", &id]).contains("InProgress"));
    let status = String::from_utf8(git_output(&f.0, &["status", "--porcelain"]).stdout).unwrap();
    assert!(
        status
            .lines()
            .any(|line| line == "?? .axon/records/" || line.starts_with("?? .axon/records/")),
        "the accepted side effect shows as a new record file in the main worktree: {status}"
    );
    // The index check follows the store, not the worktree the command ran in, and the
    // diagnostic names the main worktree and each of its unmerged paths once.
    make_index_unmerged(&f.0, &[".axon/header.json", ".axon/.gitignore"]);
    let diagnostic = format!(
        "{UNMERGED} {}; resolve and stage these paths before normal operations\nUnmerged: .axon/.gitignore\nUnmerged: .axon/header.json\n",
        fs::canonicalize(&f.0).unwrap().display()
    );
    for args in [vec!["list"], vec!["note", "add", &id, "-m", "rejected"]] {
        let error = failure(command(&linked.0).args(args).output().unwrap());
        assert!(error.ends_with(&diagnostic), "{error}");
    }
}

#[test]
fn conditions_run_in_the_current_worktree_while_the_store_is_shared() {
    let f = repository();
    f.init();
    ignore_store(&f.0, ".axon/");
    let task = f.accepted("task");
    let proposal = created(&f.ok(&["capture", "--label", "chore", "--title", "proposal"]));
    for id in [&task, &proposal] {
        f.ok(&[
            "condition",
            "set",
            id,
            "--command",
            "test \"$AXON_TEST_CONDITION\" = inherited && test -f marker",
        ]);
    }
    let linked = add_worktree(&f.0, "linked");
    let candidates = |fixture: &Fixture, args: &[&str]| {
        success(
            command(&fixture.0)
                .env("AXON_TEST_CONDITION", "inherited")
                .args(args)
                .output()
                .unwrap(),
        )
    };
    assert!(candidates(&f, &["tasks"]).is_empty());
    assert!(candidates(&linked, &["tasks"]).is_empty());
    fs::write(linked.0.join("marker"), "").unwrap();
    fs::create_dir(linked.0.join("subdir")).unwrap();
    let traced = command(&linked.0.join("subdir"))
        .env("AXON_TEST_CONDITION", "inherited")
        .args(["tasks", "--trace-conditions"])
        .output()
        .unwrap();
    assert!(traced.status.success());
    assert!(String::from_utf8(traced.stdout).unwrap().contains(&task));
    assert!(
        String::from_utf8(traced.stderr)
            .unwrap()
            .contains(&format!("cwd: {}", linked.0.display()))
    );
    assert!(candidates(&linked, &["proposals"]).contains(&proposal));
    assert!(candidates(&f, &["tasks"]).is_empty());
    assert!(candidates(&f, &["proposals"]).is_empty());
    fs::write(f.0.join("marker"), "").unwrap();
    assert!(candidates(&f, &["tasks"]).contains(&task));
}
