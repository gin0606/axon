use super::*;
use crate::project::ProjectConnection;
use axon::lifecycle::{
    Context, Kind, Label, Lifecycle,
    record::{Current, Entry, new_entity_id},
};
use axon::location::Location;
use std::{
    collections::BTreeSet,
    io::{BufRead, BufReader},
    process::{Command, Stdio},
    time::Duration,
};

fn data() -> (tempfile::TempDir, AppData) {
    let dir = tempfile::tempdir().unwrap();
    let data = AppData::at(dir.path().join("data")).unwrap();
    (dir, data)
}

/// A management root initialized as `axon init` would, at `dir/name`.
fn store(dir: &Path, name: &str) -> PathBuf {
    let root = dir.join(name);
    fs::create_dir_all(&root).unwrap();
    Location::explicit(&root).unwrap().init("axon").unwrap();
    root
}

/// Writes an Entity as the CLI would, through the core and not the window's connection.
fn create_entity(root: &Path, title: &str) {
    let mut store = Location::explicit(root)
        .and_then(|location| location.open())
        .unwrap();
    let outcome = store.update(|header, records, _| {
        let id = new_entity_id(&header.prefix)?;
        let record = records.create(
            id,
            Current {
                kind: Kind::Issue,
                lifecycle: Lifecycle::Undecided,
                owner: None,
                title: title.into(),
                description: String::new(),
                label: Label::Feat,
                condition: None,
                parent: None,
                needs: BTreeSet::new(),
            },
            Context {
                at: chrono::Utc::now(),
                recorder: None,
            },
        )?;
        Ok((vec![Entry::Record(record)], ()))
    });
    assert!(outcome.is_ok(), "{outcome:?}");
}

fn titles(root: &ProjectRoot) -> Vec<String> {
    ProjectConnection::new(root.clone())
        .read(|_, _, view| {
            let mut titles: Vec<_> = view
                .known()
                .filter_map(|id| view.current(id))
                .map(|current| current.title.clone())
                .collect();
            titles.sort();
            titles
        })
        .unwrap()
}

/// Every file and directory under `dir` with its length and modification time.
fn snapshot(dir: &Path) -> Vec<(PathBuf, u64, std::time::SystemTime)> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(path) = pending.pop() {
        let metadata = fs::symlink_metadata(&path).unwrap();
        if metadata.is_dir() {
            pending.extend(fs::read_dir(&path).unwrap().map(|e| e.unwrap().path()));
        }
        found.push((path, metadata.len(), metadata.modified().unwrap()));
    }
    found.sort();
    found
}

/// Takes the lock of `data` again after its holder in this process dropped it. A child process
/// another test is starting may hold a copy of the descriptor until it executes; the lock is
/// free once it does.
fn relock(data: &AppData) -> InstanceLock {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        match data.lock_instance() {
            Ok(lock) => return lock,
            Err(InstanceError::AlreadyRunning) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10))
            }
            Err(error) => panic!("{error}"),
        }
    }
}

#[test]
fn registered_roots_are_read_apart_and_survive_a_restart() {
    let (dir, data) = data();
    let reading = store(dir.path(), "読書会");
    let budget = store(dir.path(), "家計簿");
    create_entity(&reading, "最初の本を選ぶ");
    create_entity(&budget, "予算を決める");

    let lock = data.lock_instance().unwrap();
    assert!(data.load_registry().unwrap().roots().is_empty());
    let (_, first) = lock.register(&reading).unwrap();
    let (registry, second) = lock.register(&budget).unwrap();
    assert_eq!(registry.roots(), [first.clone(), second.clone()]);
    assert_eq!(first.path(), fs::canonicalize(&reading).unwrap());
    assert_eq!(
        (first.name(), second.name()),
        ("読書会".into(), "家計簿".into())
    );
    assert_eq!(titles(&first), ["最初の本を選ぶ"]);
    assert_eq!(titles(&second), ["予算を決める"]);

    // A new process sees the same list.
    drop(lock);
    let restarted = AppData::at(data.dir().to_path_buf()).unwrap();
    let _lock = relock(&restarted);
    assert_eq!(restarted.load_registry().unwrap(), registry);
}

