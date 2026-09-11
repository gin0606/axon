use axon::{lifecycle::*, sqlite::Store};
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
    fn plan(&self, title: &str) -> String {
        self.ok(&["plan", "--title", title])
            .split_whitespace()
            .next()
            .unwrap()
            .into()
    }
    fn init(&self) {
        self.ok(&["init", "t"]);
    }
    fn db(&self) -> PathBuf {
        self.0.join(".axon/axon.db")
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
fn git(path: &Path, args: &[&str]) {
    let mut cmd = Command::new("git");
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("GIT_") {
            cmd.env_remove(name);
        }
    }
    let out = cmd
        .current_dir(path)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .args(args)
        .output()
        .unwrap();
    success(out);
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
        title: title.into(),
        description: String::new(),
        lifecycle: Lifecycle::NotStarted,
        condition: None,
        parent: None,
        dependencies: BTreeSet::new(),
    }
}

#[test]
fn registration_to_group_completion_and_records() {
    let f = Fixture::new();
    f.init();
    let group = f
        .ok(&["group", "plan", "--title", "計画", "-m", "計画本文"])
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    let dependency = f.plan("先行");
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
    assert!(f.ok(&["show", &issue]).contains("未判断"));
    f.ok(&["accept", &issue]);
    let wait = f.ok(&["show", &issue]);
    assert!(wait.contains("親の着手:"));
    assert!(wait.contains("依存先の完了:"));
    assert!(!wait.contains("所属:"));
    failure(f.run(&["start", &issue]));
    f.ok(&["start", &group]);
    f.ok(&["start", &dependency]);
    f.ok(&["done", &dependency]);
    f.ok(&[
        "write",
        &issue,
        "--title",
        "実装済みの目的",
        "-m",
        "編集本文",
    ]);
    f.ok(&["start", &issue]);
    f.ok(&["note", "add", &issue, "-m", "検証結果"]);
    failure(f.run(&["done", &group]));
    let show = f.ok(&["show", &issue]);
    assert!(show.starts_with(&issue));
    assert!(show.contains("1 notes"));
    assert!(show.contains("編集本文"));
    assert!(!show.contains("検証結果"));
    assert!(!show.contains("依存先の完了:"));
    f.ok(&["done", &issue, "--reason", "検証完了"]);
    assert!(
        f.ok(&["show", &group])
            .contains("1/1件終了（完了1・取りやめ0）")
    );
    assert!(f.ok(&["show", &group]).contains("最終確認待ち"));
    f.ok(&["done", &group]);
    assert!(f.ok(&["note", "list", &issue]).contains("検証結果"));
    let log = f.ok(&["log", &issue]);
    assert!(log.contains("InProgress → Completed"));
    assert!(log.contains("検証完了"));
    assert!(!log.contains("record-"));
    let list = f.ok(&["list"]);
    assert!(list.find(&group) < list.find(&dependency));
    assert!(list.find(&dependency) < list.find(&issue));
    let bytes = fs::read(f.db()).unwrap();
    failure(f.run(&["reconsider", &issue]));
    assert_eq!(bytes, fs::read(f.db()).unwrap());
}
#[test]
fn lifecycle_and_relation_edits_use_common_guards() {
    let f = Fixture::new();
    f.init();
    let a = f.plan("A");
    let b = f.plan("B");
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
        .ok(&["group", "capture", "--title", "G"])
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    f.ok(&["group", "set", &a, "--parent", &g]);
    assert!(f.ok(&["show", &g]).contains(&a));
    f.ok(&["group", "unset", &a]);
    f.ok(&["cancel", &g]);
    failure(f.run(&["group", "set", &a, "--parent", &g]));
}
#[test]
fn stdin_files_help_invalid_arguments_and_terminal_controls() {
    let f = Fixture::new();
    f.init();
    let body = f.0.join("body.txt");
    fs::write(&body, "long\n本文").unwrap();
    let id = f
        .ok(&[
            "plan",
            "--title",
            "safe\x1b[2J",
            "-F",
            body.to_str().unwrap(),
        ])
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    let show = f.ok(&["show", &id]);
    assert!(show.contains("safe\\x1b[2J"));
    assert!(!show.contains('\x1b'));
    assert!(show.contains("long\n本文"));
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
    assert!(f.ok(&["--help"]).contains("done"));
    assert!(f.ok(&["done", "--help"]).contains("最終確認"));
}
#[test]
fn concurrent_start_has_one_winner_and_other_writes_survive() {
    let f = Fixture::new();
    f.init();
    let id = f.plan("start");
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
                .args(["plan", "--title", &format!("entity {n}")])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    for child in children {
        success(child.wait_with_output().unwrap());
    }
    let (_, snapshot) = Store::open(&f.db()).unwrap().read().unwrap();
    assert_eq!(snapshot.notes(&eid(&id)).unwrap().len(), 12);
    assert_eq!(snapshot.entities().count(), 9);
    assert_eq!(snapshot.history(&eid(&id)).unwrap().len(), 2);
}
#[test]
fn sqlite_roundtrip_preserves_branches_integration_and_failed_changes() {
    let f = Fixture::new();
    let mut base = Snapshot::new(StoreId::generate());
    base.create(eid("item"), Kind::Issue, current("item"), context())
        .unwrap();
    base.perform(&eid("item"), Operation::Start, None, context())
        .unwrap();
    let mut left = base.clone();
    let mut right = base.clone();
    left.perform(&eid("item"), Operation::Complete, None, context())
        .unwrap();
    right
        .perform(&eid("item"), Operation::Release, None, context())
        .unwrap();
    left.add_note(&eid("item"), "same".into(), context())
        .unwrap();
    right
        .add_note(&eid("item"), "same".into(), context())
        .unwrap();
    let merged = left
        .integrate(
            &right,
            &BTreeMap::from([(eid("item"), Side::Right)]),
            Some("adopt unfinished".into()),
            context(),
        )
        .unwrap();
    fs::create_dir(f.0.join(".axon")).unwrap();
    let path = f.db();
    let mut store = Store::create(&path, "branch", &base).unwrap();
    store
        .update(|_, snapshot| {
            *snapshot = merged.clone();
            Ok(())
        })
        .unwrap();
    drop(store);
    let mut store = Store::open(&path).unwrap();
    assert_eq!(store.read().unwrap().1, merged);
    let log = f.ok(&["log", "item"]);
    assert!(log.contains("並行する分岐"));
    assert!(log.contains("統合: NotStarted を採用"));
    assert!(!log.contains("record-"));
    assert!(f.ok(&["note", "list", "item"]).contains("並行する分岐"));
    let failed: axon::sqlite::Result<()> = store.update(|_, snapshot| {
        snapshot.add_note(&eid("item"), "rollback".into(), context())?;
        snapshot.perform(&eid("item"), Operation::Complete, None, context())?;
        Ok(())
    });
    assert!(failed.is_err());
    assert_eq!(store.read().unwrap().1, merged);
    let barrier = Arc::new(Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let path = path.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut store = Store::open(&path).unwrap();
                barrier.wait();
                store
                    .update(|_, s| {
                        s.add_note(&eid("item"), "parallel".into(), context())?;
                        Ok(())
                    })
                    .unwrap();
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
    assert_eq!(
        store.read().unwrap().1.notes(&eid("item")).unwrap().len(),
        4
    );
}
#[test]
fn old_unknown_corrupt_and_mixed_stores_are_rejected_without_changes() {
    for kind in ["legacy", "unknown", "corrupt", "mixed", "pending"] {
        let f = Fixture::new();
        fs::create_dir(f.0.join(".axon")).unwrap();
        match kind {
            "legacy" => {
                let c = rusqlite::Connection::open(f.db()).unwrap();
                c.execute_batch("PRAGMA user_version=12; CREATE TABLE entities (id TEXT)")
                    .unwrap();
            }
            "unknown" => {
                drop(Store::create(&f.db(), "t", &axon::sqlite::empty()).unwrap());
                let c = rusqlite::Connection::open(f.db()).unwrap();
                c.pragma_update(None, "user_version", 999).unwrap();
            }
            "corrupt" => fs::write(f.db(), b"invalid database").unwrap(),
            "mixed" => {
                drop(Store::create(&f.db(), "t", &axon::sqlite::empty()).unwrap());
                fs::write(f.0.join(".axon/state.jsonl"), "invalid file").unwrap();
            }
            "pending" => {
                fs::write(f.0.join(".axon/init.pending"), "interrupted").unwrap();
                fs::write(f.db(), "").unwrap();
            }
            _ => unreachable!(),
        }
        let before = fs::read(f.db()).unwrap();
        failure(f.run(&["list"]));
        failure(f.run(&["init"]));
        assert_eq!(before, fs::read(f.db()).unwrap(), "{kind}");
    }
}
#[test]
fn discovery_outside_git_and_git_boundary() {
    let f = Fixture::new();
    f.init();
    let id = f.plan("outer");
    let nested = f.0.join("nested/deeper");
    fs::create_dir_all(nested.join(".axon")).unwrap();
    fs::write(nested.join(".axon/lock"), "").unwrap();
    assert!(success(command(&nested).args(["list"]).output().unwrap()).contains(&id));
    failure(command(&nested).args(["init"]).output().unwrap());
    fs::write(nested.join(".axon/init.pending"), "").unwrap();
    assert!(failure(command(&nested).args(["list"]).output().unwrap()).contains("incomplete"));
    fs::remove_file(nested.join(".axon/init.pending")).unwrap();
    git(&nested, &["init", "--quiet"]);
    assert!(failure(command(&nested).args(["list"]).output().unwrap()).contains("not initialized"));
    success(command(&nested).args(["init", "inner"]).output().unwrap());
    assert!(success(command(&nested).args(["list"]).output().unwrap()).is_empty());
    assert!(f.ok(&["list"]).contains(&id));
}
#[test]
fn linked_worktrees_share_sqlite_and_init_is_new_only() {
    let f = Fixture::new();
    let main = f.0.join("main");
    let linked = f.0.join("linked");
    fs::create_dir(&main).unwrap();
    git(&main, &["init", "--quiet"]);
    git(
        &main,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "core.hooksPath=/dev/null",
            "commit",
            "--allow-empty",
            "-m",
            "fixture",
        ],
    );
    git(
        &main,
        &[
            "-c",
            "core.hooksPath=/dev/null",
            "worktree",
            "add",
            "--quiet",
            "-b",
            "linked",
            linked.to_str().unwrap(),
        ],
    );
    success(command(&linked).args(["init", "shared"]).output().unwrap());
    assert!(main.join(".axon/axon.db").is_file());
    assert!(!linked.join(".axon/axon.db").exists());
    let id = success(
        command(&linked)
            .args(["plan", "--title", "shared"])
            .output()
            .unwrap(),
    )
    .split_whitespace()
    .next()
    .unwrap()
    .to_string();
    assert!(success(command(&main).args(["list"]).output().unwrap()).contains(&id));
    failure(command(&main).args(["init"]).output().unwrap());
    failure(command(&linked).args(["init"]).output().unwrap());
    assert!(!main.join(".gitattributes").exists());
    assert!(!main.join(".axon/.gitignore").exists());
    fs::create_dir_all(linked.join(".axon")).unwrap();
    fs::write(linked.join(".axon/state.jsonl"), "").unwrap();
    assert!(failure(command(&linked).args(["list"]).output().unwrap()).contains("mixed"));
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
    assert!(!f.0.join(".axon/init.pending").exists());
    Store::open(&f.db()).unwrap();
}
#[test]
fn output_failure_reports_applied_storage() {
    let f = Fixture::new();
    f.init();
    let mut child = f
        .command()
        .args(["plan", "--title", "retained"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    let out = child.wait_with_output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("storage applied; output failed"));
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
        success(command(path).args(["init"]).output().unwrap());
    }
    success(
        command(&spaced)
            .args(["plan", "--title", "only in spaced"])
            .output()
            .unwrap(),
    );
    assert!(success(command(&plain).args(["list"]).output().unwrap()).is_empty());
    assert!(success(command(&spaced).args(["list"]).output().unwrap()).contains("only in spaced"));
    assert_ne!(
        Store::open(&plain.join(".axon/axon.db"))
            .unwrap()
            .read()
            .unwrap()
            .1
            .store(),
        Store::open(&spaced.join(".axon/axon.db"))
            .unwrap()
            .read()
            .unwrap()
            .1
            .store()
    );
}

