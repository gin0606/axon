use axon::{
    file,
    lifecycle::{
        Context, EntityId, Kind, Lifecycle, Recorder,
        record::{self, Current, Entry, Store},
    },
    location::Location,
};
use chrono::Utc;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::{Arc, Barrier},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "axon-lifecycle-test-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }
    fn command(&self) -> Command {
        command(&self.0)
    }
    fn run(&self, args: &[&str]) -> Output {
        self.command().args(args).output().unwrap()
    }
    fn ok(&self, args: &[&str]) -> String {
        success(self.run(args))
    }
    fn accepted(&self, title: &str) -> String {
        self.ok(&["capture", "--accept", "--title", title])
            .split_whitespace()
            .next()
            .unwrap()
            .into()
    }
    fn init(&self) {
        self.ok(&["init", "t"]);
    }
    fn header(&self) -> PathBuf {
        self.0.join(".axon/header.json")
    }
    fn records_dir(&self) -> PathBuf {
        self.0.join(".axon/records")
    }
    fn store(&self) -> file::Store {
        Location::discover(&self.0, false).unwrap().open().unwrap()
    }
    /// The record set as the CLI reads it.
    fn records(&self) -> Store {
        self.store().read().unwrap().1
    }
    fn view(&self) -> record::View {
        self.records().view().unwrap()
    }
    /// The current value of a settled Entity.
    fn current(&self, id: &str) -> Current {
        self.view().current(&eid(id)).unwrap().clone()
    }
    /// The record files under `.axon/records/`, relative, sorted.
    fn record_files(&self) -> Vec<PathBuf> {
        record_files(&self.0)
    }
    /// Publishes records built in memory, as one batch under the writer's lock.
    fn publish(&self, entries: Vec<Entry>) {
        self.store().update(|_, _, _| Ok((entries, ()))).unwrap();
    }
}
fn record_files(root: &Path) -> Vec<PathBuf> {
    let base = root.join(".axon/records");
    let mut found = Vec::new();
    let Ok(subdirectories) = fs::read_dir(&base) else {
        return found;
    };
    for subdirectory in subdirectories {
        let subdirectory = subdirectory.unwrap();
        if !subdirectory.file_type().unwrap().is_dir() {
            found.push(
                subdirectory
                    .path()
                    .strip_prefix(&base)
                    .unwrap()
                    .to_path_buf(),
            );
            continue;
        }
        for file in fs::read_dir(subdirectory.path()).unwrap() {
            let path = file.unwrap().path();
            found.push(path.strip_prefix(&base).unwrap().to_path_buf());
        }
    }
    found.sort();
    found
}
/// Copies every record file of `from` into `to`: what a Git merge of two tracked stores does.
fn merge_records(from: &Path, to: &Path) {
    for relative in record_files(from) {
        let source = from.join(".axon/records").join(&relative);
        let destination = to.join(".axon/records").join(&relative);
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::copy(source, destination).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn command(path: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_axon"));
    cmd.current_dir(path);
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("GIT_") {
            cmd.env_remove(name);
        }
    }
    cmd.env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null");
    cmd
}
fn success(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}
fn failure(output: Output) -> String {
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    String::from_utf8(output.stderr).unwrap()
}
/// A `git` that ignores the caller's repository selection and configuration.
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
fn git_output(path: &Path, args: &[&str]) -> Output {
    isolated_git(path).args(args).output().unwrap()
}
fn git(path: &Path, args: &[&str]) {
    success(git_output(path, args));
}
/// Git with a fixed identity and no hooks, for the operations that create commits.
fn git_integration(path: &Path, args: &[&str]) -> Output {
    let mut all = vec![
        "-c",
        "user.name=Test",
        "-c",
        "user.email=test@example.com",
        "-c",
        "core.hooksPath=/dev/null",
    ];
    all.extend_from_slice(args);
    git_output(path, &all)
}
/// A commit with a fixed identity and no hooks; `args` follows `commit`.
fn git_commit(path: &Path, args: &[&str]) {
    let mut all = vec!["commit"];
    all.extend_from_slice(args);
    success(git_integration(path, &all));
}
/// A linked worktree on a new branch cut from the main worktree's HEAD.
fn add_worktree(main: &Path, name: &str) -> Fixture {
    let linked = Fixture::new();
    git(
        main,
        &["worktree", "add", "-qb", name, linked.0.to_str().unwrap()],
    );
    linked
}
/// The ignored operation, as `axon init` displays it: the shown line, added to the ignore file
/// of this repository alone. The isolated template may carry no `info` directory yet.
fn ignore_store(root: &Path, line: &str) {
    let shown =
        String::from_utf8(git_output(root, &["rev-parse", "--git-path", "info/exclude"]).stdout)
            .unwrap();
    let exclude = root.join(shown.trim());
    fs::create_dir_all(exclude.parent().unwrap()).unwrap();
    let mut rules = fs::read_to_string(&exclude).unwrap_or_default();
    rules.push_str(line);
    rules.push('\n');
    fs::write(&exclude, rules).unwrap();
}
/// The tracked operation, whose step `axon init` prints instead of applying it.
fn track_store(root: &Path) {
    git(root, &["add", ".axon"]);
}
fn context() -> Context {
    Context {
        at: Utc::now(),
        recorder: None,
    }
}
fn eid(value: &str) -> EntityId {
    value.to_string().try_into().unwrap()
}
fn current(title: &str) -> Current {
    Current {
        kind: Kind::Issue,
        lifecycle: Lifecycle::NotStarted,
        owner: None,
        title: title.into(),
        description: String::new(),
        condition: None,
        parent: None,
        needs: BTreeSet::new(),
    }
}
/// A registration record for a NotStarted Issue with a fixed ID, as a library caller makes it.
fn registration(records: &Store, id: &str) -> Entry {
    Entry::Record(records.create(eid(id), current(id), context()).unwrap())
}

#[test]
fn registration_to_group_completion_and_records() {
    let f = Fixture::new();
    f.init();
    let group = f
        .ok(&[
            "capture",
            "--kind",
            "group",
            "--title",
            "計画",
            "-m",
            "計画本文",
        ])
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    let dependency = f.accepted("先行");
    let issue = f
        .ok(&[
            "capture",
            "--title",
            "実装",
            "--parent",
            &group,
            "--needs",
            &dependency,
            "-m",
            "保存本文",
        ])
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    assert!(f.ok(&["show", &issue]).contains("Undecided"));
    f.ok(&["accept", &issue]);
    let wait = f.ok(&["show", &issue]);
    assert!(wait.contains("Required to start"));
    assert!(wait.contains(&format!("Ancestor must be adopted: {group}")));
    assert!(wait.contains("Dependency must complete:"));
    assert!(!wait.contains("Parent:"));
    failure(f.run(&["start", &issue]));
    f.ok(&["accept", &group]);
    assert!(f.ok(&["show", &issue]).contains("Parent:"));
    failure(f.run(&["start", &issue]));
    failure(f.run(&["start", &group]));
    f.ok(&["start", &dependency]);
    f.ok(&["complete", &dependency]);
    f.ok(&[
        "write",
        &issue,
        "--title",
        "実装済みの目的",
        "-m",
        "編集本文",
    ]);
    f.ok(&["start", &issue]);
    assert!(f.ok(&["show", &group]).contains("Group  InProgress  計画"));
    failure(f.run(&["release", &group]));
    f.ok(&["note", "add", &issue, "-m", "検証結果"]);
    failure(f.run(&["complete", &group]));
    let show = f.ok(&["show", &issue]);
    assert!(show.starts_with(&issue));
    assert!(show.contains("1 notes"));
    assert!(show.contains("編集本文"));
    assert!(!show.contains("検証結果"));
    assert!(!show.contains("Dependency must complete:"));
    f.ok(&["complete", &issue, "--reason", "検証完了"]);
    assert!(
        f.ok(&["show", &group])
            .contains("1/1 terminal (1 completed, 0 cancelled)")
    );
    assert!(
        f.ok(&["show", &group])
            .contains("Awaiting final confirmation")
    );
    f.ok(&["complete", &group]);
    assert!(f.ok(&["note", "list", &issue]).contains("検証結果"));
    let log = f.ok(&["log", &issue]);
    assert!(log.contains("InProgress → Completed"));
    assert!(log.contains("検証完了"));
    assert!(!log.contains("record-"));
    let log = f.ok(&["log", &group]);
    assert!(log.contains("NotStarted → Completed"));
    assert!(!log.contains("InProgress"));
    let list = f.ok(&["list"]);
    assert!(list.find(&group) < list.find(&dependency));
    assert!(list.find(&dependency) < list.find(&issue));
    let files = f.record_files();
    failure(f.run(&["reconsider", &issue]));
    assert_eq!(files, f.record_files());
}
#[test]
fn lifecycle_and_relation_edits_use_common_guards() {
    let f = Fixture::new();
    f.init();
    let a = f.accepted("A");
    let b = f.accepted("B");
    f.ok(&["dep", "add", &a, "--needs", &b]);
    failure(f.run(&["dep", "add", &b, "--needs", &a]));
    f.ok(&["dep", "rm", &a, "--needs", &b]);
    f.ok(&["withdraw", &a]);
    f.ok(&["accept", &a]);
    f.ok(&["start", &a]);
    f.ok(&["release", &a]);
    f.ok(&["cancel", &a]);
    f.ok(&["reconsider", &a]);
    let g = f
        .ok(&["capture", "--kind", "group", "--title", "G"])
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    f.ok(&["parent", "set", &a, "--parent", &g]);
    assert!(f.ok(&["show", &g]).contains(&a));
    f.ok(&["parent", "unset", &a]);
    f.ok(&["cancel", &g]);
    failure(f.run(&["parent", "set", &a, "--parent", &g]));
}
#[test]
fn stdin_files_help_invalid_arguments_and_terminal_controls() {
    let f = Fixture::new();
    f.init();
    let body = f.0.join("body.txt");
    fs::write(&body, "long\n本文\x1b[2J").unwrap();
    let id = f
        .ok(&[
            "capture",
            "--accept",
            "--title",
            "safe",
            "-F",
            body.to_str().unwrap(),
        ])
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    let show = f.ok(&["show", &id]);
    assert!(!show.contains('\x1b'));
    assert!(show.contains("  long\n  本文\\x1b[2J"));
    let rejected = failure(f.run(&["write", &id, "--title", "unsafe\x1b[2J"]));
    assert!(rejected.contains("control character"), "{rejected}");
    assert!(!rejected.contains('\x1b'));
    let mut child = f
        .command()
        .args(["note", "add", &id, "-F", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all("stdin本文".as_bytes())
        .unwrap();
    success(child.wait_with_output().unwrap());
    assert!(f.ok(&["note", "list", &id]).contains("stdin本文"));
    failure(f.run(&["write", &id]));
    failure(f.run(&["note", "add", &id]));
    failure(f.run(&["write", &id, "-m", "x", "-F", "-"]));
    let help = f.ok(&["--help"]);
    let listed = |name: &str| {
        help.lines()
            .any(|line| line.trim_start().starts_with(&format!("{name} ")))
    };
    for name in ["complete", "parent", "condition"] {
        assert!(listed(name), "{help}");
    }
    assert!(f.ok(&["complete", "--help"]).contains("final review"));
}
#[test]
fn concurrent_start_has_one_winner_and_other_writes_survive() {
    let f = Fixture::new();
    f.init();
    let id = f.accepted("start");
    let first = f
        .command()
        .args(["start", &id])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let second = f
        .command()
        .args(["start", &id])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    assert_ne!(
        first.wait_with_output().unwrap().status.success(),
        second.wait_with_output().unwrap().status.success()
    );
    let children: Vec<_> = (0..12)
        .map(|n| {
            f.command()
                .args(["note", "add", &id, "-m", &format!("note {n}")])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    for child in children {
        success(child.wait_with_output().unwrap());
    }
    let children: Vec<_> = (0..8)
        .map(|n| {
            f.command()
                .args(["capture", "--accept", "--title", &format!("entity {n}")])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    for child in children {
        success(child.wait_with_output().unwrap());
    }
    let records = f.records();
    assert_eq!(records.notes_of(&eid(&id)).len(), 12);
    assert_eq!(records.view().unwrap().known().count(), 9);
    assert_eq!(records.history(&eid(&id)).unwrap().len(), 2);
}
#[test]
fn concurrent_branches_are_read_as_a_conflict_that_stops_ordinary_operations() {
    let f = Fixture::new();
    f.init();
    f.publish(vec![registration(&f.records(), "t-item")]);
    f.ok(&["start", "t-item"]);
    // Two copies of the store diverge: one completes the Issue, the other releases it.
    let left = Fixture::new();
    let right = Fixture::new();
    for side in [&left, &right] {
        fs::create_dir(side.0.join(".axon")).unwrap();
        fs::copy(f.header(), side.header()).unwrap();
        merge_records(&f.0, &side.0);
    }
    left.ok(&["complete", "t-item"]);
    left.ok(&["note", "add", "t-item", "-m", "same"]);
    right.ok(&["release", "t-item"]);
    right.ok(&["note", "add", "t-item", "-m", "same"]);
    merge_records(&right.0, &left.0);
    let log = left.ok(&["log", "t-item"]);
    assert!(log.contains("Concurrent branch"), "{log}");
    assert!(log.contains("InProgress → Completed") && log.contains("InProgress → NotStarted"));
    assert!(log.contains("Conflicted: 2 heads"), "{log}");
    let notes = left.ok(&["note", "list", "t-item"]);
    assert_eq!(notes.matches("same").count(), 2);
    assert!(!notes.contains("Concurrent branch"));
    let show = left.ok(&["show", "t-item"]);
    assert!(show.contains("Issue  Conflicted  t-item"), "{show}");
    assert!(show.contains("Conflicted\n"), "{show}");
    let list = left.run(&["list"]);
    assert!(list.status.success());
    assert!(String::from_utf8_lossy(&list.stderr).contains("axon storage check"));
    // A conflicted store accepts Notes and rejects everything else without a record.
    let before = left.record_files();
    let rejected = failure(left.run(&["start", "t-item"]));
    assert!(rejected.contains("conflicted"), "{rejected}");
    failure(left.run(&["capture", "--title", "blocked"]));
    assert_eq!(left.record_files(), before);
    left.ok(&["note", "add", "t-item", "-m", "still allowed"]);
    // Parallel writers of the same Entity both succeed: each adds its own record file.
    let barrier = Arc::new(Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let root = left.0.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut store = Location::discover(&root, false).unwrap().open().unwrap();
                barrier.wait();
                store
                    .update(|_, records, _| {
                        let note =
                            records.add_note(&eid("t-item"), "parallel".into(), None, context())?;
                        Ok((vec![Entry::Note(note)], ()))
                    })
                    .unwrap();
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
    assert_eq!(left.records().notes_of(&eid("t-item")).len(), 5);
}
#[test]
fn log_lists_each_concurrent_branch_together_with_one_branch_boundary() {
    let f = Fixture::new();
    f.init();
    f.publish(vec![registration(&f.records(), "t-item")]);
    let left = Fixture::new();
    let right = Fixture::new();
    for side in [&left, &right] {
        fs::create_dir(side.0.join(".axon")).unwrap();
        fs::copy(f.header(), side.header()).unwrap();
        merge_records(&f.0, &side.0);
    }
    // Several records per side, so that the sides can interleave unless kept together.
    for (side, name) in [(&left, "left"), (&right, "right")] {
        for n in 1..=3 {
            side.ok(&["start", "t-item"]);
            side.ok(&["release", "t-item", "-r", &format!("{name} {n}")]);
        }
    }
    merge_records(&right.0, &left.0);
    merge_records(&left.0, &right.0);
    let log = left.ok(&["log", "t-item"]);
    assert_eq!(log, right.ok(&["log", "t-item"]));
    assert!(log.contains("Conflicted: 2 heads"), "{log}");
    assert_eq!(log.matches("Concurrent branch").count(), 1, "{log}");
    let at = |text: &str| log.find(text).unwrap_or_else(|| panic!("{text}: {log}"));
    let boundary = at("Concurrent branch");
    let (first, second) = if at("left 1") < at("right 1") {
        ("left", "right")
    } else {
        ("right", "left")
    };
    let positions: Vec<_> = [first, second]
        .iter()
        .flat_map(|name| (1..=3).map(move |n| format!("{name} {n}")))
        .map(|text| at(&text))
        .collect();
    assert!(positions.is_sorted(), "{log}");
    assert!(positions[2] < boundary && boundary < positions[3], "{log}");
}
#[test]
fn log_finishes_a_nested_fork_before_moving_to_another_branch() {
    let f = Fixture::new();
    f.init();
    f.publish(vec![registration(&f.records(), "t-item")]);
    let copy = |from: &Fixture| {
        let side = Fixture::new();
        fs::create_dir(side.0.join(".axon")).unwrap();
        fs::copy(from.header(), side.header()).unwrap();
        merge_records(&from.0, &side.0);
        side
    };
    // One side starts and then forks into two releases; the other side starts and releases.
    let left = copy(&f);
    let right = copy(&f);
    left.ok(&["start", "t-item"]);
    let fork = copy(&left);
    left.ok(&["release", "t-item", "-r", "left inner"]);
    fork.ok(&["release", "t-item", "-r", "fork inner"]);
    right.ok(&["start", "t-item"]);
    right.ok(&["release", "t-item", "-r", "right outer"]);
    merge_records(&fork.0, &left.0);
    merge_records(&right.0, &left.0);
    let log = left.ok(&["log", "t-item"]);
    assert!(log.contains("Conflicted: 3 heads"), "{log}");
    // Two switches, one per branch end, and the two inner releases stay next to each other.
    // The record IDs vary per run; the unit tests of the history order pin the IDs apart.
    assert_eq!(log.matches("Concurrent branch").count(), 2, "{log}");
    let lines: Vec<_> = log.lines().collect();
    let at = |text: &str| {
        lines
            .iter()
            .position(|line| line.contains(text))
            .unwrap_or_else(|| panic!("{text}: {log}"))
    };
    let (inner, other) = (
        at("left inner").min(at("fork inner")),
        at("left inner").max(at("fork inner")),
    );
    assert_eq!(other - inner, 2, "{log}");
    assert!(lines[inner + 1].starts_with("Concurrent branch"), "{log}");
}
#[test]
fn log_names_an_edit_record_without_field_changes_and_its_reason() {
    let f = Fixture::new();
    f.init();
    let created = registration(&f.records(), "t-item");
    let mut parent = created.id().unwrap();
    f.publish(vec![created]);
    // Edit records with the parent's own values, as a storage migration may write them, and
    // an edit that changes the title and carries a reason, which keeps its display.
    for (reason, title) in [
        (Some("copied from the earlier format"), "t-item"),
        (None, "t-item"),
        (Some("retitled"), "renamed"),
    ] {
        let edit = Entry::Record(record::Record {
            entity: eid("t-item"),
            kind: record::RecordKind::Edit,
            parents: BTreeSet::from([parent]),
            at: Utc::now(),
            recorder: None,
            reason: reason.map(Into::into),
            after: current(title),
        });
        parent = edit.id().unwrap();
        f.publish(vec![edit]);
    }
    let log = f.ok(&["log", "t-item"]);
    let lines: Vec<_> = log.lines().collect();
    assert_eq!(lines.len(), 4, "{log}");
    assert!(
        lines[1].ends_with("Edited: no field changes  Reason: copied from the earlier format"),
        "{log}"
    );
    assert!(lines[2].ends_with("Edited: no field changes"), "{log}");
    assert!(lines[3].ends_with("Edited: title"), "{log}");
}
#[test]
fn unsupported_corrupt_and_earlier_format_stores_are_rejected_without_changes() {
    for kind in ["wrong-format", "corrupt-header", "earlier-format"] {
        let f = Fixture::new();
        fs::create_dir(f.0.join(".axon")).unwrap();
        let path = match kind {
            "wrong-format" => {
                fs::write(
                    f.header(),
                    "{\"format\":\"axon-records/v2\",\"store\":\"store-1\",\"prefix\":\"t\"}\n",
                )
                .unwrap();
                f.header()
            }
            "corrupt-header" => {
                fs::write(f.header(), b"not a header").unwrap();
                f.header()
            }
            _ => {
                let state = f.0.join(".axon/state.jsonl");
                fs::write(&state, "{\"format\":\"axon-file/v1\",\"prefix\":\"t\"}\n").unwrap();
                state
            }
        };
        let before = fs::read(&path).unwrap();
        let error = failure(f.run(&["list"]));
        assert!(!error.contains("not initialized"), "{kind}: {error}");
        failure(f.run(&["init"]));
        assert_eq!(before, fs::read(&path).unwrap(), "{kind}");
        assert!(!f.records_dir().exists(), "{kind}");
    }
}
#[test]
fn discovery_outside_git_and_git_boundary() {
    let f = Fixture::new();
    f.init();
    let id = f.accepted("outer");
    let nested = f.0.join("nested/deeper");
    fs::create_dir_all(nested.join(".axon")).unwrap();
    fs::write(nested.join(".axon/write.lock"), "").unwrap();
    assert!(success(command(&nested).args(["list"]).output().unwrap()).contains(&id));
    failure(command(&nested).args(["init"]).output().unwrap());
    // A file that is not the residue of an initialization stops discovery here.
    fs::write(nested.join(".axon/state.jsonl"), "").unwrap();
    assert!(failure(command(&nested).args(["list"]).output().unwrap()).contains("not a store"));
    fs::remove_file(nested.join(".axon/state.jsonl")).unwrap();
    git(&nested, &["init", "--quiet"]);
    assert!(failure(command(&nested).args(["list"]).output().unwrap()).contains("not initialized"));
    success(command(&nested).args(["init", "inner"]).output().unwrap());
    assert!(success(command(&nested).args(["list"]).output().unwrap()).is_empty());
    assert!(f.ok(&["list"]).contains(&id));
}
#[test]
fn concurrent_initialization_never_replaces_a_store() {
    let f = Fixture::new();
    let children: Vec<_> = (0..4)
        .map(|_| {
            f.command()
                .args(["init"])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    let results: Vec<_> = children
        .into_iter()
        .map(|c| c.wait_with_output().unwrap())
        .collect();
    assert_eq!(results.iter().filter(|o| o.status.success()).count(), 1);
    assert!(!f.0.join(".axon/header.json.tmp").exists());
    f.store();
}
#[test]
fn broken_pipe_is_success_after_storage_is_applied() {
    let f = Fixture::new();
    f.init();
    let mut child = f
        .command()
        .args(["capture", "--accept", "--title", "retained"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    assert!(out.stderr.is_empty());
    assert!(f.ok(&["list"]).contains("retained"));
}

#[test]
fn git_repository_paths_keep_trailing_whitespace() {
    let f = Fixture::new();
    let plain = f.0.join("repo");
    let spaced = f.0.join("repo ");
    for path in [&plain, &spaced] {
        fs::create_dir(path).unwrap();
        git(path, &["init", "--quiet"]);
        success(command(path).args(["init", "repo"]).output().unwrap());
    }
    success(
        command(&spaced)
            .args(["capture", "--accept", "--title", "only in spaced"])
            .output()
            .unwrap(),
    );
    assert!(success(command(&plain).args(["list"]).output().unwrap()).is_empty());
    assert!(success(command(&spaced).args(["list"]).output().unwrap()).contains("only in spaced"));
    let identity = |root: &Path| {
        record::decode_header(&fs::read(root.join(".axon/header.json")).unwrap())
            .unwrap()
            .store
    };
    assert_ne!(identity(&plain), identity(&spaced));
}

#[cfg(unix)]
#[test]
fn broken_git_marker_blocks_ancestor_storage_fallback() {
    let f = Fixture::new();
    f.init();
    f.accepted("outer");
    let nested = f.0.join("nested");
    fs::create_dir(&nested).unwrap();
    std::os::unix::fs::symlink("missing-git-directory", nested.join(".git")).unwrap();
    let before = f.record_files();
    for args in [
        vec!["list"],
        vec!["init"],
        vec!["capture", "--accept", "--title", "must not reach outer"],
    ] {
        assert!(
            failure(command(&nested).args(args).output().unwrap()).contains("Git discovery failed")
        );
    }
    assert_eq!(before, f.record_files());
}
#[cfg(unix)]
#[test]
fn initialization_path_never_emits_terminal_controls() {
    let f = Fixture::new();
    let path = f.0.join("repo\x1b[2J");
    fs::create_dir(&path).unwrap();
    let rejected = failure(command(&path).args(["init"]).output().unwrap());
    assert!(!rejected.contains('\x1b'));
    assert!(rejected.contains("repo\\x1b[2J"), "{rejected}");
    assert!(!path.join(".axon").exists());
    let out = success(command(&path).args(["init", "repo"]).output().unwrap());
    assert!(!out.contains('\x1b'));
    assert!(out.contains("repo\\x1b[2J"));
    assert!(path.join(".axon/header.json").is_file());
}

#[cfg(unix)]
#[test]
fn git_cannot_skip_a_broken_inner_marker_to_an_outer_repository() {
    let f = Fixture::new();
    git(&f.0, &["init", "--quiet"]);
    f.init();
    let nested = f.0.join("nested");
    fs::create_dir(&nested).unwrap();
    std::os::unix::fs::symlink("missing", nested.join(".git")).unwrap();
    let before = f.record_files();
    for args in [
        vec!["list"],
        vec!["init"],
        vec!["capture", "--accept", "--title", "wrong store"],
    ] {
        assert!(
            failure(command(&nested).args(args).output().unwrap()).contains("Git discovery failed")
        );
    }
    assert_eq!(before, f.record_files());
}
#[test]
fn bare_repository_is_a_boundary_without_a_dot_git_entry() {
    let f = Fixture::new();
    f.init();
    let bare = f.0.join("bare.git");
    fs::create_dir(&bare).unwrap();
    git(&bare, &["init", "--bare", "--quiet"]);
    let before = f.record_files();
    for args in [
        vec!["list"],
        vec!["init"],
        vec!["capture", "--accept", "--title", "wrong store"],
    ] {
        assert!(
            failure(command(&bare).args(args).output().unwrap()).contains("Git discovery failed")
        );
    }
    assert_eq!(before, f.record_files());
}

#[test]
fn inherited_git_overrides_do_not_select_a_foreign_store() {
    let f = Fixture::new();
    let repo = f.0.join("repo");
    fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "--quiet"]);
    success(command(&repo).args(["init"]).output().unwrap());
    let before = record_files(&repo);
    let outside = f.0.join("outside");
    fs::create_dir(&outside).unwrap();
    let out = command(&outside)
        .env("GIT_DIR", repo.join(".git"))
        .env("GIT_WORK_TREE", &repo)
        .env("GIT_COMMON_DIR", repo.join(".git"))
        .args(["capture", "--accept", "--title", "wrong store"])
        .output()
        .unwrap();
    assert!(failure(out).contains("not initialized"));
    assert_eq!(before, record_files(&repo));
    success(
        command(&outside)
            .env("GIT_DIR", repo.join(".git"))
            .args(["init"])
            .output()
            .unwrap(),
    );
    assert!(outside.join(".axon/header.json").is_file());
    assert_eq!(before, record_files(&repo));
}

fn set_condition(f: &Fixture, id: &str, command: &str) {
    f.ok(&["condition", "set", id, "--command", command]);
}
fn new_entity(f: &Fixture, args: &[&str]) -> String {
    f.ok(args).split_whitespace().next().unwrap().into()
}

#[test]
fn candidate_sets_and_lazy_ancestor_evaluation_are_shared_only_within_invocation() {
    let f = Fixture::new();
    f.init();
    let root = new_entity(
        &f,
        &["capture", "--kind", "group", "--accept", "--title", "root"],
    );
    let nested = new_entity(
        &f,
        &[
            "capture", "--kind", "group", "--accept", "--title", "nested", "--parent", &root,
        ],
    );
    let dep = f.accepted("dependency");
    let child = new_entity(
        &f,
        &[
            "capture", "--accept", "--title", "child", "--parent", &nested, "--needs", &dep,
        ],
    );
    let draft = new_entity(&f, &["capture", "--title", "draft", "--parent", &nested]);
    set_condition(&f, &root, "echo root >> observations; test -f open");
    set_condition(&f, &nested, "echo nested >> observations");
    set_condition(&f, &child, "echo child >> observations");
    set_condition(&f, &draft, "echo draft >> observations");
    let initial = f.ok(&["tasks"]);
    assert!(initial.contains(&dep));
    assert!(!initial.contains(&root));
    assert!(!initial.contains(&child));
    assert_eq!(
        fs::read_to_string(f.0.join("observations")).unwrap(),
        "root\n"
    );
    fs::write(f.0.join("open"), "").unwrap();
    fs::write(f.0.join("observations"), "").unwrap();
    let result = f.run(&["tasks", "--trace-conditions"]);
    assert!(result.status.success());
    let rows = String::from_utf8(result.stdout).unwrap();
    for id in [&root, &nested, &child, &dep] {
        assert!(rows.contains(id));
    }
    assert!(!rows.contains(&draft));
    assert!(
        rows.lines()
            .find(|s| s.starts_with(&child))
            .unwrap()
            .contains("Blocked")
    );
    assert_eq!(
        fs::read_to_string(f.0.join("observations")).unwrap(),
        "root\nnested\nchild\n"
    );
    let trace = String::from_utf8(result.stderr).unwrap();
    assert_eq!(trace.matches("Condition trace:").count(), 3);
    assert!(trace.find(&root).unwrap() < trace.find(&nested).unwrap());
    assert!(trace.find(&nested).unwrap() < trace.find(&child).unwrap());
    fs::write(f.0.join("observations"), "").unwrap();
    assert!(f.ok(&["proposals"]).contains(&draft));
    assert_eq!(
        fs::read_to_string(f.0.join("observations")).unwrap(),
        "root\nnested\ndraft\n"
    );
    f.ok(&["start", &dep]);
    f.ok(&["complete", &dep]);
    f.ok(&["start", &child]);
    fs::remove_file(f.0.join("open")).unwrap();
    fs::write(f.0.join("observations"), "").unwrap();
    // Working Groups are listed although they do not surface, and only the root is
    // evaluated because nothing below an unsurfaced ancestor is.
    let rows = f.ok(&["tasks"]);
    for id in [&root, &nested, &child] {
        assert!(rows.contains(id));
    }
    assert!(
        rows.lines()
            .filter(|s| s.starts_with(&root) || s.starts_with(&nested))
            .all(|s| s.contains("Group  InProgress  "))
    );
    assert_eq!(
        fs::read_to_string(f.0.join("observations")).unwrap(),
        "root\n"
    );
    // A working Group's own condition is still evaluated, so its failure fails the list.
    set_condition(&f, &root, "exit 23");
    assert!(failure(f.run(&["tasks"])).contains("exit status: 23"));
    assert!(failure(f.run(&["proposals"])).contains("exit status: 23"));
}

#[test]
fn conditions_preserve_saved_state_and_explicit_operations_never_evaluate() {
    let f = Fixture::new();
    f.init();
    let root = new_entity(
        &f,
        &["capture", "--kind", "group", "--accept", "--title", "root"],
    );
    let id = new_entity(&f, &["capture", "--title", "item", "--parent", &root]);
    let script = "echo executed >> forbidden; exit 23";
    set_condition(&f, &root, script);
    set_condition(&f, &id, script);
    for args in [
        vec!["list"],
        vec!["show", &id],
        vec!["log", &id],
        vec!["note", "list", &id],
    ] {
        f.ok(&args);
    }
    f.ok(&["accept", &id]);
    f.ok(&["withdraw", &id]);
    f.ok(&["accept", &id]);
    f.ok(&["start", &id]);
    f.ok(&["release", &id]);
    f.ok(&["cancel", &id]);
    set_condition(&f, &id, script);
    f.ok(&["reconsider", &id]);
    f.ok(&["write", &id, "--title", "changed"]);
    f.ok(&["parent", "unset", &id]);
    f.ok(&["parent", "set", &id, "--parent", &root]);
    f.ok(&["accept", &id]);
    f.ok(&["start", &id]);
    f.ok(&["complete", &id]);
    f.ok(&["reopen", &id]);
    f.ok(&["start", &id]);
    f.ok(&["complete", &id]);
    f.ok(&["complete", &root]);
    f.ok(&["reopen", &root]);
    f.ok(&["complete", &root]);
    set_condition(&f, &id, "exit 2");
    f.ok(&["condition", "unset", &id]);
    f.ok(&["note", "add", &id, "-m", "supplement"]);
    assert!(!f.0.join("forbidden").exists());
    let before = f.records();
    assert!(f.ok(&["tasks"]).is_empty());
    assert!(f.ok(&["proposals"]).is_empty());
    assert!(
        failure(f.run(&["condition", "set", &id, "--command", " "])).contains("empty condition")
    );
    assert_eq!(f.records(), before);
}

#[test]
fn condition_results_diagnostics_and_repair_do_not_publish_partial_rows() {
    let f = Fixture::new();
    f.init();
    let ongoing = f.accepted("ongoing");
    f.ok(&["start", &ongoing]);
    let id = f.accepted("condition");
    set_condition(
        &f,
        &id,
        "printf '\\033bad\\377'; printf problem >&2; exit 23",
    );
    let before = f.records();
    let error = failure(f.run(&["tasks", "--trace-conditions"]));
    for text in [
        &id,
        f.0.to_str().unwrap(),
        "exit status: 23",
        "\\x1bbad�",
        "problem",
    ] {
        assert!(error.contains(text), "{error}");
    }
    assert!(!error.contains("Condition trace:"));
    assert_eq!(f.records(), before);
    set_condition(&f, &id, "exit 1");
    let out = f.run(&["tasks", "--trace-conditions"]);
    assert!(!String::from_utf8(out.stdout).unwrap().contains(&id));
    let trace = String::from_utf8(out.stderr).unwrap();
    assert!(trace.contains("not satisfied (exit 1)"));
    assert_eq!(trace.matches("(empty)").count(), 2);
    set_condition(&f, &id, "echo ignored; echo ignored >&2; exit 0");
    let out = f.run(&["tasks"]);
    assert!(out.status.success() && out.stderr.is_empty());
    assert!(!String::from_utf8(out.stdout).unwrap().contains("ignored"));
    set_condition(&f, &id, "echo signal-detail >&2; kill -TERM $$");
    assert!(failure(f.run(&["tasks"])).contains("signal-detail"));
    f.ok(&["condition", "unset", &id]);
    assert!(f.ok(&["tasks"]).contains(&id));
}

#[test]
fn condition_output_keeps_both_edges_per_stream_in_trace_and_failure() {
    let f = Fixture::new();
    f.init();
    let id = f.accepted("output");
    let script = "awk 'BEGIN { for (i=0;i<40000;i++) printf \"A\"; for (i=0;i<40000;i++) printf \"B\" }'; awk 'BEGIN { for (i=0;i<40000;i++) printf \"C\"; for (i=0;i<40000;i++) printf \"D\" }' >&2";
    for exit in [0, 23] {
        set_condition(&f, &id, &format!("{script}; exit {exit}"));
        let out = f.run(&["tasks", "--trace-conditions"]);
        assert_eq!(out.status.success(), exit == 0);
        let err = String::from_utf8(out.stderr).unwrap();
        for character in ['A', 'B', 'C', 'D'] {
            assert!(err.contains(&character.to_string().repeat(32768)));
        }
        assert_eq!(err.matches("14464 bytes omitted").count(), 2);
        assert!(err.len() < 133000);
        if exit != 0 {
            assert!(out.stdout.is_empty());
        }
    }
}

fn wait_pid(path: &Path) -> i32 {
    let start = std::time::Instant::now();
    loop {
        if let Ok(text) = fs::read_to_string(path)
            && let Ok(pid) = text.trim().parse()
        {
            return pid;
        }
        assert!(
            start.elapsed().as_secs() < 10,
            "PID not ready: {}",
            path.display()
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
fn assert_process_gone(pid: i32) {
    let start = std::time::Instant::now();
    loop {
        if unsafe { libc::kill(pid, 0) } == -1 {
            return;
        }
        let output = Command::new("ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .output()
            .unwrap();
        if String::from_utf8_lossy(&output.stdout)
            .trim()
            .starts_with('Z')
        {
            return;
        }
        assert!(start.elapsed().as_secs() < 5, "process {pid} remains");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
#[test]
fn condition_timeout_and_ctrl_c_terminate_shell_and_descendants() {
    let f = Fixture::new();
    f.init();
    let id = f.accepted("interrupt");
    let script = "echo $$ > shell-pid; sh -c 'trap \"\" TERM; echo $$ > descendant-pid; while :; do :; done' </dev/null >/dev/null 2>/dev/null & wait";
    set_condition(&f, &id, script);
    let before = f.records();
    for interrupt in [false, true] {
        let mut cmd = f.command();
        cmd.args([
            "tasks",
            "--condition-timeout",
            if interrupt { "10s" } else { "500ms" },
        ]);
        let out = if interrupt {
            fs::remove_file(f.0.join("descendant-pid")).ok();
            let child = cmd
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            wait_pid(&f.0.join("descendant-pid"));
            assert_eq!(unsafe { libc::kill(child.id() as i32, libc::SIGINT) }, 0);
            child.wait_with_output().unwrap()
        } else {
            cmd.output().unwrap()
        };
        let error = failure(out);
        assert!(
            error.contains(if interrupt {
                "interrupted by Ctrl-C"
            } else {
                "timed out after 500ms"
            }),
            "{error}"
        );
        assert!(
            error.contains("TERM followed by KILL after 1s grace"),
            "{error}"
        );
        assert_process_gone(wait_pid(&f.0.join("shell-pid")));
        assert_process_gone(wait_pid(&f.0.join("descendant-pid")));
        assert_eq!(f.records(), before);
    }
}

#[test]
fn condition_default_timeout_is_thirty_seconds_in_a_real_process() {
    let f = Fixture::new();
    f.init();
    let id = f.accepted("default timeout");
    set_condition(&f, &id, "sleep 60");
    let start = std::time::Instant::now();
    let error = failure(f.run(&["tasks"]));
    assert!(start.elapsed() >= std::time::Duration::from_secs(30));
    assert!(error.contains("timed out after 30s"), "{error}");
}

#[test]
fn candidate_help_timeout_validation_and_trace_sink_failure() {
    use std::os::fd::{FromRawFd, OwnedFd};
    let f = Fixture::new();
    f.init();
    let id = f.accepted("trace");
    let help = f.ok(&["tasks", "--help"]);
    for text in [
        "--condition-timeout",
        "--trace-conditions",
        "30",
        "/bin/sh",
        "64 KiB",
        "TERM",
        "KILL",
    ] {
        assert!(help.contains(text));
    }
    for value in ["0", "-1", "NaN", "inf", "1e100", "1e-100", "wrong"] {
        failure(f.run(&["tasks", "--condition-timeout", value]));
    }
    set_condition(&f, &id, "echo ran >> observed");
    let mut cmd = f.command();
    cmd.args(["tasks", "--trace-conditions"]);
    let mut pipe = [0; 2];
    assert_eq!(unsafe { libc::pipe(pipe.as_mut_ptr()) }, 0);
    assert_eq!(unsafe { libc::close(pipe[0]) }, 0);
    let writer = unsafe { OwnedFd::from_raw_fd(pipe[1]) };
    let out = cmd.stderr(Stdio::from(writer)).output().unwrap();
    assert!(!out.status.success() && out.stdout.is_empty());
    assert_eq!(fs::read_to_string(f.0.join("observed")).unwrap(), "ran\n");
}

#[test]
fn condition_uses_current_worktree_or_management_root_and_inherits_environment() {
    let f = Fixture::new();
    f.init();
    let id = f.accepted("cwd");
    set_condition(
        &f,
        &id,
        "pwd; test \"$AXON_TEST_CONDITION\" = inherited; test -f local-file",
    );
    fs::write(f.0.join("local-file"), "").unwrap();
    fs::create_dir(f.0.join("subdir")).unwrap();
    let out = command(&f.0.join("subdir"))
        .env("AXON_TEST_CONDITION", "inherited")
        .args(["tasks", "--trace-conditions"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8(out.stdout).unwrap().contains(&id));
    assert!(
        String::from_utf8(out.stderr)
            .unwrap()
            .contains(&format!("cwd: {}", f.0.display()))
    );
    git(&f.0, &["init", "-q"]);
    git(
        &f.0,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "initial",
        ],
    );
    let other = Fixture::new();
    git(
        &f.0,
        &["worktree", "add", "--detach", other.0.to_str().unwrap()],
    );
    fs::create_dir(other.0.join("subdir")).unwrap();
    let script = "test \"$AXON_TEST_CONDITION\" = inherited && test -f local-file";
    set_condition(&f, &id, script);
    let out = command(&other.0.join("subdir"))
        .env("AXON_TEST_CONDITION", "inherited")
        .args(["tasks", "--trace-conditions"])
        .output()
        .unwrap();
    assert!(out.status.success() && out.stdout.is_empty());
    assert!(
        String::from_utf8(out.stderr)
            .unwrap()
            .contains(&format!("cwd: {}", other.0.display()))
    );
    fs::write(other.0.join("local-file"), "").unwrap();
    let out = command(&other.0.join("subdir"))
        .env("AXON_TEST_CONDITION", "inherited")
        .args(["tasks"])
        .output()
        .unwrap();
    assert!(success(out).contains(&id));
}

#[test]
fn condition_trace_and_failure_preserve_utf8_across_capture_boundary() {
    let f = Fixture::new();
    f.init();
    let id = f.accepted("utf8");
    let output = format!("{}日", "A".repeat(32767));
    fs::write(f.0.join("utf8-output"), &output).unwrap();
    for exit in [0, 23] {
        set_condition(
            &f,
            &id,
            &format!("cat utf8-output; cat utf8-output >&2; exit {exit}"),
        );
        let out = f.run(&["tasks", "--trace-conditions"]);
        assert_eq!(out.status.success(), exit == 0);
        let diagnostic = String::from_utf8(out.stderr).unwrap();
        assert_eq!(diagnostic.matches(&output).count(), 2);
        assert!(!diagnostic.contains('�'));
    }
}

fn without_recorder(command: &mut Command) -> &mut Command {
    for name in [
        "AXON_ACTOR",
        "AXON_SESSION_ID",
        "CODEX_THREAD_ID",
        "CODEX_SANDBOX",
        "CLAUDECODE",
        "CLAUDE_CODE",
        "CLAUDE_CODE_SESSION_ID",
        "AI_AGENT",
        "USER",
    ] {
        command.env_remove(name);
    }
    command
}

#[test]
fn recorder_is_automatic_durable_optional_and_not_an_operation_guard() {
    let f = Fixture::new();
    f.init();
    let created = success(
        without_recorder(&mut f.command())
            .env("CODEX_THREAD_ID", "original-session")
            .args(["capture", "--accept", "--title", "provenance"])
            .output()
            .unwrap(),
    );
    let id = created.split_whitespace().next().unwrap();
    success(
        without_recorder(&mut f.command())
            .args(["start", id])
            .output()
            .unwrap(),
    );
    success(
        without_recorder(&mut f.command())
            .env("CODEX_SANDBOX", "seatbelt")
            .args(["note", "add", id, "-m", "partial metadata"])
            .output()
            .unwrap(),
    );
    success(
        without_recorder(&mut f.command())
            .env("AXON_ACTOR", "another-worker")
            .args(["complete", id])
            .output()
            .unwrap(),
    );
    let normal = f.ok(&["log", id]);
    assert!(normal.contains("codex"));
    assert!(!normal.contains("original-session"));
    let details = success(
        without_recorder(&mut f.command())
            .env("CODEX_THREAD_ID", "current-session")
            .args(["log", id, "--recorder-details"])
            .output()
            .unwrap(),
    );
    assert!(details.contains(r#"data: {"session_id":"original-session"}"#));
    assert!(!details.contains("current-session"));
    assert!(details.contains("another-worker  data: {}"));
    let notes = f.ok(&["note", "list", id, "--recorder-details"]);
    assert!(notes.contains("codex  data: {}"));
    let records = f.records();
    let history = records.history(&eid(id)).unwrap();
    assert_eq!(history.len(), 3);
    assert!(history[1].1.recorder.is_none());
    assert_eq!(
        history[0].1.recorder.as_ref().unwrap().data["session_id"],
        "original-session"
    );
}

#[cfg(unix)]
#[test]
fn non_utf8_recorder_environment_does_not_block_writes() {
    use std::os::unix::ffi::OsStringExt;
    let f = Fixture::new();
    f.init();
    let invalid = std::ffi::OsString::from_vec(vec![0xff]);
    let created = success(
        without_recorder(&mut f.command())
            .env("AXON_ACTOR", &invalid)
            .env("CODEX_THREAD_ID", &invalid)
            .args(["capture", "--title", "unknown recorder"])
            .output()
            .unwrap(),
    );
    let id = created.split_whitespace().next().unwrap();
    success(
        without_recorder(&mut f.command())
            .env("AXON_ACTOR", "custom")
            .env("AXON_SESSION_ID", &invalid)
            .args(["note", "add", id, "-m", "still saved"])
            .output()
            .unwrap(),
    );
    success(
        without_recorder(&mut f.command())
            .env("CLAUDECODE", "1")
            .env("CLAUDE_CODE_SESSION_ID", invalid)
            .args(["note", "add", id, "-m", "saved from claude code"])
            .output()
            .unwrap(),
    );
    assert!(f.ok(&["log", id, "--recorder-details"]).contains("—"));
    let notes = f.ok(&["note", "list", id, "--recorder-details"]);
    assert!(notes.contains("custom  data: {}"));
    assert!(notes.contains("claude-code  data: {}"));
}

#[test]
fn claude_code_session_is_recorded_when_available() {
    let f = Fixture::new();
    f.init();
    let claude = |f: &Fixture| {
        let mut command = f.command();
        without_recorder(&mut command).env("CLAUDECODE", "1");
        command
    };
    let created = success(
        claude(&f)
            .env("CLAUDE_CODE_SESSION_ID", "claude-session")
            .args(["capture", "--accept", "--title", "claude provenance"])
            .output()
            .unwrap(),
    );
    let id = created.split_whitespace().next().unwrap();
    success(
        claude(&f)
            .env("CLAUDE_CODE_SESSION_ID", "claude-session")
            .args(["note", "add", id, "-m", "with session"])
            .output()
            .unwrap(),
    );
    success(claude(&f).args(["start", id]).output().unwrap());
    let normal = f.ok(&["log", id]);
    assert!(normal.contains("claude-code"));
    assert!(!normal.contains("claude-session"));
    let details = f.ok(&["log", id, "--recorder-details"]);
    assert_eq!(
        details
            .matches(r#"claude-code  data: {"session_id":"claude-session"}"#)
            .count(),
        1
    );
    assert_eq!(details.matches("claude-code  data: {}").count(), 1);
    assert!(
        f.ok(&["note", "list", id, "--recorder-details"])
            .contains(r#"claude-code  data: {"session_id":"claude-session"}"#)
    );
}

#[test]
fn recorder_details_preserve_unknown_json_and_escape_terminal_controls() {
    let f = Fixture::new();
    f.init();
    let id = f.accepted("unknown metadata");
    let mut store = f.store();
    store
        .update(|_, records, _| {
            let mut ctx = context();
            ctx.recorder = Some(Recorder {
                actor: "future\x1b[31m-agent".into(),
                data: BTreeMap::from([(
                    "nested".into(),
                    serde_json::json!({"number": 17, "array": [true, null], "text": "\x1b[31m"}),
                )]),
            });
            let note = records.add_note(&eid(&id), "unknown actor".into(), None, ctx)?;
            Ok((vec![Entry::Note(note)], ()))
        })
        .unwrap();
    let normal = f.ok(&["note", "list", &id]);
    assert!(!normal.contains('\x1b'));
    assert!(!normal.contains("nested"));
    let details = f.ok(&["note", "list", &id, "--recorder-details"]);
    assert!(!details.contains('\x1b'));
    assert!(details.contains(r#""array":[true,null]"#));
    assert!(details.contains(r#""number":17"#));
}

#[test]
fn recorder_help_is_available_without_a_store() {
    let f = Fixture::new();
    for args in [vec!["log", "--help"], vec!["note", "list", "--help"]] {
        assert!(f.ok(&args).contains("--recorder-details"));
    }
    assert!(!f.0.join(".axon").exists());
}

#[path = "lifecycle/file.rs"]
mod file_lifecycle;

#[path = "lifecycle/location.rs"]
mod location;

#[path = "lifecycle/workflow.rs"]
mod workflow;

#[path = "lifecycle/contracts.rs"]
mod contracts;

#[path = "lifecycle/declaration.rs"]
mod declaration;