#[test]
fn registration_needs_a_directory_with_a_header_and_writes_nothing_otherwise() {
    let (dir, data) = data();
    let lock = data.lock_instance().unwrap();
    let empty = dir.path().join("空");
    fs::create_dir(&empty).unwrap();
    let headless = dir.path().join("headless");
    fs::create_dir_all(headless.join(".axon/records")).unwrap();
    let file = dir.path().join("file.txt");
    fs::write(&file, "").unwrap();
    let stray = dir.path().join("stray");
    fs::create_dir(&stray).unwrap();
    fs::write(stray.join(".axon"), "").unwrap();

    for (path, refused) in [
        (&empty, "NotAStore"),
        (&headless, "NotAStore"),
        (&stray, "NotAStore"),
        (&file, "NotADirectory"),
        (&dir.path().join("missing"), "Unreachable"),
    ] {
        let error = lock.register(path).unwrap_err();
        assert!(
            format!("{error:?}").starts_with(refused),
            "{path:?}: {error:?}"
        );
    }
    assert!(matches!(
        lock.register(&empty),
        Err(UpdateError::NotAStore(header)) if header.ends_with(".axon/header.json")
    ));
    assert!(!data.dir().join(REGISTRY_FILE).exists());
}

#[test]
fn the_same_root_is_registered_once_however_it_is_named() {
    let (dir, data) = data();
    let root = store(dir.path(), "repo");
    let lock = data.lock_instance().unwrap();
    let (registry, _) = lock.register(&root).unwrap();
    let mut aliases = vec![root.join("."), root.join("../repo")];
    #[cfg(unix)]
    {
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&root, &link).unwrap();
        aliases.push(link);
    }
    for alias in aliases {
        assert!(
            matches!(lock.register(&alias), Err(UpdateError::Duplicate(_))),
            "{alias:?}"
        );
    }
    assert_eq!(data.load_registry().unwrap(), registry);
}

#[test]
fn unregistering_leaves_the_store_untouched() {
    let (dir, data) = data();
    let root = store(dir.path(), "repo");
    create_entity(&root, "残る記録");
    let other = store(dir.path(), "other");
    let lock = data.lock_instance().unwrap();
    let (_, registered) = lock.register(&root).unwrap();
    let (_, kept) = lock.register(&other).unwrap();
    let before = snapshot(&root);

    let registry = lock.unregister(&registered).unwrap();
    assert_eq!(registry.roots(), [kept]);
    assert_eq!(data.load_registry().unwrap(), registry);
    assert_eq!(snapshot(&root), before);
    assert!(matches!(
        lock.unregister(&registered),
        Err(UpdateError::Unknown(_))
    ));
    // It can be registered again.
    lock.register(&root).unwrap();
}

#[test]
fn an_unreadable_registry_is_an_error_and_is_not_overwritten() {
    let (dir, data) = data();
    let root = store(dir.path(), "repo");
    let lock = data.lock_instance().unwrap();
    let path = data.dir().join(REGISTRY_FILE);
    for (content, expected) in [
        ("{ broken", "Invalid"),
        (r#"{"format":9,"roots":[]}"#, "Format"),
    ] {
        fs::write(&path, content).unwrap();
        let error = data.load_registry().unwrap_err();
        assert!(
            format!("{error:?}").contains(expected),
            "{content}: {error:?}"
        );
        assert!(matches!(lock.register(&root), Err(UpdateError::Read(_))));
        let registered = ProjectRoot::new(fs::canonicalize(&root).unwrap()).unwrap();
        assert!(matches!(
            lock.unregister(&registered),
            Err(UpdateError::Read(_))
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), content);
    }
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(matches!(data.load_registry(), Err(RegistryError::Io(_))));
}

#[test]
fn the_files_of_earlier_builds_are_neither_read_nor_removed() {
    let (dir, data) = data();
    fs::create_dir_all(data.dir().join("projects/000000000001/.axon/records")).unwrap();
    let old = r#"{"format":1,"projects":[]}"#;
    fs::write(data.dir().join("projects.json"), old).unwrap();
    let before = snapshot(&data.dir().join("projects"));

    assert!(data.load_registry().unwrap().roots().is_empty());
    let lock = data.lock_instance().unwrap();
    lock.register(&store(dir.path(), "repo")).unwrap();
    assert_eq!(
        fs::read_to_string(data.dir().join("projects.json")).unwrap(),
        old
    );
    assert_eq!(snapshot(&data.dir().join("projects")), before);
}

/// Runs Git in `repository` without the user's configuration, feeding it `input`.
fn git(repository: &Path, args: &[&str], input: &str) -> String {
    let mut command = Command::new("git");
    // A Git hook running the tests sets variables that would point Git at another repository.
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("GIT_") {
            command.env_remove(name);
        }
    }
    let mut child = command
        .args(args)
        .current_dir(repository)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "git {args:?}");
    String::from_utf8(output.stdout).unwrap()
}