#[cfg(unix)]
#[test]
fn broken_git_marker_blocks_ancestor_storage_fallback() {
    let f = Fixture::new();
    f.init();
    f.plan("outer");
    let nested = f.0.join("nested");
    fs::create_dir(&nested).unwrap();
    std::os::unix::fs::symlink("missing-git-directory", nested.join(".git")).unwrap();
    let before = fs::read(f.db()).unwrap();
    for args in [
        vec!["list"],
        vec!["init"],
        vec!["plan", "--title", "must not reach outer"],
    ] {
        assert!(
            failure(command(&nested).args(args).output().unwrap()).contains("Git discovery failed")
        );
    }
    assert_eq!(before, fs::read(f.db()).unwrap());
}
#[cfg(unix)]
#[test]
fn initialization_path_never_emits_terminal_controls() {
    let f = Fixture::new();
    let path = f.0.join("repo\x1b[2J");
    fs::create_dir(&path).unwrap();
    let out = success(command(&path).args(["init"]).output().unwrap());
    assert!(!out.contains('\x1b'));
    assert!(out.contains("repo\\x1b[2J"));
    assert!(path.join(".axon/axon.db").is_file());
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
    let before = fs::read(f.db()).unwrap();
    for args in [
        vec!["list"],
        vec!["init"],
        vec!["plan", "--title", "wrong store"],
    ] {
        assert!(
            failure(command(&nested).args(args).output().unwrap()).contains("Git discovery failed")
        );
    }
    assert_eq!(before, fs::read(f.db()).unwrap());
}
#[test]
fn bare_repository_is_a_boundary_without_a_dot_git_entry() {
    let f = Fixture::new();
    f.init();
    let bare = f.0.join("bare.git");
    fs::create_dir(&bare).unwrap();
    git(&bare, &["init", "--bare", "--quiet"]);
    let before = fs::read(f.db()).unwrap();
    for args in [
        vec!["list"],
        vec!["init"],
        vec!["plan", "--title", "wrong store"],
    ] {
        assert!(
            failure(command(&bare).args(args).output().unwrap()).contains("Git discovery failed")
        );
    }
    assert_eq!(before, fs::read(f.db()).unwrap());
}

