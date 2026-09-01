use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);

struct UninitializedDir(PathBuf);

impl UninitializedDir {
    fn new() -> Self {
        let sequence = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("axon-help-test-{}-{sequence}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for UninitializedDir {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn axon(args: &[&str]) -> (Output, UninitializedDir) {
    let dir = UninitializedDir::new();
    let output = Command::new(env!("CARGO_BIN_EXE_axon"))
        .args(args)
        .current_dir(&dir.0)
        .output()
        .unwrap();
    (output, dir)
}

#[cfg(unix)]
fn axon_with_closed_stdout(args: &[&str]) -> Output {
    use std::os::fd::{FromRawFd, OwnedFd};

    let dir = UninitializedDir::new();
    let mut pipe = [0; 2];
    assert_eq!(unsafe { libc::pipe(pipe.as_mut_ptr()) }, 0);
    assert_eq!(unsafe { libc::close(pipe[0]) }, 0);
    let writer = unsafe { OwnedFd::from_raw_fd(pipe[1]) };

    Command::new(env!("CARGO_BIN_EXE_axon"))
        .args(args)
        .current_dir(&dir.0)
        .stdout(Stdio::from(writer))
        .stderr(Stdio::piped())
        .output()
        .unwrap()
}

#[test]
fn complete_help_succeeds_without_opening_a_database() {
    let (output, dir) = axon(&["--help"]);

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("# CLI\n"));
    assert!(!dir.0.join(".axon").exists());
}

#[test]
fn help_all_is_identical_to_root_long_help() {
    let (root, _) = axon(&["--help"]);
    let (all, _) = axon(&["help", "all"]);

    assert_eq!(all.status.code(), Some(0));
    assert!(all.stderr.is_empty());
    assert_eq!(all.stdout, root.stdout);
}

#[test]
fn short_help_stays_short() {
    let (output, _) = axon(&["-h"]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    assert!(stdout.contains("Usage: axon <COMMAND>"));
    assert!(stdout.contains("Commands:"));
    assert!(!stdout.contains("# CLI"));
    assert!(!stdout.contains("# コマンドリファレンス"));
}

#[test]
fn nested_help_path_matches_the_subcommand_help_flag() {
    let (help_command, _) = axon(&["help", "group", "dep", "add"]);
    let (help_flag, _) = axon(&["group", "dep", "add", "--help"]);

    assert_eq!(help_command.status.code(), Some(0));
    assert!(help_command.stderr.is_empty());
    assert_eq!(help_command.stdout, help_flag.stdout);
}

#[test]
fn invalid_help_path_fails_on_stderr() {
    let (output, _) = axon(&["help", "group", "missing"]);

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("error:"));
}

#[cfg(unix)]
#[test]
fn complete_help_treats_closed_stdout_as_normal_termination() {
    for args in [["--help"].as_slice(), ["help", "all"].as_slice()] {
        let output = axon_with_closed_stdout(args);

        assert_eq!(output.status.code(), Some(0));
        assert!(output.stderr.is_empty());
    }
}