/// A Git repository whose subdirectory `sub` is a management root holding one Entity, added to
/// the index.
fn repository_with_a_store(dir: &Path) -> (PathBuf, PathBuf) {
    let repository = dir.join("repository");
    fs::create_dir(&repository).unwrap();
    git(&repository, &["init", "-q"], "");
    let root = repository.join("sub");
    fs::create_dir(&root).unwrap();
    Location::explicit(&root).unwrap().init("axon").unwrap();
    create_entity(&root, "Git の中の記録");
    git(&repository, &["add", "."], "");
    (repository, root)
}

/// Leaves `path` (relative to the repository) unmerged in the index, as a conflicting merge
/// does.
fn conflict(repository: &Path, path: &str) {
    let blob = git(repository, &["hash-object", "-w", "--stdin"], "");
    let blob = blob.trim();
    let zero = "0".repeat(40);
    git(
        repository,
        &["update-index", "--index-info"],
        &format!("0 {zero}\t{path}\n100644 {blob} 2\t{path}\n100644 {blob} 3\t{path}\n"),
    );
}

#[test]
fn a_root_inside_a_git_repository_is_read_without_changing_it() {
    let dir = tempfile::tempdir().unwrap();
    let (repository, root) = repository_with_a_store(dir.path());
    let data = AppData::at(dir.path().join("data")).unwrap();
    let lock = data.lock_instance().unwrap();
    let (_, registered) = lock.register(&root).unwrap();
    let store = root.join(".axon");
    let before = snapshot(&store);
    let index = fs::read(repository.join(".git/index")).unwrap();
    assert_eq!(titles(&registered), ["Git の中の記録"]);
    assert_eq!(snapshot(&store), before);
    assert_eq!(fs::read(repository.join(".git/index")).unwrap(), index);
}

#[test]
fn an_unmerged_store_is_registered_and_its_read_fails_on_the_merge() {
    let dir = tempfile::tempdir().unwrap();
    let (repository, root) = repository_with_a_store(dir.path());
    let data = AppData::at(dir.path().join("data")).unwrap();
    let lock = data.lock_instance().unwrap();
    // Only the header decides the registration; the merge is the read's error.
    conflict(&repository, "sub/.axon/.gitignore");
    let (_, registered) = lock.register(&root).unwrap();
    let read = || {
        ProjectConnection::new(registered.clone())
            .read(|_, _, _| ())
            .unwrap_err()
    };
    assert!(
        matches!(read(), axon::Error::Unmerged { .. }),
        "{:?}",
        read()
    );

    // A header the merge left out is explained by the merge, not reported as missing.
    conflict(&repository, "sub/.axon/header.json");
    fs::remove_file(root.join(".axon/header.json")).unwrap();
    assert!(
        matches!(read(), axon::Error::Unmerged { .. }),
        "{:?}",
        read()
    );
}

#[cfg(unix)]
#[test]
fn a_registry_that_cannot_be_written_stays_as_it_was() {
    use std::os::unix::fs::PermissionsExt;
    let (dir, data) = data();
    let lock = data.lock_instance().unwrap();
    let (registry, kept) = lock.register(&store(dir.path(), "kept")).unwrap();
    let added = store(dir.path(), "added");
    fs::set_permissions(data.dir(), fs::Permissions::from_mode(0o555)).unwrap();
    let writable = fs::File::create(data.dir().join("probe")).is_ok();
    let registered = lock.register(&added);
    let unregistered = lock.unregister(&kept);
    fs::set_permissions(data.dir(), fs::Permissions::from_mode(0o755)).unwrap();
    if writable {
        return; // Permissions do not apply to this user (root); nothing to observe.
    }
    assert!(
        matches!(registered, Err(UpdateError::Save(_))),
        "{registered:?}"
    );
    assert!(
        matches!(unregistered, Err(UpdateError::Save(_))),
        "{unregistered:?}"
    );
    assert_eq!(data.load_registry().unwrap(), registry);
}

