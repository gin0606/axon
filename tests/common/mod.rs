#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
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
