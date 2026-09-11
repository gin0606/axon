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
