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
fn root_help_forms_match_and_stay_concise() {
    let root = help_stdout(&[]);
    for args in [
        &["help"][..],
        &["-h"][..],
        &["--help"][..],
        &["-h", "plan"][..],
        &["--help", "plan"][..],
    ] {
        assert_eq!(root, help_stdout(args), "{args:?}");
    }
    assert!(root.contains("Usage: axon <COMMAND>"));
    assert!(root.contains("axon docs"));
    assert!(!root.contains("help all"));
}

#[test]
fn root_help_groups_commands_in_workflow_order() {
    let help = help_stdout(&["-h"]);
    let sections = [
        (
            "Workflow:",
            [
                "plan", "capture", "ready", "triage", "start", "done", "release",
            ]
            .as_slice(),
        ),
        (
            "Inspect:",
            [
                "status", "show", "list", "claims", "log", "note", "revision", "actor",
            ]
            .as_slice(),
        ),
        (
            "Plan management:",
            [
                "write", "group", "dep", "decide", "when", "export", "import",
            ]
            .as_slice(),
        ),
        (
            "Setup & utilities:",
            ["init", "completion", "docs", "help"].as_slice(),
        ),
    ];

    let mut previous_section = 0;
    for (index, (heading, commands)) in sections.iter().enumerate() {
        let section = help
            .find(*heading)
            .unwrap_or_else(|| panic!("missing section {heading:?}"));
        assert!(section >= previous_section, "{heading}");
        previous_section = section;
        let section_end = sections
            .get(index + 1)
            .and_then(|(next_heading, _)| help.find(next_heading))
            .unwrap_or(help.len());
        let section_help = &help[section..section_end];

        let mut previous_command = 0;
        for command in *commands {
            let command = section_help
                .find(&format!("\n  {command}"))
                .unwrap_or_else(|| panic!("missing command {command:?}"));
            assert!(command >= previous_command, "{heading}: {command}");
            previous_command = command;
        }
    }
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
    for command in ["ready", "triage", "claims", "list"] {
        let help = help_stdout(&[command, "--help"]);
        assert!(help.contains("--kind <KIND>"), "{help}");
        assert!(help.contains("issue"), "{help}");
        assert!(help.contains("group"), "{help}");
    }
    let after = help_stdout(&["when", "after", "--help"]);
    assert!(after.contains("Entity whose terminal state"));
    let set = help_stdout(&["group", "set", "--help"]);
    assert!(set.contains("Parent group ID"));
    let triage = help_stdout(&["triage", "--help"]);
    for required in [
        "all four conditions: non-terminal",
        "own Resurface condition is satisfied (surfaced)",
        "active scope (all ancestor Group gates open)",
        "Undecided or orphaned",
        "root Entity with Manual is active but unsurfaced",
        "surfaced child below a closed ancestor gate is inactive",
        "do not repeat creation",
        "axon list",
        "axon show <ID>",
        "not a complete inventory",
    ] {
        assert!(triage.contains(required), "missing {required:?}: {triage}");
    }
    let show = help_stdout(&["show", "--help"]);
    assert!(show.contains("situation and waits"), "{show}");
}

#[test]
fn condition_evaluating_leaf_help_documents_trace_contract() {
    for args in [
        &["ready", "--help"][..],
        &["triage", "--help"][..],
        &["status", "--help"][..],
        &["start", "--help"][..],
        &["list", "--help"][..],
        &["show", "--help"][..],
        &["import", "check", "--help"][..],
        &["import", "apply", "--help"][..],
    ] {
        let help = help_stdout(args);
        assert!(help.contains("--trace-conditions"), "{args:?}: {help}");
        assert!(
            help.contains("not redacted or truncated"),
            "{args:?}: {help}"
        );
        assert!(
            help.contains("non-UTF-8 bytes are rendered lossily"),
            "{args:?}: {help}"
        );
        assert!(
            help.contains("trace write failure fails"),
            "{args:?}: {help}"
        );
    }
    for args in [
        &["claims", "--help"][..],
        &["log", "--help"][..],
        &["export", "--help"][..],
        &["import", "prepare", "--help"][..],
    ] {
        assert!(
            !help_stdout(args).contains("--trace-conditions"),
            "{args:?}"
        );
    }
}

#[test]
fn help_command_drills_into_nested_command_help() {
    assert_eq!(
        help_stdout(&["help", "group", "plan"]),
        help_stdout(&["group", "plan", "--help"])
    );
}

