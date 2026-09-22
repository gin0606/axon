//! Where `axon init` puts a store, and which store an operation reaches inside Git.
use super::*;

fn state(root: &Path) -> PathBuf {
    root.join(".axon/state.jsonl")
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
/// The names inside the management directory, sorted.
fn management_entries(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(root.join(".axon"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}
/// The literal lines shown under a numbered step of the init output.
fn steps(output: &str, number: u32) -> Vec<String> {
    output
        .lines()
        .skip_while(|line| !line.starts_with(&format!("  {number}. ")))
        .skip(1)
        .take_while(|line| line.starts_with("       "))
        .map(|line| line.trim_start().to_owned())
        .collect()
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
fn add_worktree(main: &Path, name: &str) -> Fixture {
    let linked = Fixture::new();
    git(
        main,
        &["worktree", "add", "-qb", name, linked.0.to_str().unwrap()],
    );
    linked
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
/// The Entity IDs named by the snapshot rows of one kind, in file order.
fn row_entities(path: &Path, kind: &str, field: &str) -> Vec<String> {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter_map(|line| {
            let row: serde_json::Value = serde_json::from_str(line).ok()?;
            (row["type"] == kind).then(|| row["value"][field].as_str().unwrap().to_owned())
        })
        .collect()
}
fn last_line(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .last()
        .unwrap()
        .to_owned()
}
/// The tracked operation with the merge driver registration, step 3, left out.
fn track_without_driver(root: &Path) {
    fs::write(
        root.join(".axon/.gitignore"),
        "*\n!.gitignore\n!state.jsonl\n",
    )
    .unwrap();
    fs::write(
        root.join(".gitattributes"),
        "/.axon/state.jsonl merge=axon\n",
    )
    .unwrap();
    git(
        root,
        &[
            "add",
            ".axon/state.jsonl",
            ".axon/.gitignore",
            ".gitattributes",
        ],
    );
    git_commit(root, &["-qm", "track the store"]);
    assert!(
        git_output(root, &["config", "--get", "merge.axon.driver"])
            .stdout
            .is_empty()
    );
}
/// A merge that may leave conflicts, with a fixed identity and no hooks.
fn merge_branch(root: &Path, branch: &str) -> Output {
    git_output(
        root,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "core.hooksPath=/dev/null",
            "merge",
            "--no-edit",
            branch,
        ],
    )
}
/// The reported failure without the command name it is reported under.
fn detail(error: &str) -> &str {
    error.rsplit_once(": ").unwrap().1
}
/// Replace the index entry for the snapshot with unresolved stages.
fn make_index_unmerged(root: &Path) {
    let blob =
        String::from_utf8(git_output(root, &["hash-object", "-w", ".axon/state.jsonl"]).stdout)
            .unwrap();
    let blob = blob.trim();
    let stages = format!(
        "100644 {blob} 1\t.axon/state.jsonl\n100644 {blob} 2\t.axon/state.jsonl\n100644 {blob} 3\t.axon/state.jsonl\n"
    );
    git(
        root,
        &["update-index", "--force-remove", ".axon/state.jsonl"],
    );
    let mut child = isolated_git(root)
        .args(["update-index", "--index-info"])
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
fn init_creates_the_snapshot_only_and_leaves_it_untracked() {
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
        output.starts_with(&format!("Initialized {}\n", state(&f.0).display())),
        "{output}"
    );
    // The snapshot is the whole of what init leaves behind: no ignore file of its own, and no
    // marker, lock or temporary file from the initialization.
    assert_eq!(management_entries(&f.0), ["state.jsonl"]);
    assert!(state(&f.0).is_file());
    // Both operations are explained, and neither is applied.
    assert!(
        output.contains(&format!(
            "Unless an ignore rule of yours already covers it, Git sees .axon as untracked\nand git add -A would commit the store. Check which applies with:\n       git check-ignore -v .axon/state.jsonl\nChoose one way to use the store; do not mix the two in one repository. Run the\nsteps in\n{}:",
            f.0.display()
        )),
        "{output}"
    );
    assert_eq!(ignore_line(&output), ".axon/");
    assert!(
        output.contains(
            "To track the store in Git and merge it between branches instead, first remove\nany ignore rule outside .axon that covers the directory, then:"
        ),
        "{output}"
    );
    for number in 1..=4 {
        assert!(!steps(&output, number).is_empty(), "{output}");
    }
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
    // The displayed steps are the only way the merge driver gets registered.
    assert!(
        git_output(&f.0, &["config", "--get-regexp", "^merge\\.axon\\."])
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
        git_output(&f.0, &["check-ignore", "-q", ".axon/state.jsonl"])
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
    let tracked = fs::read(state(&f.0)).unwrap();
    git(&f.0, &["checkout", "-q", &base]);
    assert!(!state(&f.0).exists());
    f.init();
    f.accepted("saved in the store this branch never committed");
    let untracked = fs::read(state(&f.0)).unwrap();
    assert_ne!(untracked, tracked);

    // Untracked, so Git stops instead of losing records it never saved.
    let refused = git_output(&f.0, &["checkout", "tracked"]);
    assert!(!refused.status.success());
    let reason = String::from_utf8_lossy(&refused.stderr).into_owned();
    assert!(reason.contains(".axon/state.jsonl"), "{reason}");
    assert_eq!(fs::read(state(&f.0)).unwrap(), untracked);
    assert_eq!(head_branch(&f.0), base);

    // Ignored, so the same checkout succeeds and the store is the other branch's.
    ignore_store(&f.0, ".axon/");
    git(&f.0, &["checkout", "-q", "tracked"]);
    let after = fs::read(state(&f.0)).unwrap();
    assert_ne!(after, untracked);
    assert_eq!(after, tracked);
}

#[test]
fn the_displayed_steps_track_the_store_and_let_the_driver_merge_branches() {
    let f = repository();
    let output = f.ok(&["init", "t"]);
    for heading in [
        "  1. Create .axon/.gitignore with these lines:",
        "  2. Add this line to .gitattributes in the repository root:",
        "  3. Register the merge driver, using the absolute path of the axon binary:",
        "  4. Stage and commit the files:",
    ] {
        assert!(output.contains(heading), "{output}");
    }
    let rules = steps(&output, 1);
    assert_eq!(rules, ["*", "!.gitignore", "!state.jsonl"]);
    assert!(!f.0.join(".axon/.gitignore").exists(), "step 1 creates it");
    fs::write(
        f.0.join(".axon/.gitignore"),
        format!("{}\n", rules.join("\n")),
    )
    .unwrap();
    let attributes = steps(&output, 2);
    assert_eq!(attributes, ["/.axon/state.jsonl merge=axon"]);
    fs::write(f.0.join(".gitattributes"), format!("{}\n", attributes[0])).unwrap();
    for line in [steps(&output, 3), steps(&output, 4)].concat() {
        // Only the placeholder path is the reader's to fill in.
        run_step(
            &f.0,
            &line.replace("/absolute/path/to/axon", env!("CARGO_BIN_EXE_axon")),
        );
    }
    // The commit is part of step 4, so the steps alone leave the tree clean and the files saved.
    let status = String::from_utf8(git_output(&f.0, &["status", "--porcelain"]).stdout).unwrap();
    assert!(status.is_empty(), "{status}");
    let tracked =
        String::from_utf8(git_output(&f.0, &["ls-tree", "-r", "--name-only", "HEAD"]).stdout)
            .unwrap();
    for path in [".axon/state.jsonl", ".axon/.gitignore", ".gitattributes"] {
        assert!(tracked.lines().any(|line| line == path), "{tracked}");
    }
    let attribute = String::from_utf8(
        git_output(&f.0, &["check-attr", "merge", "--", ".axon/state.jsonl"]).stdout,
    )
    .unwrap();
    assert!(attribute.contains("merge: axon"), "{attribute}");

    let mine = f.accepted("mine");
    let theirs = f.accepted("theirs");
    git_commit(&f.0, &["-qam", "two entities"]);
    let base = head_branch(&f.0);
    git(&f.0, &["checkout", "-qb", "left"]);
    f.ok(&["start", &mine]);
    git_commit(&f.0, &["-qam", "start mine"]);
    git(&f.0, &["checkout", "-q", &base]);
    git(&f.0, &["checkout", "-qb", "right"]);
    f.ok(&["start", &theirs]);
    git_commit(&f.0, &["-qam", "start theirs"]);
    success(merge_branch(&f.0, "left"));
    assert!(git_output(&f.0, &["ls-files", "-u"]).stdout.is_empty());
    f.ok(&["storage", "check", state(&f.0).to_str().unwrap()]);
    for id in [&mine, &theirs] {
        assert!(f.ok(&["show", id]).contains("InProgress"), "{id}");
    }
}

#[test]
fn a_missing_merge_driver_still_lets_git_combine_changes_to_distant_entities() {
    let f = repository();
    f.init();
    // Enough Entities that the two branches change lines far apart in the file.
    let ids: Vec<String> = (0..42)
        .map(|n| f.accepted(&format!("entity {n}")))
        .collect();
    track_without_driver(&f.0);
    let entities = row_entities(&state(&f.0), "Entity", "id");
    let records = row_entities(&state(&f.0), "State", "entity");
    let saved_last = last_line(&state(&f.0));
    let base = head_branch(&f.0);
    // Entity rows come first, sorted by ID; State records follow, in causal order with record
    // ID tie-breaks. A start appends one State record, and for the Entity of the file's last
    // record that position is forced: nothing saved can follow it.
    let late = records.last().unwrap().clone();
    git(&f.0, &["checkout", "-qb", "left"]);
    f.ok(&["start", &late]);
    assert_ne!(last_line(&state(&f.0)), saved_last);
    git_commit(&f.0, &["-qam", "start one entity"]);
    git(&f.0, &["checkout", "-q", &base]);
    git(&f.0, &["checkout", "-qb", "right"]);
    // The other branch needs its own line: a record ID that sorts after every saved one lands
    // in the position the first branch already took, so such an attempt is undone and retried
    // on an Entity whose row is not a neighbour of the first branch's either.
    let late_row = entities.iter().position(|id| *id == late).unwrap();
    let mut candidates: Vec<(usize, &String)> = entities
        .iter()
        .enumerate()
        .filter(|(row, _)| row.abs_diff(late_row) > 1)
        .collect();
    candidates.sort_by_key(|(row, _)| std::cmp::Reverse(row.abs_diff(late_row)));
    let mut chosen = None;
    for (_, candidate) in candidates {
        f.ok(&["start", candidate]);
        if last_line(&state(&f.0)) == saved_last {
            chosen = Some(candidate.clone());
            break;
        }
        git(&f.0, &["reset", "--hard"]);
    }
    let early =
        chosen.expect("an Entity whose new record is not appended after the last saved one");
    git_commit(&f.0, &["-qam", "start another entity"]);
    success(merge_branch(&f.0, "left"));
    assert!(git_output(&f.0, &["ls-files", "-u"]).stdout.is_empty());
    f.ok(&["storage", "check", state(&f.0).to_str().unwrap()]);
    let list = f.ok(&["list"]);
    for id in &ids {
        assert!(list.contains(id), "{id}");
    }
    for id in [&late, &early] {
        assert!(f.ok(&["show", id]).contains("InProgress"), "{id}");
    }
}

#[test]
fn a_missing_merge_driver_leaves_one_entity_changed_on_both_sides_unreadable() {
    let f = repository();
    f.init();
    let contested = f.accepted("changed on both sides");
    f.accepted("untouched");
    track_without_driver(&f.0);
    let base = head_branch(&f.0);
    git(&f.0, &["checkout", "-qb", "left"]);
    f.ok(&["start", &contested]);
    git_commit(&f.0, &["-qam", "start it"]);
    git(&f.0, &["checkout", "-q", &base]);
    git(&f.0, &["checkout", "-qb", "right"]);
    f.ok(&["write", &contested, "--title", "renamed instead"]);
    git_commit(&f.0, &["-qam", "rename it"]);
    assert!(!merge_branch(&f.0, "left").status.success());
    assert!(fs::read_to_string(state(&f.0)).unwrap().contains("<<<<<<<"));
    assert!(!git_output(&f.0, &["ls-files", "-u"]).stdout.is_empty());
    let checked = failure(f.run(&["storage", "check", state(&f.0).to_str().unwrap()]));
    // The unresolved index stops a normal read before the file is even parsed.
    let unmerged = failure(f.run(&["list"]));
    assert!(unmerged.contains("unmerged"), "{unmerged}");
    git(&f.0, &["add", ".axon/state.jsonl"]);
    // Staging the markers as they are answers the index, not the snapshot.
    let staged = failure(f.run(&["list"]));
    assert!(!staged.contains("unmerged"), "{staged}");
    assert_eq!(detail(&staged), detail(&checked));
}

#[test]
fn init_outside_git_creates_the_store_without_git_guidance() {
    let f = Fixture::new();
    let output = f.ok(&["init", "t"]);
    // Outside Git neither operation applies, so nothing but the created path is reported.
    assert_eq!(output, format!("Initialized {}\n", state(&f.0).display()));
    // Outside Git the init lock has no common directory to live in and stays beside the store.
    assert_eq!(management_entries(&f.0), ["axon-init.lock", "state.jsonl"]);
}

#[test]
fn init_in_a_subdirectory_creates_the_store_at_the_repository_root() {
    let f = repository();
    let nested = f.0.join("deep/deeper");
    fs::create_dir_all(&nested).unwrap();
    let output = success(command(&nested).args(["init", "t"]).output().unwrap());
    assert!(state(&f.0).is_file());
    assert!(!nested.join(".axon").exists());
    assert!(
        output.starts_with(&format!("Initialized {}\n", state(&f.0).display())),
        "{output}"
    );
    // The steps are run where the tracked files belong, not where init was typed.
    assert!(
        output.contains(&format!("Run the\nsteps in\n{}:\n", f.0.display())),
        "{output}"
    );
    let id = created(&success(
        command(&nested)
            .args(["capture", "--accept", "--title", "from a subdirectory"])
            .output()
            .unwrap(),
    ));
    assert!(f.ok(&["list"]).contains(&id));
}

#[cfg(unix)]
#[test]
fn an_existing_ignore_file_beside_the_store_is_left_untouched() {
    // Init takes no part in how Git treats the store, so whatever is at that path is not its
    // concern, whether or not it is a file at all.
    for kind in ["file", "directory", "symlink"] {
        let f = repository();
        let ignore = f.0.join(".axon/.gitignore");
        fs::create_dir(f.0.join(".axon")).unwrap();
        match kind {
            "file" => fs::write(&ignore, "# mine\n*\n!state.jsonl\n").unwrap(),
            "directory" => fs::create_dir(&ignore).unwrap(),
            _ => {
                fs::write(f.0.join("rules"), "*\n").unwrap();
                std::os::unix::fs::symlink(f.0.join("rules"), &ignore).unwrap();
            }
        }
        f.ok(&["init", "t"]);
        assert!(state(&f.0).is_file(), "{kind}");
        assert!(!f.0.join(".axon/init.pending").exists(), "{kind}");
        let found = fs::symlink_metadata(&ignore).unwrap().file_type();
        match kind {
            "file" => {
                assert!(found.is_file(), "{kind}");
                assert_eq!(fs::read(&ignore).unwrap(), b"# mine\n*\n!state.jsonl\n");
            }
            "directory" => {
                assert!(found.is_dir(), "{kind}");
                assert_eq!(fs::read_dir(&ignore).unwrap().count(), 0);
            }
            _ => {
                assert!(found.is_symlink(), "{kind}");
                assert_eq!(fs::read_link(&ignore).unwrap(), f.0.join("rules"));
                assert_eq!(fs::read(f.0.join("rules")).unwrap(), b"*\n");
            }
        }
        assert!(f.ok(&["list"]).is_empty(), "{kind}");
    }
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
    let before = fs::read(state(&f.0)).unwrap();
    let linked = add_worktree(&f.0, "linked");
    assert!(success(command(&linked.0).args(["list"]).output().unwrap()).contains(&id));
    git(&f.0, &["config", "core.bare", "true"]);
    // The directory beside a bare repository's common directory is not its main worktree.
    let error = failure(command(&linked.0).args(["list"]).output().unwrap());
    assert!(error.contains("not initialized"), "{error}");
    assert_eq!(fs::read(state(&f.0)).unwrap(), before);
}

#[test]
fn a_main_worktree_that_cannot_be_determined_is_an_error_instead_of_an_absent_store() {
    let f = repository();
    f.init();
    f.accepted("in the main worktree");
    let before = fs::read(state(&f.0)).unwrap();
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
    assert_eq!(fs::read(state(&f.0)).unwrap(), before);
}

#[test]
fn a_linked_worktree_without_a_store_uses_the_main_worktrees_store() {
    let f = repository();
    f.init();
    ignore_store(&f.0, ".axon/");
    let linked = add_worktree(&f.0, "linked");
    let from_linked = created(&success(
        command(&linked.0)
            .args(["capture", "--accept", "--title", "from linked"])
            .output()
            .unwrap(),
    ));
    assert!(f.ok(&["list"]).contains(&from_linked));
    let from_main = f.accepted("from main");
    assert!(
        success(command(&linked.0).args(["list"]).output().unwrap()).contains(&from_main),
        "the linked worktree reads the main worktree's store"
    );
    assert!(!linked.0.join(".axon").exists());
}

#[test]
fn concurrent_start_across_worktrees_has_one_winner_and_one_lock() {
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
    assert!(f.0.join(".axon/state.lock").is_file());
    assert!(!linked.0.join(".axon").exists());
}

#[test]
fn init_in_a_linked_worktree_is_refused_beside_the_main_worktrees_store() {
    for marker in ["state.jsonl", "init.pending"] {
        let f = repository();
        if marker == "state.jsonl" {
            f.init();
        } else {
            fs::create_dir(f.0.join(".axon")).unwrap();
            fs::write(f.0.join(".axon/init.pending"), "interrupted").unwrap();
        }
        let existing = f.0.join(".axon").join(marker);
        let before = fs::read(&existing).unwrap();
        let linked = add_worktree(&f.0, "linked");
        let error = failure(command(&linked.0).args(["init", "demo"]).output().unwrap());
        assert!(error.contains("already holds a store"), "{marker}: {error}");
        assert!(
            error.contains(&f.0.join(".axon").display().to_string()),
            "{marker}: {error}"
        );
        assert!(!linked.0.join(".axon").exists(), "{marker}");
        assert_eq!(fs::read(&existing).unwrap(), before, "{marker}");
    }
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
    assert!(state(&linked.0).is_file());
    assert!(!f.0.join(".axon").exists());
    let id = created(&success(
        command(&linked.0)
            .args(["capture", "--accept", "--title", "local only"])
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
            .args(["capture", "--accept", "--title", "superproject only"])
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
    fs::copy(state(&parent), modules.join("state.jsonl")).unwrap();
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
fn discovery_settles_on_a_snapshot_or_a_marker_and_never_falls_back_from_one() {
    for kind in ["init.pending", "empty", "lock", "invalid"] {
        let f = repository();
        f.init();
        ignore_store(&f.0, ".axon/");
        let id = f.accepted("in the main worktree");
        let before = fs::read(state(&f.0)).unwrap();
        let linked = add_worktree(&f.0, "linked");
        fs::create_dir(linked.0.join(".axon")).unwrap();
        match kind {
            "init.pending" => {
                fs::write(linked.0.join(".axon/init.pending"), "interrupted").unwrap()
            }
            "lock" => fs::write(linked.0.join(".axon/state.lock"), "").unwrap(),
            "invalid" => fs::write(state(&linked.0), b"not a snapshot\n").unwrap(),
            _ => {}
        }
        let read = command(&linked.0).args(["list"]).output().unwrap();
        match kind {
            // A settled store that cannot be used stops the operation.
            "init.pending" => {
                assert!(
                    failure(read).contains("incomplete initialization"),
                    "{kind}"
                );
            }
            "invalid" => {
                let error = failure(read);
                assert!(error.contains("invalid file header"), "{kind}: {error}");
            }
            // Neither an empty directory nor a lock alone settles discovery.
            _ => assert!(success(read).contains(&id), "{kind}"),
        }
        assert_eq!(fs::read(state(&f.0)).unwrap(), before, "{kind}");
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
    let before = fs::read(state(&target.0)).unwrap();
    for args in [
        vec!["list"],
        vec!["capture", "--accept", "--title", "rejected"],
    ] {
        let error = failure(command(&linked.0).args(args).output().unwrap());
        assert!(
            error.contains("management directory is not a regular directory"),
            "{error}"
        );
    }
    assert_eq!(fs::read(state(&target.0)).unwrap(), before);
}

#[test]
fn a_branch_without_the_tracked_store_writes_the_main_worktrees_file() {
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
            .any(|line| line == " M .axon/state.jsonl" || line == "M  .axon/state.jsonl"),
        "the accepted side effect shows as a change in the main worktree: {status}"
    );
    // The index check follows the store, not the worktree the command ran in.
    make_index_unmerged(&f.0);
    for args in [vec!["list"], vec!["note", "add", &id, "-m", "rejected"]] {
        let error = failure(command(&linked.0).args(args).output().unwrap());
        assert!(error.contains("unmerged"), "{error}");
    }
}

#[test]
fn a_leftover_axon_db_file_is_neither_a_store_nor_an_obstacle() {
    let f = Fixture::new();
    let leftover = f.0.join(".axon/axon.db");
    let bytes = b"\x00\x01leftover bytes".to_vec();
    fs::create_dir(f.0.join(".axon")).unwrap();
    fs::write(&leftover, &bytes).unwrap();
    assert!(failure(f.run(&["list"])).contains("not initialized"));
    f.ok(&["init", "t"]);
    assert!(state(&f.0).is_file());
    assert_eq!(fs::read(&leftover).unwrap(), bytes);
    assert!(f.ok(&["list"]).is_empty());
}

#[test]
fn conditions_run_in_the_current_worktree_while_the_store_is_shared() {
    let f = repository();
    f.init();
    ignore_store(&f.0, ".axon/");
    let task = f.accepted("task");
    let proposal = created(&f.ok(&["capture", "--title", "proposal"]));
    for id in [&task, &proposal] {
        f.ok(&["condition", "set", id, "--command", "test -f marker"]);
    }
    let linked = add_worktree(&f.0, "linked");
    assert!(f.ok(&["tasks"]).is_empty());
    assert!(success(command(&linked.0).args(["tasks"]).output().unwrap()).is_empty());
    fs::write(linked.0.join("marker"), "").unwrap();
    assert!(
        success(command(&linked.0).args(["tasks"]).output().unwrap()).contains(&task),
        "the condition sees the worktree the command ran in"
    );
    assert!(success(command(&linked.0).args(["proposals"]).output().unwrap()).contains(&proposal));
    assert!(f.ok(&["tasks"]).is_empty());
    assert!(f.ok(&["proposals"]).is_empty());
    fs::write(f.0.join("marker"), "").unwrap();
    assert!(f.ok(&["tasks"]).contains(&task));
}