#[test]
fn unknown_trigger_with_sqlite_like_name_cannot_destroy_the_snapshot() {
    let f = Fixture::new();
    f.init();
    f.plan("retained");
    let connection = rusqlite::Connection::open(f.db()).unwrap();
    connection.execute_batch("CREATE TRIGGER sqliteX AFTER UPDATE ON lifecycle_store BEGIN DELETE FROM lifecycle_store; END").unwrap();
    drop(connection);
    let before = fs::read(f.db()).unwrap();
    assert!(failure(f.run(&["plan", "--title", "must fail"])).contains("unexpected SQLite schema"));
    assert_eq!(before, fs::read(f.db()).unwrap());
    let connection = rusqlite::Connection::open(f.db()).unwrap();
    let count: i64 = connection
        .query_row("SELECT count(*) FROM lifecycle_store", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);
}
#[test]
fn inherited_git_overrides_do_not_select_a_foreign_store() {
    let f = Fixture::new();
    let repo = f.0.join("repo");
    fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "--quiet"]);
    success(command(&repo).args(["init"]).output().unwrap());
    let before = fs::read(repo.join(".axon/axon.db")).unwrap();
    let outside = f.0.join("outside");
    fs::create_dir(&outside).unwrap();
    let out = command(&outside)
        .env("GIT_DIR", repo.join(".git"))
        .env("GIT_WORK_TREE", &repo)
        .env("GIT_COMMON_DIR", repo.join(".git"))
        .args(["plan", "--title", "wrong store"])
        .output()
        .unwrap();
    assert!(failure(out).contains("not initialized"));
    assert_eq!(before, fs::read(repo.join(".axon/axon.db")).unwrap());
    success(
        command(&outside)
            .env("GIT_DIR", repo.join(".git"))
            .args(["init"])
            .output()
            .unwrap(),
    );
    assert!(outside.join(".axon/axon.db").is_file());
    assert_eq!(before, fs::read(repo.join(".axon/axon.db")).unwrap());
}

