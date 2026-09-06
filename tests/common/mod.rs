#![allow(dead_code)]

use rusqlite::{Connection, params};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

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
                Err(error) => panic!("failed to create {}: {error}", path.display()),
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
}

impl Drop for TestDir {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.path)
            .unwrap_or_else(|error| panic!("failed to remove {}: {error}", self.path.display()));
    }
}

pub struct TestRepo {
    dir: TestDir,
    root: PathBuf,
}

#[derive(Debug, Eq, PartialEq)]
pub struct EntitySnapshot {
    pub kind: String,
    pub title: String,
    pub description: Option<String>,
    pub progress: String,
    pub disposition: String,
    pub resurface_kind: Option<String>,
    pub resurface_date: Option<String>,
    pub resurface_ref: Option<String>,
    pub resurface_command: Option<String>,
    pub parent: Option<String>,
    pub updated_at: String,
    pub decision_events: i64,
    pub progress_events: i64,
}

impl TestRepo {
    pub fn new() -> Self {
        let dir = TestDir::new("cli");
        let root = dir.path.join("repo");
        fs::create_dir(&root).unwrap();
        dir.init_git(&root);
        Self { dir, root }
    }

    pub fn init(&self, prefix: &str) {
        assert_success(&self.axon(&["init", prefix]));
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn plan(&self, title: &str) -> String {
        self.create(&["plan", title])
    }

    pub fn capture(&self, title: &str) -> String {
        self.create(&["capture", title])
    }

    pub fn group_plan(&self, title: &str) -> String {
        self.create(&["group", "plan", title])
    }

    pub fn group_capture(&self, title: &str) -> String {
        self.create(&["group", "capture", title])
    }

    pub fn undecide(&self, id: &str) {
        assert_success(&self.axon(&["decide", "undecide", id, "-r", "test declaration edit"]));
    }

    pub fn accept(&self, id: &str) {
        assert_success(&self.axon(&["decide", "accept", id, "-r", "test declaration fixed"]));
    }

    pub fn set_parent(&self, id: &str, parent: &str) {
        self.undecide(id);
        assert_success(&self.axon(&["group", "set", id, parent]));
        self.accept(id);
    }

    pub fn add_dependency(&self, source: &str, target: &str) {
        self.undecide(source);
        assert_success(&self.axon(&["dep", "add", source, "--needs", target]));
        self.accept(source);
    }

    pub fn axon(&self, args: &[&str]) -> Output {
        self.axon_in(&self.root, args)
    }

    #[cfg(unix)]
    pub fn axon_in_timezone(&self, timezone: &str, args: &[&str]) -> Output {
        axon_command(&self.root, &self.dir.git_config)
            .env("TZ", timezone)
            .args(args)
            .output()
            .unwrap()
    }

    pub fn axon_in(&self, directory: &Path, args: &[&str]) -> Output {
        axon_command(directory, &self.dir.git_config)
            .args(args)
            .output()
            .unwrap()
    }

    pub fn axon_with_env(&self, args: &[&str], environment: &[(&str, Option<&str>)]) -> Output {
        let mut command = axon_command(&self.root, &self.dir.git_config);
        for (key, value) in environment {
            match value {
                Some(value) => command.env(key, value),
                None => command.env_remove(key),
            };
        }
        command.args(args).output().unwrap()
    }

    #[cfg(unix)]
    pub fn axon_with_closed_stdout(&self, args: &[&str]) -> Output {
        use std::os::fd::{FromRawFd, OwnedFd};

        let mut pipe = [0; 2];
        assert_eq!(unsafe { libc::pipe(pipe.as_mut_ptr()) }, 0);
        assert_eq!(unsafe { libc::close(pipe[0]) }, 0);
        let writer = unsafe { OwnedFd::from_raw_fd(pipe[1]) };
        axon_command(&self.root, &self.dir.git_config)
            .args(args)
            .stdout(Stdio::from(writer))
            .stderr(Stdio::piped())
            .output()
            .unwrap()
    }

    #[cfg(unix)]
    pub fn axon_with_closed_stderr(&self, args: &[&str]) -> Output {
        use std::os::fd::{FromRawFd, OwnedFd};

        let mut pipe = [0; 2];
        assert_eq!(unsafe { libc::pipe(pipe.as_mut_ptr()) }, 0);
        assert_eq!(unsafe { libc::close(pipe[0]) }, 0);
        let writer = unsafe { OwnedFd::from_raw_fd(pipe[1]) };
        axon_command(&self.root, &self.dir.git_config)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::from(writer))
            .output()
            .unwrap()
    }

    pub fn axon_with_stdin(&self, args: &[&str], input: &str) -> Output {
        let mut child = axon_command(&self.root, &self.dir.git_config)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }

