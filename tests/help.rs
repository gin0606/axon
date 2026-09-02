use std::process::{Output, Stdio};

mod common;
use common::TestDir;

fn axon(args: &[&str]) -> (Output, TestDir) {
    let dir = TestDir::new("help");
    let output = dir.axon_command().args(args).output().unwrap();
    (output, dir)
}

#[cfg(unix)]
fn axon_with_closed_stdout(args: &[&str]) -> Output {
    use std::os::fd::{FromRawFd, OwnedFd};

    let dir = TestDir::new("help-pipe");
    let mut pipe = [0; 2];
    assert_eq!(unsafe { libc::pipe(pipe.as_mut_ptr()) }, 0);
    assert_eq!(unsafe { libc::close(pipe[0]) }, 0);
    let writer = unsafe { OwnedFd::from_raw_fd(pipe[1]) };

    dir.axon_command()
        .args(args)
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
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("# axon CLI\n"));
    assert!(!dir.path().join(".axon").exists());
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
    assert!(!stdout.contains("# axon CLI"));
    assert!(!stdout.contains("# Command reference"));
}

#[test]
fn complete_help_is_an_english_self_contained_manual() {
    let (output, _) = axon(&["--help"]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    for required in [
        "## State model",
        "Progress",
        "Disposition",
        "Resurface condition",
        "## Basic workflow",
        "## Choosing a query",
        "## Safety and concurrency",
        "## Input, output, and exit status",
        "# Command reference",
    ] {
        assert!(stdout.contains(required), "missing {required:?} from help");
    }
    assert!(
        !stdout.chars().any(|c| matches!(
            c,
            '\u{3040}'..='\u{30ff}' | '\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}'
        )),
        "complete help contains Japanese text"
    );
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
fn reason_options_match_the_operation_contract() {
    let (done, _) = axon(&["done", "--help"]);
    let (release, _) = axon(&["release", "--help"]);
    let (group_reject, _) = axon(&["group", "reject", "--help"]);

    for output in [&done, &release, &group_reject] {
        assert_eq!(output.status.code(), Some(0));
        assert!(output.stderr.is_empty());
    }
    assert!(!String::from_utf8_lossy(&done.stdout).contains("--reason"));
    assert!(String::from_utf8_lossy(&release.stdout).contains("--reason"));
    assert!(String::from_utf8_lossy(&group_reject.stdout).contains("--reason"));
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

#[test]
fn completion_scripts_are_generated_without_opening_a_database() {
    for shell in ["bash", "elvish", "fish", "powershell", "zsh"] {
        let (output, dir) = axon(&["completion", shell]);

        assert_eq!(output.status.code(), Some(0), "{shell}");
        assert!(output.stderr.is_empty(), "{shell}");
        assert!(!output.stdout.is_empty(), "{shell}");
        assert!(!dir.path().join(".axon").exists(), "{shell}");
    }
}

#[test]
fn completion_rejects_unsupported_shells() {
    let (output, _) = axon(&["completion", "nushell"]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    for shell in ["bash", "elvish", "fish", "powershell", "zsh"] {
        assert!(stderr.contains(shell), "missing {shell}:\n{stderr}");
    }
}

#[cfg(unix)]
#[test]
fn completion_treats_closed_stdout_as_normal_termination() {
    let output = axon_with_closed_stdout(&["completion", "bash"]);

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
}