fn set_when(f: &Fixture, id: &str, command: &str) {
    f.ok(&["when", "set", id, "--command", command]);
}
fn new_entity(f: &Fixture, args: &[&str]) -> String {
    f.ok(args).split_whitespace().next().unwrap().into()
}

#[test]
fn candidate_sets_and_lazy_ancestor_evaluation_are_shared_only_within_invocation() {
    let f = Fixture::new();
    f.init();
    let root = new_entity(&f, &["group", "plan", "--title", "root"]);
    let nested = new_entity(
        &f,
        &["group", "plan", "--title", "nested", "--parent", &root],
    );
    let dep = f.plan("dependency");
    let child = new_entity(
        &f,
        &[
            "plan", "--title", "child", "--parent", &nested, "--needs", &dep,
        ],
    );
    let draft = new_entity(&f, &["capture", "--title", "draft", "--parent", &nested]);
    set_when(&f, &root, "echo root >> observations; test -f open");
    set_when(&f, &nested, "echo nested >> observations");
    set_when(&f, &child, "echo child >> observations");
    set_when(&f, &draft, "echo draft >> observations");
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
            .contains("依存待ち")
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
    assert!(f.ok(&["triage"]).contains(&draft));
    assert_eq!(
        fs::read_to_string(f.0.join("observations")).unwrap(),
        "root\nnested\ndraft\n"
    );
    f.ok(&["start", &root]);
    f.ok(&["start", &nested]);
    fs::remove_file(f.0.join("open")).unwrap();
    let rows = f.ok(&["tasks"]);
    assert!(rows.contains(&root) && rows.contains(&nested));
    assert!(!rows.contains(&child));
    f.ok(&["cancel", &child]);
    set_when(&f, &root, "exit 23");
    fs::write(f.0.join("observations"), "").unwrap();
    let rows = f.ok(&["tasks"]);
    assert!(rows.contains(&root) && rows.contains(&nested));
    assert!(
        fs::read_to_string(f.0.join("observations"))
            .unwrap()
            .is_empty()
    );
    assert!(failure(f.run(&["triage"])).contains("exit status: 23"));
}

