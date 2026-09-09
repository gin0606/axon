use crate::common::{TestDir, assert_success, stderr, stdout};
use std::fs;

#[test]
fn self_dependency_is_an_input_rejection_and_preserves_both_backends() {
    for backend in ["sqlite", "file"] {
        let dir = TestDir::new("self-dependency");
        let run = |args: &[&str]| dir.axon_in(dir.path(), args);
        assert_success(&run(&["init", "--backend", backend, "t"]));
        let created = run(&["capture", "task"]);
        assert_success(&created);
        let id = stdout(&created)
            .split_whitespace()
            .next()
            .unwrap()
            .to_string();
        assert_success(&run(&["note", "add", &id, "-m", "retained"]));
        let path = dir.path().join(if backend == "file" {
            ".axon/state.jsonl"
        } else {
            ".axon/axon.db"
        });
        let before = fs::read(&path).unwrap();
        let rejected = run(&["dep", "add", &id, "--needs", &id]);
        assert_eq!(rejected.status.code(), Some(1));
        assert!(rejected.stdout.is_empty());
        assert_eq!(
            stderr(&rejected),
            format!("Error: {id} dep add: Entity {id} cannot depend on itself\n")
        );
        assert_eq!(fs::read(&path).unwrap(), before);

        let exported = run(&["export", &id]);
        assert_success(&exported);
        let declaration = dir.path().join("self.yml");
        let invalid = stdout(&exported).replacen("    dependencies: []", &format!("    dependencies:\n      - dependent: {{ id: {id} }}\n        prerequisite: {{ id: {id} }}"), 1);
        fs::write(&declaration, &invalid).unwrap();
        for operation in ["prepare", "apply"] {
            let output = run(&["import", operation, declaration.to_str().unwrap()]);
            assert_eq!(output.status.code(), Some(1));
            assert!(
                stderr(&output).contains(&format!("Entity {id} cannot depend on itself")),
                "{}",
                stderr(&output)
            );
            assert_eq!(fs::read(&path).unwrap(), before);
            assert_eq!(fs::read_to_string(&declaration).unwrap(), invalid);
        }
    }
}

#[test]
fn input_file_errors_identify_operation_and_file() {
    let dir = TestDir::new("mutation-input");
    assert_success(&dir.axon_in(dir.path(), &["init", "t"]));
    let created = dir.axon_in(dir.path(), &["capture", "task"]);
    assert_success(&created);
    let id = stdout(&created)
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    let missing = dir.path().join("missing.md");
    let output = dir.axon_in(
        dir.path(),
        &["note", "add", &id, "-F", missing.to_str().unwrap()],
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains(&format!("{id} note add:")));
    assert!(stderr(&output).contains("missing.md read input:"));
}

#[cfg(unix)]
#[test]
fn file_append_io_failure_names_storage_phase_and_keeps_all_saved_bytes() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TestDir::new("file-append-fault");
    let run = |args: &[&str]| dir.axon_in(dir.path(), args);
    assert_success(&run(&["init", "--backend", "file", "t"]));
    let created = run(&["capture", "task"]);
    assert_success(&created);
    let id = stdout(&created)
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    let state = dir.path().join(".axon/state.jsonl");
    let before = fs::read(&state).unwrap();
    let directory = state.parent().unwrap();
    let permissions = fs::metadata(directory).unwrap().permissions();
    fs::set_permissions(directory, fs::Permissions::from_mode(0o555)).unwrap();
    let result = run(&["note", "add", &id, "-m", "not saved"]);
    fs::set_permissions(directory, permissions).unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(result.stdout.is_empty());
    assert!(stderr(&result).contains(&format!("{id} note add:")));
    assert!(stderr(&result).contains(&format!("{} before replace:", state.display())));
    assert!(stderr(&result).contains("Not applied: storage update"));
    assert!(!stderr(&result).contains("Result unknown"));
    assert_eq!(fs::read(&state).unwrap(), before);
}

#[cfg(unix)]
#[test]
fn sqlite_import_readonly_failure_preserves_storage_and_declaration() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TestDir::new("sqlite-import-readonly");
    let run = |args: &[&str]| dir.axon_in(dir.path(), args);
    assert_success(&run(&["init", "t"]));
    let created = run(&["capture", "original"]);
    assert_success(&created);
    let id = stdout(&created)
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    let exported = run(&["export", &id]);
    assert_success(&exported);
    let path = dir.path().join("plan.yml");
    fs::write(
        &path,
        stdout(&exported).replace("title: original", "title: edited"),
    )
    .unwrap();
    assert_success(&run(&["import", "prepare", path.to_str().unwrap()]));
    let state = dir.path().join(".axon/axon.db");
    let before = fs::read(&state).unwrap();
    let declaration = fs::read(&path).unwrap();
    let permissions = fs::metadata(&state).unwrap().permissions();
    fs::set_permissions(&state, fs::Permissions::from_mode(0o444)).unwrap();
    let result = run(&["import", "apply", path.to_str().unwrap()]);
    fs::set_permissions(&state, permissions).unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(result.stdout.is_empty());
    assert!(stderr(&result).contains(&format!("{} import apply:", path.display())));
    assert!(stderr(&result).contains(state.to_str().unwrap()));
    assert!(
        stderr(&result).contains("Not applied:"),
        "{}",
        stderr(&result)
    );
    assert_eq!(fs::read(&state).unwrap(), before);
    assert_eq!(fs::read(&path).unwrap(), declaration);
}

#[cfg(unix)]
#[test]
fn init_size_limit_failure_reports_retained_marker_and_unpublished_state() {
    use std::os::unix::process::CommandExt;
    for backend in ["sqlite", "file"] {
        let dir = TestDir::new("init-size-limit");
        let root =
            std::path::Path::new("/tmp").join(format!("axil-{:016x}", rand::random::<u64>()));
        fs::create_dir(&root).unwrap();
        let mut command = dir.axon_command_in(&root);
        command.args(["init", "--backend", backend, "t"]);
        // RLIMIT_FSIZE must constrain only Axon's files, not the coverage runtime's profile.
        if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
            command.env("LLVM_PROFILE_FILE", "/dev/null");
        }
        let size_limit = if backend == "sqlite" { 1024 } else { 100 };
        unsafe {
            command.pre_exec(move || {
                libc::signal(libc::SIGXFSZ, libc::SIG_IGN);
                let limit = libc::rlimit {
                    rlim_cur: size_limit,
                    rlim_max: size_limit,
                };
                if libc::setrlimit(libc::RLIMIT_FSIZE, &limit) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let output = command.output().unwrap();
        let marker = root.join(".axon/init.pending").is_file();
        let state = root.join(".axon/axon.db").exists() || root.join(".axon/state.jsonl").exists();
        let config = root.join(".axon/config.json").exists();
        let temporary = fs::read_dir(root.join(".axon")).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp")
        });
        fs::remove_dir_all(&root).unwrap();
        assert_eq!(output.status.code(), Some(1));
        let diagnostic = stderr(&output);
        if backend == "sqlite" {
            assert!(
                diagnostic.contains("initialize storage: SQLite:"),
                "{diagnostic}"
            );
        }
        assert!(diagnostic.contains("Applied: initialization marker at"));
        assert!(diagnostic.contains("Result unknown: storage update at"));
        assert!(diagnostic.contains("Help: Preserve the state and init.pending"));
        assert!(!diagnostic.contains("axon note"));
        assert!(marker);
        assert!(!state);
        assert!(!config);
        assert!(temporary);
    }
}
