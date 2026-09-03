mod common;

use common::TestDir;
use std::process::{Output, Stdio};

fn axon(args: &[&str]) -> (Output, TestDir) {
    let dir = TestDir::new("help");
    let output = dir.axon_command().args(args).output().unwrap();
    (output, dir)
}

fn help_stdout(args: &[&str]) -> String {
    let (output, _) = axon(args);
    assert_eq!(output.status.code(), Some(0), "{args:?}");
    assert!(output.stderr.is_empty(), "{args:?}");
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn complete_help_is_self_contained_and_does_not_open_a_database() {
    let (output, dir) = axon(&["--help"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    for required in [
        "# axon CLI",
        "## State model",
        "Entity",
        "Progress",
        "Disposition",
        "Resurface condition",
        "## Basic workflow",
        "## Editing a plan declaration",
        "# Command reference",
        "## `axon group plan`",
        "## `axon group capture`",
        "## `axon import apply`",
    ] {
        assert!(stdout.contains(required), "missing {required:?}");
    }
    assert_eq!(stdout.matches("-m, --message <MESSAGE>").count(), 5);
    assert_eq!(stdout.matches("-F, --file <FILE>").count(), 5);
    assert!(!dir.path().join(".axon").exists());
}

#[test]
fn bare_help_matches_root_long_help_and_short_help_stays_short() {
    let (root, _) = axon(&["--help"]);
    let (help, _) = axon(&["help"]);
    assert_eq!(root.stdout, help.stdout);
    let short = help_stdout(&["-h"]);
    assert!(short.contains("Usage: axon <COMMAND>"));
    assert!(!short.contains("# axon CLI"));
}

#[test]
fn leaf_help_documents_common_entity_inputs_and_kind_filters() {
    for args in [
        &["plan", "--help"][..],
        &["capture", "--help"][..],
        &["group", "plan", "--help"][..],
        &["group", "capture", "--help"][..],
    ] {
        let help = help_stdout(args);
        assert!(help.contains("title"), "{help}");
        assert!(help.contains("--parent"), "{help}");
        assert!(help.contains("-m, --message <MESSAGE>"), "{help}");
        assert!(help.contains("-F, --file <FILE>"), "{help}");
    }
    for command in ["ready", "triage", "claims", "stale", "list"] {
        let help = help_stdout(&[command, "--help"]);
        assert!(help.contains("--kind <KIND>"), "{help}");
        assert!(help.contains("issue"), "{help}");
        assert!(help.contains("group"), "{help}");
    }
    let after = help_stdout(&["when", "after", "--help"]);
    assert!(after.contains("Entity whose terminal state"));
    let set = help_stdout(&["group", "set", "--help"]);
    assert!(set.contains("Parent group ID"));
}

#[test]
fn removed_slug_commands_are_absent_from_complete_help() {
    let help = help_stdout(&["--help"]);
    for path in [
        "axon group new",
        "axon group list",
        "axon group show",
        "axon group reject",
        "axon group dep",
    ] {
        assert!(!help.contains(&format!("## `{path}")), "{path}");
    }
}

#[test]
fn help_and_completion_do_not_open_a_database() {
    for args in [
        &["help"][..],
        &["-h"][..],
        &["group", "plan", "--help"][..],
        &["completion", "zsh"][..],
    ] {
        let (output, dir) = axon(args);
        assert!(output.status.success(), "{args:?}");
        assert!(output.stderr.is_empty(), "{args:?}");
        assert!(!dir.path().join(".axon").exists(), "{args:?}");
    }
}

#[cfg(unix)]
#[test]
fn generated_output_treats_closed_stdout_as_success() {
    use std::os::fd::{FromRawFd, OwnedFd};

    for args in [&["--help"][..], &["completion", "bash"][..]] {
        let dir = TestDir::new("closed-pipe");
        let mut pipe = [0; 2];
        assert_eq!(unsafe { libc::pipe(pipe.as_mut_ptr()) }, 0);
        assert_eq!(unsafe { libc::close(pipe[0]) }, 0);
        let writer = unsafe { OwnedFd::from_raw_fd(pipe[1]) };
        let output = dir
            .axon_command()
            .args(args)
            .stdout(Stdio::from(writer))
            .stderr(Stdio::piped())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(0));
        assert!(output.stderr.is_empty());
    }
}