#[test]
fn conditions_preserve_saved_state_and_explicit_operations_never_evaluate() {
    let f = Fixture::new();
    f.init();
    let root = new_entity(&f, &["group", "plan", "--title", "root"]);
    let id = new_entity(&f, &["capture", "--title", "item", "--parent", &root]);
    let script = "echo executed >> forbidden; exit 23";
    set_when(&f, &root, script);
    set_when(&f, &id, script);
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
    f.ok(&["start", &root]);
    f.ok(&["start", &id]);
    f.ok(&["release", &id]);
    f.ok(&["cancel", &id]);
    set_when(&f, &id, script);
    f.ok(&["reconsider", &id]);
    f.ok(&["write", &id, "--title", "changed"]);
    f.ok(&["group", "unset", &id]);
    f.ok(&["group", "set", &id, "--parent", &root]);
    f.ok(&["accept", &id]);
    f.ok(&["start", &id]);
    f.ok(&["done", &id]);
    f.ok(&["done", &root]);
    set_when(&f, &id, "exit 2");
    f.ok(&["when", "clear", &id]);
    f.ok(&["note", "add", &id, "-m", "supplement"]);
    assert!(!f.0.join("forbidden").exists());
    let before = Store::open(&f.db()).unwrap().read().unwrap().1;
    assert!(f.ok(&["tasks"]).is_empty());
    assert!(f.ok(&["triage"]).is_empty());
    assert!(failure(f.run(&["when", "set", &id, "--command", " "])).contains("empty condition"));
    assert_eq!(Store::open(&f.db()).unwrap().read().unwrap().1, before);
}

