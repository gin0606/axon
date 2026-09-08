use crate::common::{TestDir, TestRepo, assert_success, stderr, stdout};
use std::{collections::BTreeMap, env, fs, path::Path, process::Command};

fn command(directory: &Path) -> Command {
    let coverage_profile = env::var_os("LLVM_PROFILE_FILE");
    let mut command = Command::new(env!("CARGO_BIN_EXE_axon"));
    command.current_dir(directory).env_clear();
    // Keep coverage output outside the isolated directory whose contents this test asserts.
    if let Some(profile) = coverage_profile {
        command.env("LLVM_PROFILE_FILE", profile);
    }
    command
}

fn assert_actor(command: &mut Command, expected: &str) {
    let output = command.arg("actor").output().unwrap();
    assert_success(&output);
    assert_eq!(stdout(&output), format!("{expected}\n"));
    assert!(stderr(&output).is_empty());
}

fn files(root: &Path) -> BTreeMap<std::path::PathBuf, Vec<u8>> {
    let mut result = BTreeMap::new();
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            result.insert(path.clone(), Vec::new());
            result.extend(files(&path));
        } else {
            result.insert(path.clone(), fs::read(path).unwrap());
        }
    }
    result
}

#[test]
fn actor_preserves_detection_priority_in_an_isolated_environment() {
    let dir = TestDir::new("actor");
    let cases: &[(&[(&str, &str)], &str)] = &[
        (
            &[
                ("AXON_ACTOR", " explicit "),
                ("CLAUDECODE", "1"),
                ("CODEX_SANDBOX", "1"),
                ("AI_AGENT", "other"),
            ],
            " explicit ",
        ),
        (
            &[
                ("AXON_ACTOR", ""),
                ("CLAUDECODE", ""),
                ("CODEX_THREAD_ID", "1"),
                ("AI_AGENT", "other"),
            ],
            "claude-code",
        ),
        (&[("CLAUDE_CODE", "1")], "claude-code"),
        (&[("CODEX_SANDBOX", ""), ("AI_AGENT", "other")], "codex"),
        (&[("CODEX_THREAD_ID", "1")], "codex"),
        (&[("AXON_ACTOR", ""), ("AI_AGENT", "other")], "other"),
    ];
    for (environment, expected) in cases {
        assert_actor(
            command(dir.path()).envs(environment.iter().copied()),
            expected,
        );
    }
    for name in ["axon", "another-directory"] {
        let path = dir.path().join(name);
        fs::create_dir(&path).unwrap();
        assert_actor(
            command(&path).env("USER", "gin0606").env("AI_AGENT", ""),
            &format!("gin0606@{name}"),
        );
        assert_actor(&mut command(&path), &format!("unknown@{name}"));
        assert_actor(command(&path).env("USER", ""), &format!("@{name}"));
    }
    assert_actor(command(Path::new("/")).env("USER", "gin0606"), "gin0606");
    assert_actor(&mut command(Path::new("/")), "unknown");
}

#[test]
fn actor_does_not_discover_or_open_storage_or_create_files() {
    let dir = TestDir::new("actor-no-db");
    let before = files(dir.path());
    assert_actor(
        command(dir.path()).env("AXON_ACTOR", "observed"),
        "observed",
    );
    assert_eq!(files(dir.path()), before);

    let repo = TestRepo::new();
    repo.init("test");
    fs::write(repo.root().join(".axon/axon.db"), b"unreadable database").unwrap();
    let before = files(repo.root());
    assert_actor(
        command(repo.root()).env("AXON_ACTOR", "observed"),
        "observed",
    );
    assert_eq!(files(repo.root()), before);
    assert!(!repo.axon(&["list"]).status.success());
}

#[test]
fn actor_matches_the_first_note_without_recording_or_evaluating_conditions() {
    let repo = TestRepo::new();
    repo.init("test");
    let id = repo.plan("actor observation");
    assert_success(&repo.axon(&["when", "command", &id, "touch actor-command-ran; exit 0"]));
    let before = files(repo.root());
    let output = repo.axon(&["actor"]);
    assert_success(&output);
    assert_eq!(stdout(&output), "test-actor\n");
    assert_eq!(files(repo.root()), before);
    assert!(!repo.root().join("actor-command-ran").exists());
    assert_success(&repo.axon(&["note", "add", &id, "-m", "first note"]));
    let notes = repo.axon(&["note", "list", &id]);
    assert_success(&notes);
    assert!(stdout(&notes).contains(stdout(&output).trim()));
    let connection = rusqlite::Connection::open(repo.root().join(".axon/axon.db")).unwrap();
    let (count, actor): (i64, String) = connection
        .query_row("SELECT count(*), actor FROM entity_notes", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .unwrap();
    assert_eq!(count, 1);
    assert_eq!(actor, stdout(&output).trim_end());
}

#[test]
fn actor_help_is_available_without_a_database_and_rejects_arguments() {
    let dir = TestDir::new("actor-help");
    let output = command(dir.path())
        .args(["actor", "--help"])
        .output()
        .unwrap();
    assert_success(&output);
    let help = stdout(&output);
    assert!(help.contains("without opening a database"));
    assert!(help.contains("not a unique session ID"));
    assert!(
        !command(dir.path())
            .args(["actor", "extra"])
            .output()
            .unwrap()
            .status
            .success()
    );
}
