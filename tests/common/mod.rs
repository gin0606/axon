#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use rusqlite::{Connection, params};

static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);

pub struct TestDir {
    path: PathBuf,
    git_config: PathBuf,
}

impl TestDir {
    pub fn new(label: &str) -> Self {
        let temp_dir = fs::canonicalize(std::env::temp_dir()).unwrap();
        loop {
            let sequence = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
            let path = temp_dir.join(format!(
                "axon-{label}-test-{}-{sequence}",
                std::process::id()
            ));
            match fs::create_dir(&path) {
                Ok(()) => {
                    let git_config = path.join("gitconfig");
                    fs::write(&git_config, "").unwrap();
                    return Self { path, git_config };
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!(
                    "failed to create test directory {}: {error}",
                    path.display()
                ),
            }
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn axon_command(&self) -> Command {
        axon_command(&self.path, &self.git_config)
    }

    pub fn axon_command_in(&self, directory: &Path) -> Command {
        axon_command(directory, &self.git_config)
    }

    pub fn axon_in(&self, directory: &Path, args: &[&str]) -> Output {
        self.axon_command_in(directory).args(args).output().unwrap()
    }

    pub fn init_git(&self, directory: &Path) {
        let output = git_command(directory, &self.git_config)
            .args(["init", "--quiet"])
            .output()
            .unwrap();
        assert_success(&output);
    }

    fn git_config(&self) -> &Path {
        &self.git_config
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.path).unwrap_or_else(|error| {
            panic!(
                "failed to remove test directory {}: {error}",
                self.path.display()
            )
        });
    }
}

pub struct TestRepo {
    dir: TestDir,
    root: PathBuf,
}

#[derive(Debug, Eq, PartialEq)]
struct IssueState {
    title: String,
    description: Option<String>,
    progress: String,
    disposition: String,
    resurface_condition_kind: Option<String>,
    resurface_condition_date: Option<String>,
    resurface_condition_reference: Option<String>,
    group_id: Option<String>,
}

#[derive(Debug, Eq, PartialEq)]
pub struct IssueSnapshot {
    state: IssueState,
    updated_at: String,
    decision_events: i64,
    progress_events: i64,
}

impl TestRepo {
    pub fn new() -> Self {
        let dir = TestDir::new("cli");
        let root = dir.path().join("repo");
        fs::create_dir(&root).unwrap();
        let output = git_command(&root, dir.git_config())
            .args(["init", "--quiet"])
            .output()
            .unwrap();
        assert_success(&output);
        Self { dir, root }
    }

    pub fn init(&self, prefix: &str) {
        let output = self.axon(&["init", prefix]);
        assert_success(&output);
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn plan(&self, title: &str) -> String {
        self.create_issue("plan", title)
    }

    pub fn capture(&self, title: &str) -> String {
        self.create_issue("capture", title)
    }

    pub fn axon(&self, args: &[&str]) -> Output {
        self.axon_in(&self.root, args)
    }

    pub fn axon_in(&self, directory: &Path, args: &[&str]) -> Output {
        axon_command(directory, self.dir.git_config())
            .args(args)
            .output()
            .unwrap()
    }

    pub fn add_worktree(&self) -> PathBuf {
        let commit = git_command(&self.root, self.dir.git_config())
            .args([
                "-c",
                "user.name=Axon Tests",
                "-c",
                "user.email=axon-tests@example.invalid",
                "-c",
                "commit.gpgSign=false",
                "commit",
                "--allow-empty",
                "--no-verify",
                "--quiet",
                "-m",
                "test fixture",
            ])
            .output()
            .unwrap();
        assert_success(&commit);

        let path = self.dir.path().join("worktree");
        let output = git_command(&self.root, self.dir.git_config())
            .args(["worktree", "add", "--quiet", "--detach"])
            .arg(&path)
            .output()
            .unwrap();
        assert_success(&output);
        path
    }

    pub fn issue_snapshot(&self, id: &str) -> IssueSnapshot {
        let conn = self.connection();
        let (state, updated_at) = conn
            .query_row(
                "SELECT title, description, progress, disposition,
                        resurface_kind, resurface_date, resurface_ref, group_id, updated_at
                   FROM issues WHERE id = ?1",
                params![id],
                |row| {
                    Ok((
                        IssueState {
                            title: row.get(0)?,
                            description: row.get(1)?,
                            progress: row.get(2)?,
                            disposition: row.get(3)?,
                            resurface_condition_kind: row.get(4)?,
                            resurface_condition_date: row.get(5)?,
                            resurface_condition_reference: row.get(6)?,
                            group_id: row.get(7)?,
                        },
                        row.get(8)?,
                    ))
                },
            )
            .unwrap();
        let decision_events = conn
            .query_row(
                "SELECT COUNT(*) FROM events WHERE issue_id = ?1",
                params![id],
                |row| row.get(0),
            )
            .unwrap();
        let progress_events = conn
            .query_row(
                "SELECT COUNT(*) FROM progress_events WHERE issue_id = ?1",
                params![id],
                |row| row.get(0),
            )
            .unwrap();
        IssueSnapshot {
            state,
            updated_at,
            decision_events,
            progress_events,
        }
    }

    pub fn dep_count(&self, table: &str) -> i64 {
        assert!(matches!(table, "issue_deps" | "group_deps"));
        self.connection()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap()
    }

    pub fn insert_issue_dep_unchecked(&self, issue: &str, depends_on: &str) {
        self.connection()
            .execute(
                "INSERT INTO issue_deps (issue_id, depends_on_id) VALUES (?1, ?2)",
                params![issue, depends_on],
            )
            .unwrap();
    }

    pub fn set_after_issue_unchecked(&self, issue: &str, reference: &str) {
        self.connection()
            .execute(
                "UPDATE issues
                    SET resurface_kind = 'after_issue', resurface_date = NULL, resurface_ref = ?2
                  WHERE id = ?1",
                params![issue, reference],
            )
            .unwrap();
    }

    pub fn insert_group_dep_unchecked(&self, group: &str, depends_on: &str) {
        self.connection()
            .execute(
                "INSERT INTO group_deps (group_id, depends_on_id)
                 SELECT source.id, target.id
                   FROM groups source, groups target
                  WHERE source.slug = ?1 AND target.slug = ?2",
                params![group, depends_on],
            )
            .unwrap();
    }

    fn connection(&self) -> Connection {
        Connection::open(self.root.join(".axon/axon.db")).unwrap()
    }

    fn create_issue(&self, command: &str, title: &str) -> String {
        let output = self.axon(&[command, title]);
        assert_success(&output);
        stdout(&output)
            .split_whitespace()
            .next()
            .expect("create command should print an issue id")
            .to_string()
    }
}

pub fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "command failed with {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        stdout(output),
        stderr(output)
    );
}

pub fn assert_failure(output: &Output) {
    assert!(
        !output.status.success(),
        "command unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        stdout(output),
        stderr(output)
    );
}

pub fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

pub fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn axon_command(directory: &Path, git_config: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_axon"));
    command
        .current_dir(directory)
        .env("AXON_ACTOR", "test-actor")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env("CODEX_THREAD_ID", "test-session");
    isolate_git_environment(&mut command, git_config);
    command
}

fn git_command(directory: &Path, git_config: &Path) -> Command {
    let mut command = Command::new("git");
    command.current_dir(directory);
    isolate_git_environment(&mut command, git_config);
    command
        .env("GIT_AUTHOR_NAME", "Axon Tests")
        .env("GIT_AUTHOR_EMAIL", "axon-tests@example.invalid")
        .env("GIT_COMMITTER_NAME", "Axon Tests")
        .env("GIT_COMMITTER_EMAIL", "axon-tests@example.invalid");
    command
}

fn isolate_git_environment(command: &mut Command, git_config: &Path) {
    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_CONFIG_SYSTEM",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_PARAMETERS",
        "GIT_TEMPLATE_DIR",
        "GIT_AUTHOR_NAME",
        "GIT_AUTHOR_EMAIL",
        "GIT_AUTHOR_DATE",
        "GIT_COMMITTER_NAME",
        "GIT_COMMITTER_EMAIL",
        "GIT_COMMITTER_DATE",
        "EMAIL",
    ] {
        command.env_remove(name);
    }
    command
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", git_config);
}