#[test]
fn condition_results_diagnostics_and_repair_do_not_publish_partial_rows() {
    let f = Fixture::new();
    f.init();
    let ongoing = f.plan("ongoing");
    f.ok(&["start", &ongoing]);
    let id = f.plan("condition");
    set_when(
        &f,
        &id,
        "printf '\\033bad\\377'; printf problem >&2; exit 23",
    );
    let before = Store::open(&f.db()).unwrap().read().unwrap().1;
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
    assert_eq!(Store::open(&f.db()).unwrap().read().unwrap().1, before);
    set_when(&f, &id, "exit 1");
    let out = f.run(&["tasks", "--trace-conditions"]);
    assert!(!String::from_utf8(out.stdout).unwrap().contains(&id));
    let trace = String::from_utf8(out.stderr).unwrap();
    assert!(trace.contains("not satisfied (exit 1)"));
    assert_eq!(trace.matches("(empty)").count(), 2);
    set_when(&f, &id, "echo ignored; echo ignored >&2; exit 0");
    let out = f.run(&["tasks"]);
    assert!(out.status.success() && out.stderr.is_empty());
    assert!(!String::from_utf8(out.stdout).unwrap().contains("ignored"));
    set_when(&f, &id, "echo signal-detail >&2; kill -TERM $$");
    assert!(failure(f.run(&["tasks"])).contains("signal-detail"));
    f.ok(&["when", "clear", &id]);
    assert!(f.ok(&["tasks"]).contains(&id));
}

#[test]
fn condition_output_keeps_both_edges_per_stream_in_trace_and_failure() {
    let f = Fixture::new();
    f.init();
    let id = f.plan("output");
    let script = "awk 'BEGIN { for (i=0;i<40000;i++) printf \"A\"; for (i=0;i<40000;i++) printf \"B\" }'; awk 'BEGIN { for (i=0;i<40000;i++) printf \"C\"; for (i=0;i<40000;i++) printf \"D\" }' >&2";
    for exit in [0, 23] {
        set_when(&f, &id, &format!("{script}; exit {exit}"));
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
    let id = f.plan("interrupt");
    let script = "echo $$ > shell-pid; sh -c 'trap \"\" TERM; echo $$ > descendant-pid; while :; do :; done' </dev/null >/dev/null 2>/dev/null & wait";
    set_when(&f, &id, script);
    let before = Store::open(&f.db()).unwrap().read().unwrap().1;
    for interrupt in [false, true] {
        let mut cmd = f.command();
        cmd.args([
            "tasks",
            "--condition-timeout",
            if interrupt { "10" } else { "0.5" },
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
        assert_eq!(Store::open(&f.db()).unwrap().read().unwrap().1, before);
    }
}

#[test]
fn condition_default_timeout_is_thirty_seconds_in_a_real_process() {
    let f = Fixture::new();
    f.init();
    let id = f.plan("default timeout");
    set_when(&f, &id, "sleep 60");
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
    let id = f.plan("trace");
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
    set_when(&f, &id, "echo ran >> observed");
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
    let id = f.plan("cwd");
    set_when(
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
    set_when(&f, &id, script);
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
    let id = f.plan("utf8");
    let output = format!("{}日", "A".repeat(32767));
    fs::write(f.0.join("utf8-output"), &output).unwrap();
    for exit in [0, 23] {
        set_when(
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
            .args(["plan", "--title", "provenance"])
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
            .args(["done", id])
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
    let store = Store::open(&f.db()).unwrap();
    let (_, snapshot) = store.read().unwrap();
    let history = snapshot.history(&eid(id)).unwrap();
    assert_eq!(history.len(), 3);
    assert!(history[1].context.recorder.is_none());
    assert_eq!(
        history[0].context.recorder.as_ref().unwrap().data["session_id"],
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
            .env("AXON_SESSION_ID", invalid)
            .args(["note", "add", id, "-m", "still saved"])
            .output()
            .unwrap(),
    );
    assert!(f.ok(&["log", id, "--recorder-details"]).contains("—"));
    assert!(
        f.ok(&["note", "list", id, "--recorder-details"])
            .contains("custom  data: {}")
    );
}

#[test]
fn recorder_details_preserve_unknown_json_and_escape_terminal_controls() {
    let f = Fixture::new();
    f.init();
    let id = f.plan("unknown metadata");
    let mut store = Store::open(&f.db()).unwrap();
    store
        .update(|_, snapshot| {
            let mut ctx = context();
            ctx.recorder = Some(Recorder {
                actor: "future\x1b[31m-agent".into(),
                data: BTreeMap::from([(
                    "nested".into(),
                    serde_json::json!({"number": 17, "array": [true, null], "text": "\x1b[31m"}),
                )]),
            });
            snapshot.add_note(&eid(&id), "unknown actor".into(), ctx)?;
            Ok(())
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
    assert!(!f.db().exists());
}