#[test]
fn docs_explains_the_model_in_terminal_text_without_opening_a_database() {
    let (output, dir) = axon(&["docs"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    for required in [
        "Axon concepts",
        "State axes",
        "Progress",
        "Disposition",
        "Resurface condition",
        "Relationships and derived state",
        "triage requires all four conditions: non-terminal",
        "condition satisfied (surfaced), active scope (all ancestor Group gates open)",
        "and Undecided or orphaned",
        "root Entity with Manual is active but unsurfaced",
        "surfaced child below a closed ancestor gate is inactive",
        "do not repeat creation",
        "axon list for the complete management-root inventory",
        "axon show <ID>",
        "not a complete inventory",
        "Basic workflow",
        "axon plan / axon capture      Create an Issue",
        "axon help <COMMAND PATH>",
        "axon docs declaration",
    ] {
        assert!(stdout.contains(required), "missing {required:?}");
    }
    assert!(!stdout.lines().any(|line| line.starts_with('#')));
    assert!(!stdout.contains('`'));
    assert!(!dir.path().join(".axon").exists());
}

#[test]
fn help_all_is_not_a_command() {
    let (output, _) = axon(&["help", "all"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unrecognized subcommand 'all'"));
}

#[test]
fn removed_slug_commands_are_absent_from_group_help() {
    let help = help_stdout(&["group", "--help"]);
    for command in ["new", "list", "show", "reject", "dep"] {
        assert!(!help.contains(&format!("\n  {command}")), "{command}");
    }
}

#[test]
fn removed_stale_command_is_not_a_compatibility_alias() {
    let (output, _) = axon(&["stale"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unrecognized subcommand"));
    assert!(!help_stdout(&["--help"]).contains("\n  stale"));
}

#[test]
fn help_and_completion_do_not_open_a_database() {
    for args in [
        &[][..],
        &["help"][..],
        &["docs"][..],
        &["docs", "declaration"][..],
        &["docs", "declaration", "--example"][..],
        &["docs", "--help"][..],
        &["--help"][..],
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

    for args in [
        &["docs"][..],
        &["docs", "declaration"][..],
        &["docs", "declaration", "--example"][..],
        &["--help"][..],
        &["-h"][..],
        &["completion", "bash"][..],
    ] {
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

#[test]
fn declaration_help_connects_the_format_and_example() {
    for args in [
        &["import", "--help"][..],
        &["import", "prepare", "--help"][..],
    ] {
        let output = help_stdout(args);
        assert!(output.contains("axon docs declaration"));
        assert!(output.contains("axon docs declaration --example"));
    }
    assert!(help_stdout(&["docs", "--help"]).contains("declaration"));
    assert!(help_stdout(&["docs", "declaration", "--help"]).contains("--example"));
    let output = help_stdout(&["docs", "declaration"]);
    for required in [
        "axon import prepare plan.yml",
        "axon import check plan.yml",
        "axon import apply plan.yml",
        "id",
        "key",
        "base",
        "observed",
        "^[a-z][a-z0-9-]{0,63}$",
        "not_started",
        "claim: null",
        "disposition: accepted",
        "kind: always",
        "axon export",
        "axon decide undecide",
    ] {
        assert!(output.contains(required), "missing {required:?}");
    }
    assert!(!output.contains('`'));
    assert!(!output.contains('\x1b'));
}

#[test]
fn declaration_docs_ignore_a_broken_database() {
    let dir = TestDir::new("docs-broken-db");
    let db_dir = dir.path().join(".axon");
    std::fs::create_dir(&db_dir).unwrap();
    let db_path = db_dir.join("axon.db");
    let broken = b"not a SQLite database";
    std::fs::write(&db_path, broken).unwrap();
    for args in [
        &["docs", "declaration"][..],
        &["docs", "declaration", "--example"][..],
    ] {
        let output = dir.axon_command().args(args).output().unwrap();
        common::assert_success(&output);
        assert!(output.stderr.is_empty());
        assert_eq!(std::fs::read(&db_path).unwrap(), broken);
    }
    assert_eq!(std::fs::read_dir(db_dir).unwrap().count(), 1);
}

#[test]
fn free_text_help_distinguishes_positional_titles_and_option_values() {
    for path in [
        vec!["plan"],
        vec!["capture"],
        vec!["group", "plan"],
        vec!["group", "capture"],
    ] {
        let mut args = path;
        args.push("--help");
        let help = help_stdout(&args);
        assert!(help.contains("Put options before --"), "{help}");
        assert!(help.contains("-m='body' -- '--color text'"), "{help}");
        assert!(help.contains("--message='--help text'"), "{help}");
    }
    for path in [vec!["write"], vec!["note", "add"]] {
        let mut args = path;
        args.push("--help");
        let help = help_stdout(&args);
        assert!(help.contains("--message='--help text'"), "{help}");
        assert!(help.contains("-m='--help text'"), "{help}");
    }
    assert!(help_stdout(&["write", "--help"]).contains("--title='--color text'"));
    for path in [
        vec!["release"],
        vec!["decide", "accept"],
        vec!["decide", "reject"],
        vec!["decide", "undecide"],
        vec!["when", "at"],
        vec!["when", "after"],
        vec!["when", "manual"],
        vec!["when", "command"],
        vec!["when", "clear"],
    ] {
        let mut args = path;
        args.push("--help");
        let help = help_stdout(&args);
        assert!(help.contains("--reason='--help text'"), "{help}");
        assert!(help.contains("-r='--help text'"), "{help}");
    }
}
