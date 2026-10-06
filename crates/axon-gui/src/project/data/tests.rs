use super::*;
use axon::lifecycle::{
    Context, Kind, Label, Lifecycle,
    record::{Current, Entry, new_entity_id},
};
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

fn no_fault(_: FaultPoint) -> io::Result<()> {
    Ok(())
}

fn fail_at(target: Step) -> impl FnMut(FaultPoint) -> io::Result<()> {
    fail_on(FaultPoint::Before(target))
}

fn fail_on(target: FaultPoint) -> impl FnMut(FaultPoint) -> io::Result<()> {
    move |point| {
        if point == target {
            Err(io::Error::other(format!("injected at {point:?}")))
        } else {
            Ok(())
        }
    }
}

fn counter(start: u64) -> impl FnMut() -> u64 {
    let mut next = start;
    move || {
        next += 1;
        next
    }
}

fn entities(connection: &ProjectConnection) -> usize {
    connection.read(|_, _, view| view.known().count()).unwrap()
}

/// Writes an Entity as the CLI would, through the core and not the window's connection.
fn create_entity(connection: &ProjectConnection, title: &str) {
    let mut store = Location::standalone(connection.root())
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

#[test]
fn projects_get_separate_stores_that_survive_a_restart() {
    let (_dir, data) = data();
    let registry = data.load_registry().unwrap();
    assert!(registry.projects().is_empty());
    data.create_project("読書会").unwrap();
    let registry = data.create_project(" 家計簿 ").unwrap();
    let [reading, budget] = registry.projects() else {
        panic!("two projects expected: {registry:?}");
    };
    assert_eq!(
        (reading.name.as_str(), budget.name.as_str()),
        ("読書会", "家計簿")
    );
    assert!([reading, budget].iter().all(|p| p.status == Status::Ready));
    assert_ne!(reading.id, budget.id);

    for project in [reading, budget] {
        let root = data.project_root(&project.id);
        assert!(
            root.is_absolute() && root.starts_with(data.dir()),
            "{root:?}"
        );
        assert!(root.join(".axon/header.json").is_file());
        assert!(root.join(".axon/records").is_dir());
        let prefix = data
            .connect(project)
            .read(|header, _, _| header.prefix.clone())
            .unwrap();
        assert_eq!(prefix, STORE_PREFIX);
    }

    let (a, b) = (data.connect(reading), data.connect(budget));
    create_entity(&a, "最初の本を選ぶ");
    assert_eq!((entities(&a), entities(&b)), (1, 0));

    // A new process sees the same list and the same roots.
    let restarted = AppData::at(data.dir().to_path_buf()).unwrap();
    let reloaded = restarted.load_registry().unwrap();
    assert_eq!(reloaded, registry);
    let reading = reloaded.get(&reading.id).unwrap();
    assert_eq!(restarted.connect(reading), a);
    assert_eq!(entities(&restarted.connect(reading)), 1);
}

#[test]
fn invalid_names_write_nothing() {
    let (_dir, data) = data();
    let registry = data.create_project("読書会").unwrap();
    for (name, expected) in [
        ("  ", NameError::Empty),
        ("読書会", NameError::Duplicate),
        ("a\tb", NameError::ControlCharacter),
    ] {
        match data.create_project(name) {
            Err(CreateError::Name(error)) => assert_eq!(error, expected),
            other => panic!("{name:?}: {other:?}"),
        }
    }
    assert_eq!(data.load_registry().unwrap(), registry);
    assert_eq!(
        fs::read_dir(data.dir().join(PROJECTS_DIRECTORY))
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn a_failure_before_registration_leaves_nothing_to_finish() {
    for step in [Step::Directory, Step::Register] {
        let (_dir, data) = data();
        let before = data.create_project("既存").unwrap();
        let error = data
            .create_project_with("読書会", &mut rand::random, &mut fail_at(step))
            .unwrap_err();
        assert!(
            matches!(&error, CreateError::Failed { step: failed, .. } if *failed == step),
            "{error:?}"
        );
        assert_eq!(data.load_registry().unwrap(), before);
        assert_eq!(
            fs::read_dir(data.dir().join(PROJECTS_DIRECTORY))
                .unwrap()
                .count(),
            1,
            "{step:?} left a directory behind"
        );
        // Retrying is creating anew.
        let after = data.create_project("読書会").unwrap();
        assert_eq!(after.projects().len(), 2);
    }
}

#[test]
fn an_interrupted_initialization_is_registered_and_can_be_finished() {
    let (_dir, data) = data();
    let error = data
        .create_project_with("読書会", &mut rand::random, &mut fail_at(Step::Initialize))
        .unwrap_err();
    assert!(matches!(
        error,
        CreateError::Failed {
            step: Step::Initialize,
            ..
        }
    ));
    let registry = data.load_registry().unwrap();
    let [project] = registry.projects() else {
        panic!("{registry:?}");
    };
    assert_eq!(project.status, Status::Creating);
    let root = data.project_root(&project.id);
    assert!(!root.join(".axon").exists());
    // Residue of an initialization that stopped half way does not block finishing.
    fs::create_dir_all(root.join(".axon/records")).unwrap();
    assert!(data.connect(project).read(|_, _, _| ()).is_err());

    let finished = data.finish_creation(&project.id).unwrap();
    assert_eq!(finished.get(&project.id).unwrap().status, Status::Ready);
    assert_eq!(data.load_registry().unwrap(), finished);
    assert_eq!(entities(&data.connect(project)), 0);
    // Finishing a ready project changes nothing.
    assert_eq!(data.finish_creation(&project.id).unwrap(), finished);
}

#[test]
fn finishing_keeps_a_store_that_already_exists() {
    let (_dir, data) = data();
    data.create_project_with("読書会", &mut rand::random, &mut fail_at(Step::Finish))
        .unwrap_err();
    let registry = data.load_registry().unwrap();
    let project = registry.projects()[0].clone();
    assert_eq!(project.status, Status::Creating);
    let connection = data.connect(&project);
    create_entity(&connection, "消えてはいけない記録");
    let header = fs::read(data.project_root(&project.id).join(".axon/header.json")).unwrap();

    let finished = data.finish_creation(&project.id).unwrap();
    assert_eq!(finished.get(&project.id).unwrap().status, Status::Ready);
    assert_eq!(entities(&connection), 1);
    assert_eq!(
        fs::read(data.project_root(&project.id).join(".axon/header.json")).unwrap(),
        header
    );
}

#[test]
fn finishing_refuses_foreign_files() {
    let (_dir, data) = data();
    data.create_project_with("読書会", &mut rand::random, &mut fail_at(Step::Initialize))
        .unwrap_err();
    let registry = data.load_registry().unwrap();
    let id = registry.projects()[0].id.clone();
    let foreign = data.project_root(&id).join(".axon/notes.txt");
    fs::create_dir_all(foreign.parent().unwrap()).unwrap();
    fs::write(&foreign, "手書きのメモ").unwrap();
    let error = data.finish_creation(&id).unwrap_err();
    assert!(matches!(
        error,
        CreateError::Failed {
            step: Step::Initialize,
            ..
        }
    ));
    assert_eq!(fs::read_to_string(&foreign).unwrap(), "手書きのメモ");
    assert_eq!(data.load_registry().unwrap(), registry);
}

#[test]
fn an_existing_directory_is_never_reused() {
    let (_dir, data) = data();
    let taken = ProjectId::from_bits(1);
    let leftover = data.project_root(&taken);
    fs::create_dir_all(&leftover).unwrap();
    fs::write(leftover.join("keep"), "残す").unwrap();
    let registry = data
        .create_project_with("読書会", &mut counter(0), &mut no_fault)
        .unwrap();
    assert_eq!(registry.projects()[0].id, ProjectId::from_bits(2));
    assert_eq!(fs::read_to_string(leftover.join("keep")).unwrap(), "残す");
}

#[test]
fn unreadable_data_is_an_error_not_an_empty_list() {
    let (_dir, data) = data();
    let registry = data.create_project("読書会").unwrap();
    let project = registry.projects()[0].clone();

    fs::remove_dir_all(data.project_root(&project.id)).unwrap();
    assert!(data.connect(&project).read(|_, _, _| ()).is_err());
    assert!(
        !data.project_root(&project.id).exists(),
        "a read never initializes"
    );

    let path = data.dir().join(REGISTRY_FILE);
    fs::write(&path, "{ broken").unwrap();
    assert!(matches!(
        data.load_registry(),
        Err(RegistryError::Decode(DecodeError::Invalid(_)))
    ));
    fs::write(&path, r#"{"format":9,"projects":[]}"#).unwrap();
    assert!(matches!(
        data.load_registry(),
        Err(RegistryError::Decode(DecodeError::Format(9)))
    ));
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(matches!(data.load_registry(), Err(RegistryError::Io(_))));
}

#[test]
fn a_registered_project_without_its_directory_can_be_finished() {
    let (_dir, data) = data();
    // What a registration whose replacement landed but did not become durable can leave.
    let id = ProjectId::from_bits(7);
    let registry = Registry::default()
        .with_creating(id.clone(), "読書会")
        .unwrap();
    fs::create_dir_all(data.dir()).unwrap();
    fs::write(data.dir().join(REGISTRY_FILE), registry.encode()).unwrap();
    assert!(!data.project_root(&id).exists());

    let finished = data.finish_creation(&id).unwrap();
    assert_eq!(finished.get(&id).unwrap().status, Status::Ready);
    assert_eq!(entities(&data.connect(finished.get(&id).unwrap())), 0);
}

#[test]
fn changes_are_made_to_the_registry_on_disk() {
    let (_dir, data) = data();
    data.create_project("読書会").unwrap();
    // Another view of the same data creates a project; a later creation keeps it.
    let other = AppData::at(data.dir().to_path_buf()).unwrap();
    other.create_project("家計簿").unwrap();
    let registry = data.create_project("旅行").unwrap();
    let names: Vec<_> = registry
        .projects()
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(names, ["読書会", "家計簿", "旅行"]);
    assert!(matches!(
        data.create_project("家計簿"),
        Err(CreateError::Name(NameError::Duplicate))
    ));
}

#[test]
fn a_data_directory_inside_a_repository_is_not_treated_as_git() {
    // An empty `.git` makes Git discovery fail; the application's own roots never consult it.
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join(".git")).unwrap();
    let data = AppData::at(dir.path().join("data")).unwrap();
    let registry = data.create_project("読書会").unwrap();
    let project = &registry.projects()[0];
    assert_eq!(entities(&data.connect(project)), 0);
    assert!(!dir.path().join(".git/axon-init.lock").exists());
}

#[test]
fn a_lost_registry_beside_existing_stores_is_not_a_first_start() {
    let (_dir, data) = data();
    // Directories without a store are what failed creations leave; they do not count.
    fs::create_dir_all(data.dir().join(PROJECTS_DIRECTORY).join("000000000001")).unwrap();
    assert_eq!(data.load_registry().unwrap(), Registry::default());

    data.create_project("読書会").unwrap();
    fs::remove_file(data.dir().join(REGISTRY_FILE)).unwrap();
    assert!(matches!(
        data.load_registry(),
        Err(RegistryError::Missing { .. })
    ));
    assert!(matches!(
        data.create_project("家計簿"),
        Err(CreateError::Failed {
            step: Step::Read,
            ..
        })
    ));
    assert!(
        !data.dir().join(REGISTRY_FILE).exists(),
        "nothing is written over it"
    );
}

#[cfg(unix)]
#[test]
fn a_store_that_cannot_be_looked_at_is_not_absent() {
    use std::os::unix::fs::PermissionsExt;
    let (_dir, data) = data();
    let registry = data.create_project("読書会").unwrap();
    fs::remove_file(data.dir().join(REGISTRY_FILE)).unwrap();
    let root = data.project_root(&registry.projects()[0].id);
    fs::set_permissions(&root, fs::Permissions::from_mode(0o000)).unwrap();
    let searchable = fs::symlink_metadata(root.join(".axon")).is_ok();
    let loaded = data.load_registry();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
    if searchable {
        return; // Permissions do not apply to this user (root); nothing to observe.
    }
    assert!(matches!(loaded, Err(RegistryError::Io(_))), "{loaded:?}");
}

#[test]
fn failures_after_registration_name_the_project() {
    for step in [Step::Initialize, Step::Finish] {
        let (_dir, data) = data();
        let error = data
            .create_project_with("読書会", &mut rand::random, &mut fail_at(step))
            .unwrap_err();
        let registry = data.load_registry().unwrap();
        assert_eq!(
            error.project(),
            Some(&registry.projects()[0].id),
            "{step:?}"
        );
    }
    let (_dir, data) = data();
    let error = data
        .create_project_with("読書会", &mut rand::random, &mut fail_at(Step::Register))
        .unwrap_err();
    assert_eq!(error.project(), None);
}

#[test]
fn a_registration_replaced_but_not_durable_can_be_finished() {
    let (_dir, data) = data();
    let error = data
        .create_project_with(
            "読書会",
            &mut rand::random,
            &mut fail_on(FaultPoint::Replaced(Step::Register)),
        )
        .unwrap_err();
    assert!(matches!(
        error,
        CreateError::Failed {
            step: Step::Register,
            ..
        }
    ));
    let registry = data.load_registry().unwrap();
    let project = registry.projects()[0].clone();
    assert_eq!(error.project(), Some(&project.id));
    assert_eq!(project.status, Status::Creating);
    assert!(data.project_root(&project.id).is_dir());
    let finished = data.finish_creation(&project.id).unwrap();
    assert_eq!(finished.get(&project.id).unwrap().status, Status::Ready);
}

#[test]
fn a_finish_replaced_but_not_durable_is_ready_and_names_the_project() {
    let (_dir, data) = data();
    let error = data
        .create_project_with(
            "読書会",
            &mut rand::random,
            &mut fail_on(FaultPoint::Replaced(Step::Finish)),
        )
        .unwrap_err();
    assert!(matches!(
        error,
        CreateError::Failed {
            step: Step::Finish,
            ..
        }
    ));
    let registry = data.load_registry().unwrap();
    assert_eq!(error.project(), Some(&registry.projects()[0].id));
    assert_eq!(registry.projects()[0].status, Status::Ready);
}

#[test]
fn a_store_with_parts_missing_is_not_empty() {
    let (_dir, data) = data();
    let registry = data.create_project("読書会").unwrap();
    let project = &registry.projects()[0];
    let root = data.project_root(&project.id);
    for (remove, missing) in [
        (root.join(".axon/records"), root.join(".axon/records")),
        (
            root.join(".axon/header.json"),
            root.join(".axon/header.json"),
        ),
        (root.clone(), root.clone()),
    ] {
        if remove.is_dir() {
            fs::remove_dir_all(&remove).unwrap();
        } else {
            fs::remove_file(&remove).unwrap();
        }
        let error = data.connect(project).read(|_, _, _| ()).unwrap_err();
        let text = error.to_string();
        assert!(
            text.contains("missing") && text.contains(&missing.display().to_string()),
            "{text}"
        );
        assert!(!text.contains("axon init"), "{text}");
    }
}

#[test]
fn the_data_directory_is_absolute() {
    assert_eq!(
        AppData::at("relative/data"),
        Err(LocateError::Relative("relative/data".into()))
    );
}

#[test]
fn a_second_instance_lock_is_refused_until_the_first_is_released() {
    let (_dir, data) = data();
    let first = data.lock_instance().unwrap();
    assert!(matches!(
        data.lock_instance(),
        Err(InstanceError::AlreadyRunning)
    ));
    drop(first);
    // A child process another test is starting may hold a copy of the descriptor until it
    // executes; the lock is free once it does.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        match data.lock_instance() {
            Ok(_) => break,
            Err(InstanceError::AlreadyRunning) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10))
            }
            Err(error) => panic!("{error}"),
        }
    }
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