#[test]
fn a_registered_store_with_parts_missing_is_not_empty() {
    let (dir, data) = data();
    let root = store(dir.path(), "repo");
    let lock = data.lock_instance().unwrap();
    let (_, registered) = lock.register(&root).unwrap();
    // Git keeps no empty directory, so a checkout of a store without records has none: the
    // store is empty, as the CLI reads it.
    fs::remove_dir_all(root.join(".axon/records")).unwrap();
    assert!(titles(&registered).is_empty());
    // `.axon` replaced by a file holds no header either.
    fs::remove_dir_all(root.join(".axon")).unwrap();
    fs::write(root.join(".axon"), "").unwrap();
    for removed in [root.join(".axon/header.json"), root.clone()] {
        if removed.is_dir() {
            fs::remove_dir_all(&removed).unwrap();
        } else if removed.exists() {
            fs::remove_file(&removed).unwrap();
        }
        let error = ProjectConnection::new(registered.clone())
            .read(|_, _, _| ())
            .unwrap_err();
        let text = error.to_string();
        assert!(
            text.contains("missing") && text.contains(&removed.display().to_string()),
            "{text}"
        );
        assert!(!text.contains("axon init"), "{text}");
    }
    assert!(!root.exists(), "a read never creates a store");
    fs::write(&root, "").unwrap();
    let error = ProjectConnection::new(registered.clone())
        .read(|_, _, _| ())
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("not a directory") && error.contains(&root.display().to_string()),
        "{error}"
    );
    fs::remove_file(&root).unwrap();
    assert_eq!(data.load_registry().unwrap().roots(), [registered]);
}

#[test]
fn the_data_directory_is_absolute() {
    assert_eq!(
        AppData::at("relative/data"),
        Err(LocateError::Relative("relative/data".into()))
    );
}

#[test]
fn only_the_holder_of_the_instance_lock_changes_the_registry() {
    let (dir, data) = data();
    let first = data.lock_instance().unwrap();
    // A second instance cannot take the lock, and without it there is no way to register.
    assert!(matches!(
        data.lock_instance(),
        Err(InstanceError::AlreadyRunning)
    ));
    let (registry, _) = first.register(&store(dir.path(), "repo")).unwrap();
    drop(first);
    let second = relock(&data);
    assert_eq!(second.data().load_registry().unwrap(), registry);
}

const LOCK_CHILD_DIR: &str = "AXON_GUI_TEST_LOCK_DIR";

/// Holds the instance lock of the directory in [`LOCK_CHILD_DIR`] until killed; does nothing
/// when run as an ordinary test.
#[test]
fn lock_holding_child() {
    let Some(dir) = std::env::var_os(LOCK_CHILD_DIR) else {
        return;
    };
    let _lock = AppData::at(PathBuf::from(dir))
        .unwrap()
        .lock_instance()
        .unwrap();
    println!("locked");
    std::thread::sleep(Duration::from_secs(60));
    panic!("the lock holder was not killed");
}

#[test]
fn a_killed_instance_does_not_block_the_next_start() {
    let (_dir, data) = data();
    let child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "project::data::tests::lock_holding_child",
            "--nocapture",
        ])
        .env(LOCK_CHILD_DIR, data.dir())
        .env("LLVM_PROFILE_FILE", "/dev/null")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    // Killed however the test ends, so a failed assertion does not leave it holding the lock.
    struct Killed(std::process::Child);
    impl Drop for Killed {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut child = Killed(child);
    let stdout = child.0.stdout.take().unwrap();
    let locked = BufReader::new(stdout)
        .lines()
        .map_while(Result::ok)
        .any(|line| line.ends_with("locked"));
    assert!(locked, "the child never took the lock");
    assert!(matches!(
        data.lock_instance(),
        Err(InstanceError::AlreadyRunning)
    ));
    child.0.kill().unwrap();
    assert!(!child.0.wait().unwrap().success());
    data.lock_instance().unwrap();
}