    pub fn snapshot(&self, id: &str) -> EntitySnapshot {
        let connection = self.connection();
        let mut snapshot = connection
            .query_row(
                "SELECT kind,title,description,progress,disposition,
                 resurface_kind,resurface_date,resurface_ref,parent_id,updated_at,resurface_command
                 FROM entities WHERE id=?1",
                params![id],
                |row| {
                    Ok(EntitySnapshot {
                        kind: row.get(0)?,
                        title: row.get(1)?,
                        description: row.get(2)?,
                        progress: row.get(3)?,
                        disposition: row.get(4)?,
                        resurface_kind: row.get(5)?,
                        resurface_date: row.get(6)?,
                        resurface_ref: row.get(7)?,
                        parent: row.get(8)?,
                        updated_at: row.get(9)?,
                        resurface_command: row.get(10)?,
                        decision_events: 0,
                        progress_events: 0,
                    })
                },
            )
            .unwrap();
        snapshot.decision_events = connection
            .query_row(
                "SELECT COUNT(*) FROM entity_events WHERE entity_id=?1",
                params![id],
                |row| row.get(0),
            )
            .unwrap();
        snapshot.progress_events = connection
            .query_row(
                "SELECT COUNT(*) FROM entity_progress_events WHERE entity_id=?1",
                params![id],
                |row| row.get(0),
            )
            .unwrap();
        snapshot
    }

    pub fn dep_count(&self) -> i64 {
        self.connection()
            .query_row("SELECT COUNT(*) FROM entity_deps", [], |row| row.get(0))
            .unwrap()
    }

    pub fn note_id(&self, entity: &str, sequence: i64) -> String {
        self.record_id(entity, sequence, "entity_notes", "note", "record_id")
    }
    pub fn revision_id(&self, entity: &str, sequence: i64) -> String {
        self.record_id(
            entity,
            sequence,
            "declaration_revisions",
            "sequence",
            "revision",
        )
    }
    fn record_id(
        &self,
        entity: &str,
        sequence: i64,
        table: &str,
        order: &str,
        column: &str,
    ) -> String {
        self.connection().query_row(&format!("SELECT {column} FROM {table} WHERE (entity_id=?1 OR entity_id LIKE '%' || '-' || ?1) AND {order}=?2"), params![entity,sequence], |r| r.get(0)).unwrap()
    }

    pub fn current_revision(&self, id: &str) -> (i64, String, Option<String>, i64) {
        let connection = self.connection();
        let (revision, title, parent) = connection
            .query_row(
                "SELECT r.sequence,r.title,r.parent_id
                 FROM entities e JOIN declaration_revisions r
                   ON r.entity_id=e.id AND r.revision=e.current_revision
                 WHERE e.id=?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        let dependencies = connection
            .query_row(
                "SELECT COUNT(*) FROM revision_dependencies
                 WHERE entity_id=?1 AND revision=(SELECT revision FROM declaration_revisions WHERE entity_id=?1 AND sequence=?2)",
                params![id, revision],
                |row| row.get(0),
            )
            .unwrap();
        (revision, title, parent, dependencies)
    }

    pub fn entity_count(&self) -> i64 {
        self.connection()
            .query_row("SELECT COUNT(*) FROM entities", [], |row| row.get(0))
            .unwrap()
    }

    pub fn insert_dep_unchecked(&self, source: &str, target: &str) {
        self.connection()
            .execute(
                "INSERT INTO entity_deps VALUES (?1,?2)",
                params![source, target],
            )
            .unwrap();
    }

    pub fn execute_batch(&self, sql: &str) {
        self.connection().execute_batch(sql).unwrap();
    }

    pub fn add_worktree(&self) -> PathBuf {
        let commit = git_command(&self.root, &self.dir.git_config)
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
                "fixture",
            ])
            .output()
            .unwrap();
        assert_success(&commit);
        let path = self.dir.path.join("worktree");
        let output = git_command(&self.root, &self.dir.git_config)
            .args(["worktree", "add", "--quiet", "--detach"])
            .arg(&path)
            .output()
            .unwrap();
        assert_success(&output);
        path
    }

    fn create(&self, args: &[&str]) -> String {
        let output = self.axon(args);
        assert_success(&output);
        stdout(&output)
            .split_whitespace()
            .next()
            .expect("create command prints an ID")
            .to_string()
    }

    fn connection(&self) -> Connection {
        Connection::open(self.root.join(".axon/axon.db")).unwrap()
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
    command
        .current_dir(directory)
        .env("GIT_AUTHOR_NAME", "Axon Tests")
        .env("GIT_AUTHOR_EMAIL", "axon-tests@example.invalid")
        .env("GIT_COMMITTER_NAME", "Axon Tests")
        .env("GIT_COMMITTER_EMAIL", "axon-tests@example.invalid");
    isolate_git_environment(&mut command, git_config);
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
