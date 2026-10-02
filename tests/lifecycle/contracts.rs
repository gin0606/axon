use super::*;

fn created(text: &str) -> &str {
    text.split_whitespace().next().unwrap()
}
fn suffix(id: &str) -> &str {
    id.rsplit('-').next().unwrap()
}
fn snapshot(f: &Fixture) -> Store {
    f.records()
}
/// Registers an Issue under a fixed ID through the library, as an existing six-character ID
/// or a foreign prefix would appear in a store.
fn seed(f: &Fixture, id: &str) {
    f.publish(vec![registration(&f.records(), id)]);
}

#[test]
fn short_ids_and_suffixes_work_across_mutations() {
    let f = Fixture::new();
    f.ok(&["init", "project"]);
    let group = f.ok(&[
        "capture", "--label", "chore", "--kind", "group", "--accept", "--title", "Parent",
    ]);
    let group = created(&group);
    let dep = f.accepted("Prerequisite");
    for id in [group, &dep] {
        assert!(id.starts_with("project-"));
        assert_eq!(suffix(id).len(), 8);
        assert!(
            suffix(id)
                .bytes()
                .all(|b| b"0123456789abcdefghjkmnpqrstvwxyz".contains(&b))
        );
    }
    let issue = f.ok(&[
        "capture",
        "--label",
        "chore",
        "--title",
        "Work",
        "--parent",
        suffix(group),
        "--needs",
        suffix(&dep),
        "--command",
        "echo should-not-run > observed",
        "--description",
        "目的\nbody",
    ]);
    assert!(issue.contains("Created  Issue  Undecided  Work"));
    let id = created(&issue);
    let short = suffix(id);
    assert!(f.ok(&["show", short]).starts_with(id));
    f.ok(&["accept", short]);
    f.ok(&["start", suffix(&dep)]);
    f.ok(&["complete", suffix(&dep)]);
    f.ok(&[
        "write",
        short,
        "--title",
        "Updated work",
        "--description",
        "Updated body",
    ]);
    f.ok(&["parent", "unset", short]);
    f.ok(&["parent", "set", short, "--parent", suffix(group)]);
    f.ok(&["dep", "rm", short, "--needs", suffix(&dep)]);
    f.ok(&["dep", "add", short, "--needs", suffix(&dep)]);
    f.ok(&["condition", "unset", short]);
    f.ok(&[
        "condition",
        "set",
        short,
        "--command",
        "echo should-not-run > observed",
    ]);
    let note = f.ok(&["note", "add", short, "-m", "Evidence 日本語\nexact body"]);
    let note_id = note.split_whitespace().nth(2).unwrap();
    assert!(note.ends_with(" recorded\n"));
    assert!(
        f.ok(&["note", "show", short, note_id])
            .contains("Evidence 日本語\nexact body")
    );
    failure(f.run(&["note", "show", &dep, note_id]));
    f.ok(&["start", short]);
    f.ok(&["release", short]);
    f.ok(&["cancel", short]);
    f.ok(&["reconsider", short]);
    f.ok(&["accept", short]);
    f.ok(&["withdraw", short]);
    f.ok(&["accept", short]);
    f.ok(&["start", short]);
    f.ok(&["complete", short]);
    f.ok(&["reopen", short]);
    f.ok(&["start", short]);
    f.ok(&["complete", short]);
    let log = f.ok(&["log", short]);
    assert!(log.contains("InProgress → Completed"));
    assert!(log.contains("Completed → NotStarted"));
    assert!(!f.0.join("observed").exists());
    // Six-character IDs from earlier stores stay valid and resolve like any other.
    seed(&f, "project-0000zz");
    seed(&f, "project-1111zz");
    assert!(
        f.ok(&["show", "project-0000zz"])
            .starts_with("project-0000zz  Issue  Ready")
    );
    f.ok(&["start", "project-1111zz"]);
    let before = snapshot(&f);
    let error = failure(f.run(&["start", "zz"]));
    assert!(error.starts_with("Error: zz start:"));
    assert!(
        error.contains("ambiguous")
            && error.contains("project-0000zz")
            && error.contains("project-1111zz")
    );
    assert_eq!(snapshot(&f), before);
    // A complete ID resolves to itself even when a longer ID ends with it.
    seed(&f, "other-project-0000zz");
    assert!(
        f.ok(&["show", "project-0000zz"])
            .starts_with("project-0000zz ")
    );
    assert!(failure(f.run(&["show", "0000zz"])).contains("ambiguous"));
}

#[test]
fn filters_search_current_text_and_do_not_evaluate_excluded_candidates() {
    let f = Fixture::new();
    f.ok(&["init", "q"]);
    let parent = f.ok(&[
        "capture",
        "--label",
        "chore",
        "--kind",
        "group",
        "--accept",
        "--title",
        "Parent",
        "--command",
        "echo parent >> observed",
    ]);
    let parent = created(&parent);
    let a = f.ok(&[
        "capture",
        "--label",
        "chore",
        "--accept",
        "--title",
        "Needle %_",
        "--parent",
        parent,
        "-m",
        "Needle %_ body",
    ]);
    let a = created(&a);
    let b = f.accepted("Other");
    f.ok(&["note", "add", &b, "-m", "Needle %_ historical"]);
    f.ok(&["condition", "set", &b, "--command", "exit 19"]);
    f.ok(&["write", &b, "-m", "current text without query"]);
    let unrelated = f.ok(&[
        "capture",
        "--label",
        "chore",
        "--kind",
        "group",
        "--accept",
        "--title",
        "Excluded",
        "--command",
        "exit 17",
    ]);
    let unrelated = created(&unrelated);
    let rows = f.ok(&[
        "list",
        "--kind",
        "issue",
        "--lifecycle",
        "not-started",
        "--terminal=false",
        "--search",
        "Needle %_",
    ]);
    assert!(rows.contains(a) && !rows.contains(&b));
    assert!(!rows.contains(parent) && !rows.contains(unrelated));
    assert!(rows.contains("Matched: Title, Description"));
    assert!(!f.0.join("observed").exists());
    let candidates = f.ok(&["tasks", "--kind", "issue", "--search", "Needle %_"]);
    assert!(candidates.contains(a) && !candidates.contains(&b));
    assert_eq!(
        fs::read_to_string(f.0.join("observed")).unwrap(),
        "parent\n"
    );
    failure(f.run(&["tasks"]));
    let empty = f.run(&["list", "--search", "needle"]);
    assert!(empty.status.success() && empty.stdout.is_empty());
    assert!(String::from_utf8_lossy(&empty.stderr).contains("No matching Entities"));
    assert_eq!(f.run(&["list", "--search="]).status.code(), Some(2));
    f.ok(&["cancel", &b]);
    assert!(f.ok(&["list", "--terminal=true"]).contains(&b));
    assert!(!f.ok(&["list", "--terminal=false"]).contains(&b));
}

#[test]
fn one_registration_command_selects_kind_and_adoption_independently() {
    let f = Fixture::new();
    f.init();
    for (args, expected) in [
        (vec!["capture", "--label", "chore"], "Issue  Undecided"),
        (
            vec!["capture", "--label", "chore", "--accept"],
            "Issue  NotStarted",
        ),
        (
            vec!["capture", "--label", "chore", "--kind", "group"],
            "Group  Undecided",
        ),
        (
            vec!["capture", "--label", "chore", "--kind", "group", "--accept"],
            "Group  NotStarted",
        ),
    ] {
        let mut args = args;
        args.extend(["--title", "Registered"]);
        assert!(f.ok(&args).contains(expected), "{args:?}");
    }
    let group = f.ok(&[
        "capture", "--label", "chore", "--kind", "group", "--accept", "--title", "Root",
    ]);
    let group = created(&group);
    let dependency = f.accepted("Prerequisite");
    let child = f.ok(&[
        "capture",
        "--label",
        "chore",
        "--kind",
        "group",
        "--accept",
        "--title",
        "Child",
        "--parent",
        group,
        "--needs",
        &dependency,
        "--command",
        "exit 1",
    ]);
    let child = created(&child);
    let details = f.ok(&["show", child, "--details"]);
    assert!(details.contains(group) && details.contains(&dependency));
    assert!(details.contains("exit 1"));
    assert!(f.ok(&["show", group]).contains(child));
}

#[test]
fn details_show_saved_relationships_once_and_skip_conditions_runs_nothing() {
    let f = Fixture::new();
    f.init();
    let parent = f.ok(&[
        "capture", "--label", "chore", "--kind", "group", "--title", "Plan",
    ]);
    let parent = created(&parent);
    let dep = f.accepted("Dependency");
    f.ok(&["start", &dep]);
    f.ok(&["complete", &dep]);
    let a = f.ok(&[
        "capture",
        "--label",
        "chore",
        "--accept",
        "--title",
        "Subject",
        "--parent",
        parent,
        "--needs",
        &dep,
        "--command",
        "echo wrong > observed",
    ]);
    let a = created(&a);
    let dependent = f.ok(&[
        "capture", "--label", "chore", "--accept", "--title", "Consumer", "--needs", a,
    ]);
    let dependent = created(&dependent);
    let plain = f.ok(&["show", a, "--skip-conditions"]);
    assert!(
        plain.contains("Required to start") && !plain.contains(&dep) && !plain.contains(dependent)
    );
    let details = f.ok(&["show", a, "--details", "--skip-conditions"]);
    for id in [a, parent, &dep, dependent] {
        assert_eq!(details.matches(id).count(), 1, "{details}");
    }
    assert!(!details.contains("Required to start"));
    assert!(
        details.contains("Lifecycle: NotStarted")
            && details.contains("Condition: echo wrong > observed")
    );
    assert!(!f.0.join("observed").exists());
    // Without --skip-conditions, --details still evaluates the situation.
    let details = f.ok(&["show", a, "--details"]);
    assert!(
        details.contains("Issue  Blocked  chore  Subject"),
        "{details}"
    );
    assert!(f.0.join("observed").exists());
}

#[test]
fn no_op_confirmation_uses_locked_state_and_keeps_snapshot_and_bytes() {
    let f = Fixture::new();
    f.ok(&["init", "n"]);
    let a = f.accepted("Text");
    let b = f.accepted("Needs");
    f.ok(&["dep", "add", &a, "--needs", &b]);
    let before = snapshot(&f);
    let files = f.record_files();
    for args in [
        vec!["write", &a, "--title", "Text"],
        vec!["parent", "unset", &a],
        vec!["dep", "add", &a, "--needs", &b],
        vec!["condition", "unset", &a],
    ] {
        let out = f.ok(&args);
        assert!(out.starts_with(&a) && out.contains("No changes"));
        assert_eq!(snapshot(&f), before);
        assert_eq!(f.record_files(), files);
    }
    // A terminal Entity rejects an edit even to its own values: its text is fixed.
    f.ok(&["start", &b]);
    f.ok(&["complete", &b]);
    let rejected = failure(f.run(&["write", &b, "--title", "Needs"]));
    assert!(rejected.contains("fixed"), "{rejected}");
    let rejected = failure(f.run(&["write", &b, "-m", ""]));
    assert!(rejected.contains("fixed"), "{rejected}");
    assert_eq!(f.record_files().len(), files.len() + 2);
    assert!(
        f.ok(&["write", &a, "--title", "Text", "-m", "new description"])
            .contains("Description updated")
    );
    assert!(
        !f.ok(&["write", &a, "--title", "Text", "-m", ""])
            .contains("Title updated")
    );
}

#[test]
fn file_flags_share_spelling_and_preserve_text() {
    let f = Fixture::new();
    f.ok(&["init", "text"]);
    let path = f.0.join("body.txt");
    let body = "  本文\nsecond line\n";
    // Multi-line text is indented among structural lines; the stored value is unchanged.
    let shown = "    本文\n  second line\n";
    fs::write(&path, body).unwrap();
    let path = path.to_str().unwrap();
    for flag in ["-F", "--file"] {
        let output = f.ok(&["capture", "--label", "chore", "--title", "Text", flag, path]);
        let id = created(&output);
        assert_eq!(f.current(id).description, body);
        assert!(f.ok(&["show", id]).contains(shown));
        f.ok(&["write", id, "--description", "temporary"]);
        f.ok(&["write", id, flag, path]);
        assert_eq!(f.current(id).description, body);
        assert!(f.ok(&["show", id]).contains(shown));
        f.ok(&["note", "add", id, flag, path]);
        assert!(f.ok(&["note", "list", id]).contains(shown));
        let before = snapshot(&f);
        for args in [
            vec![
                "capture",
                "--label",
                "chore",
                "--title",
                "Rejected",
                "--description-file",
                path,
            ],
            vec!["write", id, "--description-file", path],
            vec!["note", "add", id, "--description-file", path],
            vec![
                "capture",
                "--label",
                "chore",
                "--title",
                "Rejected",
                "--description",
                "inline",
                flag,
                path,
            ],
            vec!["write", id, "--description", "inline", flag, path],
            vec!["note", "add", id, "--message", "inline", flag, path],
        ] {
            let output = f.run(&args);
            assert_eq!(output.status.code(), Some(2), "{args:?}");
            assert!(output.stdout.is_empty());
            assert!(!output.stderr.is_empty());
            assert_eq!(snapshot(&f), before);
        }
    }
}

#[test]
fn utility_commands_work_without_discovery_and_timeout_units_validate_before_storage() {
    let f = Fixture::new();
    fs::write(f.0.join(".git"), "broken marker").unwrap();
    let help = f.ok(&[]);
    for args in [vec!["help"], vec!["-h"], vec!["--help"]] {
        assert_eq!(f.ok(&args), help);
    }
    let expected_sections: &[(&str, &[&str])] = &[
        ("Registration", &["capture"]),
        (
            "Lifecycle transitions",
            &[
                "accept",
                "withdraw",
                "start",
                "release",
                "complete",
                "cancel",
                "reconsider",
                "reopen",
            ],
        ),
        (
            "Candidates & inspection",
            &[
                "proposals",
                "tasks",
                "show",
                "list",
                "log",
                "note",
                "actor",
                "export",
            ],
        ),
        (
            "Text & relationships",
            &[
                "write",
                "label",
                "parent",
                "dep",
                "condition",
                "convert",
                "import",
            ],
        ),
        (
            "Setup & utilities",
            &["init", "storage", "resolve", "completion", "docs", "help"],
        ),
    ];
    let mut previous = 0;
    for (heading, names) in expected_sections {
        let heading = format!("{heading}:\n");
        let offset = help.find(&heading).unwrap();
        assert!(offset > previous);
        previous = offset;
        let section = help[offset + heading.len()..].split("\n\n").next().unwrap();
        let commands: Vec<_> = section
            .lines()
            .map(|line| line.split_whitespace().next().unwrap())
            .collect();
        assert_eq!(&commands, names);
    }
    for args in [
        vec!["capture", "--help"],
        vec!["write", "--help"],
        vec!["note", "add", "--help"],
    ] {
        let leaf = f.ok(&args);
        assert!(leaf.contains("-F, --file"));
    }
    let docs = f.ok(&["docs"]);
    assert!(docs.contains("Cancelled is terminal"));
    assert!(docs.contains("Short flags select inline (-m) or file (-F) input"));
    assert!(docs.contains("--description or --message"));
    assert!(f.ok(&["help", "note", "show"]).contains("<NOTE_ID>"));
    assert!(!f.ok(&["actor"]).is_empty());
    let version = f.ok(&["--version"]);
    assert_eq!(f.ok(&["-V"]), version);
    assert_eq!(
        version.trim_end(),
        format!("axon {}", env!("CARGO_PKG_VERSION"))
    );
    for shell in ["bash", "zsh", "fish", "powershell", "elvish"] {
        let script = f.ok(&["completion", shell]);
        assert!(!script.contains('\x1b'));
        assert!(script.contains("tasks"));
        assert!(script.contains("proposals"));
    }
    for value in [
        "0ms",
        "30",
        "-1s",
        "+1s",
        "0.5s",
        "NaNs",
        "18446744073709551615h",
    ] {
        let out = f.run(&["tasks", &format!("--condition-timeout={value}")]);
        assert_eq!(out.status.code(), Some(2));
        failure(out);
    }
    assert!(!f.0.join(".axon").exists());
    for args in [
        vec![
            "capture",
            "--label",
            "chore",
            "--accept",
            "old positional title",
        ],
        vec![
            "capture",
            "--label",
            "chore",
            "--title",
            "Title",
            "--message",
            "old body",
        ],
        vec![
            "capture",
            "--label",
            "chore",
            "--accept",
            "--title",
            "Title",
            "--description-file",
            "old.md",
        ],
    ] {
        assert_eq!(f.run(&args).status.code(), Some(2));
    }
    fs::remove_file(f.0.join(".git")).unwrap();
    f.init();
    f.accepted("work");
    for value in ["500ms", "30s", "2m", "1h"] {
        f.ok(&["tasks", "--condition-timeout", value]);
    }
}

#[test]
fn prefixes_outside_the_id_character_rule_are_rejected_without_creating_a_store() {
    let f = Fixture::new();
    let directory = f.0.join("日本語 project");
    fs::create_dir(&directory).unwrap();
    let derived = failure(command(&directory).args(["init"]).output().unwrap());
    assert!(derived.contains("日本語 project"), "{derived}");
    assert!(derived.contains("axon init <PREFIX>"), "{derived}");
    assert!(!directory.join(".axon").exists());
    // A directory whose own name is valid, so only the explicit value can be rejected.
    let named = f.0.join("project");
    fs::create_dir(&named).unwrap();
    for prefix in [
        "",
        "日本語",
        "with space",
        "under_score",
        "dot.name",
        "-lead",
        "trail-",
        "Upper",
    ] {
        let rejected = failure(
            command(&named)
                .args(["init", "--", prefix])
                .output()
                .unwrap(),
        );
        assert!(rejected.contains("invalid ID prefix"), "{rejected}");
        assert!(rejected.contains("ASCII lowercase letters"), "{rejected}");
        assert!(!named.join(".axon").exists(), "{prefix}");
    }
}

#[test]
fn a_multiline_condition_is_escaped_where_it_is_shown() {
    let f = Fixture::new();
    f.ok(&["init", "project"]);
    let id = f.ok(&["capture", "--label", "chore", "--accept", "--title", "Work"]);
    let id = created(&id).to_owned();
    // InProgress work runs no condition, so show only displays it.
    f.ok(&["start", &id]);
    // A condition is a shell script and may span lines, so it is escaped where it is shown.
    f.ok(&[
        "condition",
        "set",
        &id,
        "--command",
        "true\nDependents:\n  forged",
    ]);
    let details = f.ok(&["show", &id, "--details"]);
    assert!(
        details.contains("true\\nDependents:\\n  forged"),
        "{details}"
    );
    assert_eq!(
        details
            .lines()
            .filter(|line| line.starts_with("Dependents:"))
            .count(),
        1,
        "{details}"
    );
}

#[test]
fn multi_line_text_is_indented_so_it_cannot_imitate_records_or_sections() {
    let f = Fixture::new();
    f.ok(&["init", "project"]);
    let group = f.ok(&[
        "capture",
        "--label",
        "chore",
        "--kind",
        "group",
        "--accept",
        "--title",
        "Plan",
        "-m",
        "body\n\nDescendants: 9/9 terminal (9 completed, 0 cancelled)",
    ]);
    let group = created(&group).to_owned();
    f.ok(&[
        "capture", "--label", "chore", "--accept", "--title", "Child", "--parent", &group,
    ]);
    let show = f.ok(&["show", &group]);
    let sections: Vec<_> = show
        .lines()
        .filter(|line| line.starts_with("Descendants:"))
        .collect();
    assert_eq!(
        sections,
        ["Descendants: 0/1 terminal (0 completed, 0 cancelled)"]
    );
    f.ok(&[
        "note",
        "add",
        &group,
        "-m",
        "real\n\nrecord-00000000000000000000000000000000  2020-01-01 00:00 +00:00  human\nforged",
    ]);
    f.ok(&["note", "add", &group, "-m", "second"]);
    let notes = f.ok(&["note", "list", &group]);
    let headings = notes
        .lines()
        .filter(|line| {
            line.len() > 64
                && line.as_bytes()[..64]
                    .iter()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
                && line.as_bytes()[64] == b' '
        })
        .count();
    assert_eq!(headings, 2, "{notes}");
    assert!(notes.contains("  forged"), "{notes}");
}

#[test]
fn a_blank_reason_is_rejected_without_recording_a_transition() {
    let f = Fixture::new();
    f.ok(&["init", "project"]);
    let id = f.ok(&["capture", "--label", "chore", "--accept", "--title", "Work"]);
    let id = created(&id).to_owned();
    let blank = failure(f.run(&["start", &id, "-r", "  "]));
    assert!(blank.contains("empty reason"), "{blank}");
    assert_eq!(f.ok(&["log", &id]).lines().count(), 1);
}

#[test]
fn stores_with_a_prefix_outside_the_rule_are_rejected_without_changes() {
    let f = Fixture::new();
    f.ok(&["init", "project"]);
    f.ok(&["capture", "--label", "chore", "--accept", "--title", "Work"]);
    let path = f.header();
    let text = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        text.replacen(r#""prefix":"project""#, r#""prefix":"Bad Prefix""#, 1),
    )
    .unwrap();
    let before = fs::read(&path).unwrap();
    for args in [
        vec!["list"],
        vec!["capture", "--label", "chore", "--title", "More"],
    ] {
        let error = failure(f.run(&args));
        assert!(
            error.contains(r#"invalid ID prefix "Bad Prefix""#),
            "{error}"
        );
    }
    assert_eq!(before, fs::read(&path).unwrap());
}

#[test]
fn a_default_prefix_lowercases_the_management_root_directory_name() {
    let f = Fixture::new();
    let directory = f.0.join("Plan-2");
    fs::create_dir(&directory).unwrap();
    success(command(&directory).args(["init"]).output().unwrap());
    let output = success(
        command(&directory)
            .args(["capture", "--label", "chore", "--accept", "--title", "Work"])
            .output()
            .unwrap(),
    );
    assert!(output.starts_with("plan-2-"), "{output}");
    let header =
        record::decode_header(&fs::read(directory.join(".axon/header.json")).unwrap()).unwrap();
    assert_eq!(header.prefix, "plan-2");
    let id = output.split_whitespace().next().unwrap().to_string();
    assert!(
        success(
            command(&directory)
                .args(["show", suffix(&id)])
                .output()
                .unwrap()
        )
        .starts_with(&id)
    );
}

#[cfg(unix)]
#[test]
fn non_pipe_output_failure_identifies_applied_storage() {
    let f = Fixture::new();
    f.init();
    use std::os::fd::{FromRawFd, OwnedFd};
    let mut sockets = [0; 2];
    assert_eq!(
        unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_DGRAM, 0, sockets.as_mut_ptr()) },
        0
    );
    assert_eq!(unsafe { libc::close(sockets[0]) }, 0);
    let writer = unsafe { OwnedFd::from_raw_fd(sockets[1]) };
    let out = f
        .command()
        .args([
            "capture", "--label", "chore", "--accept", "--title", "Retained",
        ])
        .stdout(Stdio::from(writer))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let error = String::from_utf8_lossy(&out.stderr);
    assert!(error.contains("Applied:") && error.contains("storage applied; output failed"));
    assert!(f.ok(&["list"]).contains("Retained"));
    // A mutation that wrote nothing does not claim a publication when its output fails.
    let id = f
        .ok(&["list"])
        .split_whitespace()
        .next()
        .unwrap()
        .to_owned();
    let mut sockets = [0; 2];
    assert_eq!(
        unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_DGRAM, 0, sockets.as_mut_ptr()) },
        0
    );
    assert_eq!(unsafe { libc::close(sockets[0]) }, 0);
    let writer = unsafe { OwnedFd::from_raw_fd(sockets[1]) };
    let out = f
        .command()
        .args(["write", &id, "--title", "Retained"])
        .stdout(Stdio::from(writer))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let error = String::from_utf8_lossy(&out.stderr);
    assert!(
        error.contains("output failed") && !error.contains("storage applied"),
        "{error}"
    );
}

#[cfg(unix)]
fn terminal_output(f: &Fixture, args: &[&str], no_color: bool) -> String {
    sized_terminal_output(f, args, no_color, 0)
}
/// Output on a terminal `columns` wide; 0 leaves the width unknown.
#[cfg(unix)]
fn sized_terminal_output(f: &Fixture, args: &[&str], no_color: bool, columns: u16) -> String {
    use std::io::Read;
    use std::os::fd::FromRawFd;
    let (mut master, mut slave) = (0, 0);
    let mut size = libc::winsize {
        ws_row: 24,
        ws_col: columns,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    assert_eq!(
        unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                // The parameter is `*mut` on macOS and `*const` on Linux.
                &raw mut size,
            )
        },
        0
    );
    let mut reader = unsafe { fs::File::from_raw_fd(master) };
    let writer = unsafe { fs::File::from_raw_fd(slave) };
    let mut cmd = f.command();
    cmd.args(args)
        .env("TERM", "xterm-256color")
        .env_remove("NO_COLOR");
    if no_color {
        cmd.env("NO_COLOR", "");
    }
    for fd in [master, slave] {
        assert_eq!(
            unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) },
            0
        );
    }
    let child = cmd
        .stdout(Stdio::from(writer))
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(cmd);
    let reader_thread = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut buffer = [0; 4096];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => bytes.extend_from_slice(&buffer[..n]),
                Err(e) if e.raw_os_error() == Some(libc::EIO) => break,
                Err(e) => panic!("{e}"),
            }
        }
        bytes
    });
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let bytes = reader_thread.join().unwrap();
    String::from_utf8(bytes).unwrap().replace("\r\n", "\n")
}
fn strip_sgr(value: &str) -> String {
    let mut text = String::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            assert_eq!(chars.next(), Some('['));
            for next in chars.by_ref() {
                if next == 'm' {
                    break;
                }
                assert!(next.is_ascii_digit() || next == ';');
            }
        } else {
            text.push(c);
        }
    }
    text
}
#[cfg(unix)]
#[test]
fn tty_decoration_preserves_text_and_does_not_style_user_content() {
    let f = Fixture::new();
    f.init();
    let id = f.accepted("USER_TITLE");
    for args in [
        vec!["show", &id],
        vec!["--help"],
        vec!["complete", "--help"],
    ] {
        let plain = f.ok(&args);
        let colored = terminal_output(&f, &args, false);
        assert!(colored.contains('\x1b'), "{colored}");
        assert_eq!(strip_sgr(&colored), plain);
        assert_eq!(terminal_output(&f, &args, true), plain);
        if args[0] == "show" {
            assert!(colored.contains("  USER_TITLE\n"));
        }
    }
    // A confirmation that carries a derived situation is decorated once, not escaped.
    let records = f.records();
    let heads: Vec<_> = (0..2)
        .map(|_| {
            Entry::Record(
                records
                    .perform(
                        &eid(&id),
                        axon::lifecycle::Operation::Start,
                        None,
                        context(),
                    )
                    .unwrap(),
            )
        })
        .collect();
    let head = heads[0].id().unwrap().to_string();
    f.publish(heads);
    let args = ["resolve", &id, "--head", &head];
    let colored = terminal_output(&f, &args, false);
    assert!(
        colored.contains('\x1b') && !colored.contains("\\x1b"),
        "{colored}"
    );
    assert_eq!(
        strip_sgr(&colored),
        format!("{id}  Resolved: {head}  InProgress\n")
    );
}

#[cfg(unix)]
#[test]
fn terminal_lists_line_up_columns_and_wrap_titles_in_the_title_column() {
    let f = Fixture::new();
    f.init();
    // Six- and eight-character IDs, and situations and labels of different lengths.
    let short = "t-0000zz";
    f.publish(vec![registration(&f.records(), short)]);
    let long_title = "0123456789".repeat(4);
    let started = f.accepted(&long_title);
    f.ok(&["start", &started]);
    let waiting = f.ok(&[
        "capture", "--label", "refactor", "--accept", "--title", "Waiting", "--needs", short,
    ]);
    let waiting = created(&waiting);
    let proposal = f.ok(&["capture", "--label", "feat", "--title", "Proposal"]);
    let proposal = created(&proposal);
    // Columns 12 + 7 + 12 + 10 put the title at 41; a 70-column terminal leaves it 29.
    let blank = " ".repeat(41);
    let tasks = format!(
        "{short}    Issue  Ready       chore     {short}\n\
         {started}  Issue  InProgress  chore     {}\n\
         {blank}{}\n\
         {waiting}  Issue  Blocked     refactor  Waiting\n",
        &long_title[..29],
        &long_title[29..]
    );
    let list = format!("{tasks}{proposal}  Issue  Undecided   feat      Proposal\n");
    for (args, expected) in [
        (vec!["list"], list.clone()),
        (vec!["tasks"], tasks.clone()),
        (
            vec!["proposals"],
            format!("{proposal}  Issue  Undecided  feat  Proposal\n"),
        ),
    ] {
        assert_eq!(sized_terminal_output(&f, &args, true, 70), expected);
        let colored = sized_terminal_output(&f, &args, false, 70);
        assert!(colored.contains('\x1b'), "{colored}");
        assert_eq!(strip_sgr(&colored), expected);
    }
    // Without a known width the columns still line up, and titles do not wrap.
    assert_eq!(
        sized_terminal_output(&f, &["list"], true, 0),
        list.replace(&format!("\n{blank}"), "")
    );
    // A pipe keeps one row per line with cells joined by two spaces.
    assert!(f.ok(&["list"]).contains(&format!(
        "\n{started}  Issue  InProgress  chore  {long_title}\n"
    )));
    assert_eq!(
        sized_terminal_output(&f, &["list", "--search", "Wait"], true, 70),
        format!("{waiting}  Issue  Blocked  refactor  Waiting\n  Matched: Title\n")
    );
    // Wait reasons are sentences, not table rows, and keep their pipe form in a terminal.
    let shown = f.ok(&["show", waiting]);
    assert!(
        shown.contains(&format!(
            "Dependency must complete: {short}  Issue  Ready  chore  {short}\n"
        )),
        "{shown}"
    );
    assert_eq!(
        sized_terminal_output(&f, &["show", waiting], true, 70),
        shown
    );
}

#[cfg(unix)]
#[test]
fn terminal_tree_lines_up_titles_across_depths_and_keeps_its_lines_when_wrapping() {
    let f = Fixture::new();
    f.init();
    let root = f.ok(&[
        "capture", "--label", "chore", "--kind", "group", "--accept", "--title", "Root",
    ]);
    let root = created(&root);
    let sub_title = "S".repeat(30);
    let sub = f.ok(&[
        "capture", "--label", "chore", "--kind", "group", "--accept", "--title", &sub_title,
        "--parent", root,
    ]);
    let sub = created(&sub);
    let deep_title = "0123456789".repeat(4);
    let deep = f.ok(&[
        "capture",
        "--label",
        "chore",
        "--accept",
        "--title",
        &deep_title,
        "--parent",
        sub,
    ]);
    let deep = created(&deep);
    let last_title = "L".repeat(30);
    let last = f.ok(&[
        "capture",
        "--label",
        "chore",
        "--kind",
        "group",
        "--accept",
        "--title",
        &last_title,
        "--parent",
        root,
    ]);
    let last = created(&last);
    let leaf = f.ok(&[
        "capture", "--label", "chore", "--accept", "--title", "Leaf", "--parent", last,
    ]);
    let leaf = created(&leaf);
    // The deepest drawing and ID take 18 columns, which puts the title at 18 + 23 = 41 and
    // leaves it 26 of 67 columns.
    let tree = |text: &str| text[text.find("├── ").unwrap()..].to_string();
    let expected = format!(
        "├── {sub}      Group  Ready  chore  {}\n\
         │   │{}{}\n\
         │   └── {deep}  Issue  Ready  chore  {}\n\
         │{}{}\n\
         └── {last}      Group  Ready  chore  {}\n\
         \x20   │{}{}\n\
         \x20   └── {leaf}  Issue  Ready  chore  Leaf\n",
        &sub_title[..26],
        " ".repeat(36),
        &sub_title[26..],
        &deep_title[..26],
        " ".repeat(40),
        &deep_title[26..],
        &last_title[..26],
        " ".repeat(36),
        &last_title[26..],
    );
    assert_eq!(
        tree(&sized_terminal_output(&f, &["show", root], true, 67)),
        expected
    );
    let colored = sized_terminal_output(&f, &["show", root], false, 67);
    assert_eq!(tree(&strip_sgr(&colored)), expected);
    // A pipe keeps the tree drawing followed by cells joined by two spaces.
    assert_eq!(
        tree(&f.ok(&["show", root])),
        format!(
            "├── {sub}  Group  Ready  chore  {sub_title}\n\
             │   └── {deep}  Issue  Ready  chore  {deep_title}\n\
             └── {last}  Group  Ready  chore  {last_title}\n\
             \x20   └── {leaf}  Issue  Ready  chore  Leaf\n"
        )
    );
}

#[test]
fn show_group_displays_all_descendants_in_tree_order_and_counts_terminal_entities() {
    let f = Fixture::new();
    f.ok(&["init", "t"]);
    let root = f.ok(&[
        "capture", "--label", "chore", "--kind", "group", "--accept", "--title", "Root",
    ]);
    let root = created(&root);
    assert!(
        f.ok(&["show", root])
            .contains("Descendants: 0/0 terminal (0 completed, 0 cancelled)")
    );
    let group = f.ok(&[
        "capture", "--label", "chore", "--kind", "group", "--accept", "--title", "Branch",
        "--parent", root,
    ]);
    let group = created(&group);
    let sibling = f.ok(&[
        "capture", "--label", "chore", "--accept", "--title", "Sibling", "--parent", root,
    ]);
    let sibling = created(&sibling);
    let nested = f.ok(&[
        "capture", "--label", "chore", "--kind", "group", "--accept", "--title", "Nested",
        "--parent", group,
    ]);
    let nested = created(&nested);
    let leaf = f.ok(&[
        "capture", "--label", "chore", "--accept", "--title", "Leaf", "--parent", nested,
    ]);
    let leaf = created(&leaf);
    let cancelled = f.ok(&[
        "capture",
        "--label",
        "chore",
        "--accept",
        "--title",
        "Cancelled leaf",
        "--parent",
        group,
    ]);
    let cancelled = created(&cancelled);
    let unrelated = f.accepted("Unrelated");
    f.ok(&["cancel", cancelled]);
    f.ok(&["start", leaf]);
    f.ok(&["complete", leaf]);
    f.ok(&["complete", nested]);
    for args in [vec!["show", root], vec!["show", root, "--details"]] {
        let output = f.ok(&args);
        let tree = output.split("Descendants: ").nth(1).unwrap();
        assert!(tree.starts_with("3/5 terminal (2 completed, 1 cancelled)\n"));
        let rows: Vec<_> = tree.lines().skip(1).collect();
        assert_eq!(rows.len(), 5, "{output}");
        for (line, prefix, id) in [
            (rows[0], "├── ", group),
            (rows[1], "│   ├── ", nested),
            (rows[2], "│   │   └── ", leaf),
            (rows[3], "│   └── ", cancelled),
            (rows[4], "└── ", sibling),
        ] {
            assert!(line.starts_with(&format!("{prefix}{id}  ")), "{line}");
        }
        assert!(!output.contains(&unrelated));
        assert!(!output.contains("Awaiting final confirmation"));
    }
    assert!(!f.ok(&["show", leaf]).contains("Descendants:"));
    f.ok(&["complete", group]);
    f.ok(&["cancel", sibling]);
    let output = f.ok(&["show", root]);
    assert!(output.contains("Descendants: 5/5 terminal (3 completed, 2 cancelled)"));
    assert!(output.contains("Awaiting final confirmation"));
}

#[test]
fn note_search_literal_excerpts_and_scope_match() {
    let f = Fixture::new();
    f.ok(&["init", "q"]);
    let a = f.accepted("TitleOnly");
    f.ok(&["start", &a]);
    f.ok(&["complete", &a]);
    let group = f.ok(&[
        "capture", "--label", "chore", "--kind", "group", "--title", "Group",
    ]);
    let b = created(&group);
    f.ok(&["cancel", b]);
    f.ok(&["condition", "set", &a, "--command", "echo wrong > observed"]);
    f.ok(&["condition", "set", b, "--command", "exit 19"]);
    let cases = [
        (
            "日本語".to_owned(),
            "日本語".to_owned(),
            "日本語".to_owned(),
        ),
        (
            format!("語{}", "後".repeat(30)),
            "語".into(),
            format!("語{}…", "後".repeat(24)),
        ),
        (
            format!("{}語", "前".repeat(30)),
            "語".into(),
            format!("…{}語", "前".repeat(24)),
        ),
        (
            format!("{}語{}語", "前".repeat(30), "後".repeat(30)),
            "語".into(),
            format!("…{}語{}…", "前".repeat(24), "後".repeat(24)),
        ),
        ("長".repeat(200), "長".repeat(200), "長".repeat(200)),
        (
            "a\nb\\n\t\r\u{1b}\u{85}\u{2028}\u{61c}\u{200e}\u{200f}\u{202a}\u{202b}\u{202c}\u{202d}\u{202e}\u{2066}\u{2067}\u{2068}\u{2069}".into(),
            "a\nb".into(),
            "a\\nb\\\\n\\t\\r\\x1b\\x85\\u{2028}\\u{61c}\\u{200e}\\u{200f}\\u{202a}\\u{202b}\\u{202c}\\u{202d}\\u{202e}\\u{2066}\\u{2067}\\u{2068}\\u{2069}".into(),
        ),
        (" %_.* ".into(), " %_.* ".into(), " %_.* ".into()),
    ];
    let mut ids = Vec::new();
    for (body, query, excerpt) in &cases {
        let added = f.ok(&["note", "add", &a, "-m", body]);
        let id = added.split_whitespace().nth(2).unwrap().to_owned();
        let rows = f.ok(&["note", "search", query]);
        let row = rows.lines().find(|line| line.contains(&id)).unwrap();
        assert!(row.starts_with(&a), "{row}");
        assert!(row.ends_with(&format!("Excerpt: {excerpt}")), "{row}");
        assert_eq!(rows.matches(&id).count(), 1);
        assert!(f.ok(&["note", "show", &a, &id]).contains(&id));
        ids.push(id);
    }
    let added = f.ok(&["note", "add", b, "-m", "語 group"]);
    let last = added.split_whitespace().nth(2).unwrap();
    let rows = f.ok(&["note", "search", "語"]);
    let expected = [ids[0].as_str(), &ids[1], &ids[2], &ids[3], last];
    assert_eq!(rows.lines().count(), expected.len());
    for (row, id) in rows.lines().zip(expected) {
        assert!(row.contains(id), "{rows}");
    }
    f.ok(&["note", "add", b, "-m", "e\u{301} --text literal\\n"]);
    assert!(f.ok(&["note", "search", "--", "--text"]).contains("--text"));
    assert!(f.ok(&["note", "search", "e\u{301}"]).contains("e\u{301}"));
    assert!(
        f.ok(&["note", "search", "literal\\n"])
            .contains("literal\\\\n")
    );
    for query in ["TitleOnly", "日本語 ", "GROUP", "é", "\\x1b"] {
        let empty = f.run(&["note", "search", query]);
        assert!(empty.status.success() && empty.stdout.is_empty());
        assert!(String::from_utf8_lossy(&empty.stderr).contains("No matching Notes"));
    }
    for args in [
        vec!["note", "search", ""],
        vec!["note", "search"],
        vec!["note", "list"],
        vec!["note", "search", "語", "--kind", "issue"],
    ] {
        assert_eq!(f.run(&args).status.code(), Some(2));
    }
    assert!(!f.0.join("observed").exists());
    assert_eq!(snapshot(&f).notes_of(&eid(&a))[5].1.body, cases[5].0);
}

#[test]
fn note_search_orders_entities_by_creation_and_notes_by_time_then_record_id() {
    let f = Fixture::new();
    f.ok(&["init", "q"]);
    let records = f.records();
    let at = context();
    let mut older = at.clone();
    older.at -= chrono::Duration::days(1);
    // Both Entities are registered at the same instant, so their IDs order them.
    let entries = ["q-zzzzzz", "q-aaaaaa"]
        .into_iter()
        .map(|id| Entry::Record(records.create(eid(id), current(id), at.clone()).unwrap()))
        .collect();
    f.publish(entries);
    let records = f.records();
    let id = eid("q-aaaaaa");
    let mut entries = Vec::new();
    let mut expected: Vec<(chrono::DateTime<Utc>, axon::lifecycle::RecordId)> = Vec::new();
    for (body, when) in [
        ("find first", &at),
        ("find left", &older),
        ("find right", &older),
        ("find last", &older),
    ] {
        let note = records
            .add_note(&id, body.into(), None, when.clone())
            .unwrap();
        let entry = Entry::Note(note);
        let note_id = axon::lifecycle::RecordId::of(&axon::lifecycle::encode(&entry).unwrap());
        expected.push((when.at, note_id));
        entries.push(entry);
    }
    let other = records
        .add_note(&eid("q-zzzzzz"), "find other".into(), None, at.clone())
        .unwrap();
    let other_entry = Entry::Note(other);
    let other_id = axon::lifecycle::RecordId::of(&axon::lifecycle::encode(&other_entry).unwrap());
    entries.push(other_entry);
    f.publish(entries);
    // Within an Entity, Notes are ordered by time and then by record ID; the same-time
    // Notes have no causal order, so their IDs decide.
    expected.sort();
    let mut expected: Vec<String> = expected.into_iter().map(|(_, id)| id.to_string()).collect();
    expected.push(other_id.to_string());
    let output = f
        .command()
        .env("TZ", "UTC")
        .args(["note", "search", "find"])
        .output()
        .unwrap();
    let rows = success(output);
    assert_eq!(rows.lines().count(), expected.len());
    for (row, id) in rows.lines().zip(expected) {
        assert!(row.contains(&id) && row.contains("+00:00"), "{row}");
    }
}

#[test]
fn proposals_search_excludes_note_only_candidates_before_conditions() {
    let f = Fixture::new();
    f.ok(&["init", "q"]);
    let parent = f.ok(&[
        "capture",
        "--label",
        "chore",
        "--kind",
        "group",
        "--accept",
        "--title",
        "Parent",
        "--command",
        "echo parent >> observed",
    ]);
    let parent = created(&parent);
    let child = f.ok(&[
        "capture", "--label", "chore", "--title", "needle", "--parent", parent,
    ]);
    let child = created(&child);
    let other = f.ok(&[
        "capture",
        "--label",
        "chore",
        "--title",
        "other",
        "--command",
        "exit 19",
    ]);
    let other = created(&other);
    f.ok(&["note", "add", other, "-m", "needle"]);
    let rows = f.ok(&["proposals", "--kind", "issue", "--search", "needle"]);
    assert!(rows.contains(child) && !rows.contains(other));
    assert_eq!(
        fs::read_to_string(f.0.join("observed")).unwrap(),
        "parent\n"
    );
    assert_eq!(f.run(&["proposals", "--search="]).status.code(), Some(2));
    assert_eq!(f.run(&["tasks", "--search="]).status.code(), Some(2));
}

#[test]
fn group_rows_show_derived_situations_and_show_explains_stalled_groups() {
    let f = Fixture::new();
    f.ok(&["init", "t"]);
    let gate = f.accepted("Gate");
    let outer = f.ok(&[
        "capture", "--label", "chore", "--kind", "group", "--title", "Outer",
    ]);
    let outer = created(&outer).to_owned();
    let plan = f.ok(&[
        "capture", "--label", "chore", "--kind", "group", "--accept", "--title", "Plan",
        "--parent", &outer, "--needs", &gate,
    ]);
    let plan = created(&plan).to_owned();
    let draft = f.ok(&[
        "capture", "--label", "chore", "--title", "Draft", "--parent", &plan,
    ]);
    let draft = created(&draft).to_owned();
    let sub = f.ok(&[
        "capture", "--label", "chore", "--kind", "group", "--accept", "--title", "Sub", "--parent",
        &plan,
    ]);
    let sub = created(&sub).to_owned();
    let leaf = f.ok(&[
        "capture", "--label", "chore", "--accept", "--title", "Leaf", "--parent", &sub, "--needs",
        &gate,
    ]);
    let leaf = created(&leaf).to_owned();
    let hollow = f.ok(&[
        "capture", "--label", "chore", "--kind", "group", "--accept", "--title", "Hollow",
    ]);
    let hollow = created(&hollow).to_owned();
    let rows = f.ok(&["tasks"]);
    let row = |id: &str| rows.lines().find(|l| l.starts_with(id)).unwrap().to_owned();
    assert!(row(&plan).contains("Group  Blocked  chore  Plan"), "{rows}");
    assert!(row(&sub).contains("Group  Blocked  chore  Sub"), "{rows}");
    assert!(
        row(&hollow).contains("Group  Empty  chore  Hollow"),
        "{rows}"
    );
    assert!(row(&leaf).contains("Issue  Blocked  chore  Leaf"), "{rows}");
    assert!(!rows.contains(&outer) && !rows.contains(&draft), "{rows}");
    // The stalled Plan lists every applicable reason with the Entity rows.
    let show = f.ok(&["show", &plan]);
    let stalled = show.split("Stalled\n").nth(1).unwrap();
    for line in [
        format!("Dependency must complete: {gate}  Issue  Ready  chore  Gate"),
        format!(
            "Descendant dependency must complete: {leaf}  Issue  Blocked  chore  Leaf  needs  {gate}  Issue  Ready  chore  Gate"
        ),
        format!("Undecided child: {draft}  Issue  Undecided  chore  Draft"),
        format!("Open subgroup: {sub}  Group  Blocked  chore  Sub"),
        format!("Undecided ancestor: {outer}  Group  Undecided  chore  Outer"),
    ] {
        assert!(stalled.contains(&line), "{line}\n{show}");
    }
    assert!(!show.contains("Required to complete") && !show.contains("Parent:"));
    let sub_show = f.ok(&["show", &sub]);
    assert!(sub_show.contains(&format!("Ancestor dependency must complete: {gate}")));
    assert!(sub_show.contains(&format!("Undecided ancestor: {outer}")));
    assert!(sub_show.contains(&format!("Parent: {plan}")));
    // A completable empty Group is not stalled; an Issue lists its start prerequisites.
    let hollow_show = f.ok(&["show", &hollow]);
    assert!(!hollow_show.contains("Stalled") && !hollow_show.contains("Awaiting"));
    let leaf_show = f.ok(&["show", &leaf]);
    assert!(leaf_show.contains("Required to start"));
    assert!(leaf_show.contains(&format!("Ancestor must be adopted: {outer}")));
    // Only the parent itself named as a reason replaces the parent line.
    assert!(leaf_show.contains(&format!("Parent: {sub}")), "{leaf_show}");
    assert!(leaf_show.contains(&format!("Dependency must complete: {gate}")));
    assert!(leaf_show.contains(&format!("Ancestor dependency must complete: {gate}")));
    // Details show the effective lifecycle of a Group with its saved value when they differ.
    f.ok(&["accept", &outer]);
    f.ok(&["start", &gate]);
    f.ok(&["complete", &gate]);
    let rows = f.ok(&["tasks"]);
    let row = |id: &str| rows.lines().find(|l| l.starts_with(id)).unwrap().to_owned();
    assert!(row(&plan).contains("Group  Ready  chore  Plan"), "{rows}");
    assert!(row(&leaf).contains("Issue  Ready  chore  Leaf"), "{rows}");
    let ready = f.ok(&["show", &plan]);
    assert!(
        ready.contains("Group  Ready  chore  Plan") && !ready.contains("Stalled"),
        "{ready}"
    );
    assert!(
        f.ok(&["show", &plan, "--details"])
            .contains("Lifecycle: NotStarted\n")
    );
    f.ok(&["start", &leaf]);
    f.ok(&["complete", &leaf]);
    f.ok(&["cancel", &draft]);
    let rows = f.ok(&["tasks"]);
    let row = |id: &str| rows.lines().find(|l| l.starts_with(id)).unwrap().to_owned();
    assert!(
        row(&outer).contains("Group  InProgress  chore  Outer"),
        "{rows}"
    );
    assert!(
        row(&plan).contains("Group  InProgress  chore  Plan"),
        "{rows}"
    );
    assert!(
        row(&sub).contains("Group  Confirmable  chore  Sub"),
        "{rows}"
    );
    let details = f.ok(&["show", &plan, "--details"]);
    assert!(
        details.contains("Lifecycle: InProgress (stored NotStarted)\n"),
        "{details}"
    );
    assert!(
        f.ok(&["show", &plan])
            .contains(&format!("Open subgroup: {sub}"))
    );
    assert!(
        f.ok(&["show", &sub])
            .contains("Awaiting final confirmation")
    );
    assert!(!f.ok(&["show", &sub]).contains("Stalled"));
    f.ok(&["complete", &sub]);
    assert!(
        f.ok(&["tasks"])
            .contains(&format!("{plan}  Group  Confirmable  chore  Plan"))
    );
    assert!(
        f.ok(&["list", "--lifecycle", "in-progress"])
            .contains(&outer)
    );
    assert!(
        !f.ok(&["list", "--lifecycle", "not-started"])
            .contains(&outer)
    );
    assert!(
        f.ok(&["show", &sub, "--details"])
            .contains("Lifecycle: Completed\n")
    );
}

#[test]
fn reopen_returns_completed_work_and_groups_are_never_started_or_released() {
    let f = Fixture::new();
    f.ok(&["init", "t"]);
    let base = f.accepted("Base");
    let user = f.ok(&[
        "capture", "--label", "chore", "--accept", "--title", "User", "--needs", &base,
    ]);
    let user = created(&user).to_owned();
    let group = f.ok(&[
        "capture", "--label", "chore", "--kind", "group", "--accept", "--title", "Plan",
    ]);
    let group = created(&group).to_owned();
    for id in [&base, &user] {
        f.ok(&["start", id]);
        f.ok(&["complete", id]);
    }
    let before = snapshot(&f);
    let rejected = failure(f.run(&["reopen", &base]));
    assert!(rejected.contains(&user), "{rejected}");
    assert_eq!(snapshot(&f), before);
    let reopened = f.ok(&["reopen", &user, "-r", "more work"]);
    assert_eq!(reopened, format!("{user}  Reopened  NotStarted\n"));
    f.ok(&["reopen", &base]);
    assert!(f.ok(&["log", &base]).contains("Completed → NotStarted"));
    assert!(
        f.ok(&["tasks"])
            .contains(&format!("{base}  Issue  Ready  chore  Base"))
    );
    failure(f.run(&["reopen", &base]));
    f.ok(&["write", &base, "--title", "Base again"]);
    f.ok(&["start", &base]);
    for args in [vec!["start", &group], vec!["release", &group]] {
        let rejected = failure(f.run(&args));
        assert!(
            rejected.contains("saved lifecycle does not change"),
            "{rejected}"
        );
    }
    f.ok(&["complete", &group]);
    assert!(f.ok(&["reopen", &group]).contains("Reopened  NotStarted"));
    assert!(
        f.ok(&["show", &group])
            .contains("Group  Empty  chore  Plan")
    );
    let docs = f.ok(&["docs"]);
    assert!(docs.contains("reopen returns Completed to NotStarted"));
    assert!(docs.contains("A Group is never started or released"));
    assert!(f.ok(&["reopen", "--help"]).contains("Completed dependents"));
}

#[test]
fn convert_changes_only_the_kind_of_unstarted_work_and_takes_no_reason() {
    let f = Fixture::new();
    f.ok(&["init", "t"]);
    let dependency = f.accepted("Dependency");
    let issue = f.ok(&[
        "capture",
        "--label",
        "chore",
        "--title",
        "Work",
        "-m",
        "body",
        "--needs",
        &dependency,
    ]);
    let issue = created(&issue).to_owned();
    f.ok(&["condition", "set", &issue, "--command", "true"]);
    f.ok(&["note", "add", &issue, "-m", "kept"]);
    // Undecided converts; the same kind again is No changes without a record.
    assert_eq!(
        f.ok(&["convert", &issue, "--kind", "group"]),
        format!("{issue}  Converted: Issue → Group\n")
    );
    let files = f.record_files();
    assert_eq!(
        f.ok(&["convert", &issue, "--kind", "group"]),
        format!("{issue}  No changes  Kind: Group\n")
    );
    assert_eq!(f.record_files(), files);
    let show = f.ok(&["show", &issue, "--details", "--skip-conditions"]);
    assert!(
        show.contains(&format!("{issue}  Group  Undecided  chore  Work")),
        "{show}"
    );
    assert!(show.contains("1 notes") && show.contains("body") && show.contains(&dependency));
    assert!(show.contains("Condition: true"), "{show}");
    assert!(f.ok(&["log", &issue]).contains("Converted: Issue → Group"));
    // A conversion is not a lifecycle transition, so there is no reason to record.
    assert_eq!(
        f.run(&["convert", &issue, "--kind", "issue", "-r", "why"])
            .status
            .code(),
        Some(2)
    );
    // NotStarted converts back; InProgress needs a release first; a Group with children and
    // a terminal Entity are not converted.
    f.ok(&["accept", &issue]);
    f.ok(&["convert", &issue, "--kind", "issue"]);
    f.ok(&["start", &dependency]);
    f.ok(&["complete", &dependency]);
    f.ok(&["start", &issue]);
    let rejected = failure(f.run(&["convert", &issue, "--kind", "group"]));
    assert!(rejected.contains("release"), "{rejected}");
    f.ok(&["release", &issue]);
    f.ok(&["convert", &issue, "--kind", "group"]);
    let child = f.ok(&[
        "capture", "--label", "chore", "--accept", "--title", "Child", "--parent", &issue,
    ]);
    let child = created(&child).to_owned();
    let rejected = failure(f.run(&["convert", &issue, "--kind", "issue"]));
    assert!(rejected.contains(&child), "{rejected}");
    f.ok(&["start", &child]);
    f.ok(&["complete", &child]);
    f.ok(&["complete", &issue]);
    let rejected = failure(f.run(&["convert", &issue, "--kind", "issue"]));
    assert!(rejected.contains("reopen or reconsider"), "{rejected}");
    let rejected = failure(f.run(&["convert", &dependency, "--kind", "group"]));
    assert!(rejected.contains("reopen or reconsider"), "{rejected}");
    assert!(
        f.ok(&["convert", "--help"])
            .contains("reread an Issue's description after converting it")
    );
}

#[test]
fn group_dependencies_gate_starts_below_and_the_group_itself_but_not_completion_below() {
    let f = Fixture::new();
    f.ok(&["init", "t"]);
    let group_needing = |title: &str, dep: &str| -> String {
        let out = f.ok(&[
            "capture", "--label", "chore", "--kind", "group", "--accept", "--title", title,
            "--needs", dep,
        ]);
        created(&out).to_owned()
    };
    let child = |title: &str, parent: &str, kind: &str| -> String {
        let out = f.ok(&[
            "capture", "--label", "chore", "--kind", kind, "--accept", "--title", title,
            "--parent", parent,
        ]);
        created(&out).to_owned()
    };
    let own_row = |id: &str| f.ok(&["show", id]).lines().next().unwrap().to_owned();
    let completed = |id: &str| {
        let row = own_row(id);
        assert!(row.contains("  Completed  "), "{row}");
    };
    let group_rejected_by_its_dependency = |group: &str| {
        let rejected = failure(f.run(&["complete", group]));
        assert!(
            rejected.contains("dependencies must be Completed"),
            "{rejected}"
        );
        assert!(
            !own_row(group).contains("  Completed  "),
            "{}",
            own_row(group)
        );
    };

    // 1. Empty child Groups and child Groups whose children are all Cancelled complete
    //    without any Start below the Group, so the Group's dependency never sees them.
    let dep = f.accepted("Prerequisite 1");
    let plan = group_needing("Plan 1", &dep);
    let empty = child("Empty", &plan, "group");
    let cancelled = child("Cancelled", &plan, "group");
    let dropped = child("Dropped", &cancelled, "issue");
    f.ok(&["cancel", &dropped]);
    f.ok(&["complete", &empty]);
    f.ok(&["complete", &cancelled]);
    completed(&empty);
    completed(&cancelled);
    group_rejected_by_its_dependency(&plan);

    // 2. Work started outside the Group, or moved out to start, completes after moving in.
    let dep = f.accepted("Prerequisite 2");
    let plan = group_needing("Plan 2", &dep);
    let outside = f.accepted("Outside");
    f.ok(&["start", &outside]);
    f.ok(&["parent", "set", &outside, "--parent", &plan]);
    f.ok(&["complete", &outside]);
    completed(&outside);
    let inside = child("Inside", &plan, "issue");
    let rejected = failure(f.run(&["start", &inside]));
    assert!(
        rejected.contains(&format!(
            "dependencies of ancestor {plan} must be Completed"
        )),
        "{rejected}"
    );
    f.ok(&["parent", "unset", &inside]);
    f.ok(&["start", &inside]);
    f.ok(&["parent", "set", &inside, "--parent", &plan]);
    f.ok(&["complete", &inside]);
    completed(&inside);
    assert!(
        f.ok(&["show", &inside])
            .contains(&format!("Parent: {plan}"))
    );
    group_rejected_by_its_dependency(&plan);

    // 3. A dependency added after work began blocks new starts below the Group, not the
    //    completion of started Issues or of child Groups whose children have all ended.
    let dep = f.accepted("Prerequisite 3");
    let plan = f.ok(&[
        "capture", "--label", "chore", "--kind", "group", "--accept", "--title", "Plan 3",
    ]);
    let plan = created(&plan).to_owned();
    let work = child("Work", &plan, "issue");
    let sub = child("Sub", &plan, "group");
    let done = child("Done", &sub, "issue");
    f.ok(&["start", &work]);
    f.ok(&["start", &done]);
    f.ok(&["complete", &done]);
    f.ok(&["dep", "add", &plan, "--needs", &dep]);
    let next = child("Next", &plan, "issue");
    let rejected = failure(f.run(&["start", &next]));
    assert!(
        rejected.contains(&format!(
            "dependencies of ancestor {plan} must be Completed"
        )),
        "{rejected}"
    );
    f.ok(&["complete", &work]);
    f.ok(&["complete", &sub]);
    completed(&work);
    completed(&sub);
    f.ok(&["cancel", &next]);
    group_rejected_by_its_dependency(&plan);

    // 4. Reopening the Group and then its dependency leaves the Completed work below the
    //    Group as it is; so does reopening only the dependency of a Group never completed.
    let dep = f.accepted("Prerequisite 4");
    let plan = group_needing("Plan 4", &dep);
    let work = child("Work 4", &plan, "issue");
    f.ok(&["start", &dep]);
    f.ok(&["complete", &dep]);
    f.ok(&["start", &work]);
    f.ok(&["complete", &work]);
    f.ok(&["complete", &plan]);
    completed(&plan);
    f.ok(&["reopen", &plan]);
    f.ok(&["reopen", &dep]);
    assert!(!own_row(&dep).contains("  Completed  "));
    completed(&work);
    group_rejected_by_its_dependency(&plan);

    let dep = f.accepted("Prerequisite 4b");
    let plan = group_needing("Plan 4b", &dep);
    let work = child("Work 4b", &plan, "issue");
    f.ok(&["start", &dep]);
    f.ok(&["complete", &dep]);
    f.ok(&["start", &work]);
    f.ok(&["complete", &work]);
    f.ok(&["reopen", &dep]);
    completed(&work);
    group_rejected_by_its_dependency(&plan);
}

#[test]
fn list_and_skip_conditions_ignore_conditions_while_tasks_and_show_evaluate_them() {
    let f = Fixture::new();
    f.ok(&["init", "t"]);
    let group = f.ok(&[
        "capture", "--label", "chore", "--kind", "group", "--accept", "--title", "Plan",
    ]);
    let group = created(&group).to_owned();
    let sub = f.ok(&[
        "capture",
        "--label",
        "chore",
        "--kind",
        "group",
        "--accept",
        "--title",
        "Sub",
        "--parent",
        &group,
        "--command",
        "exit 1",
    ]);
    let sub = created(&sub).to_owned();
    let issue = f.ok(&[
        "capture", "--label", "chore", "--accept", "--title", "Work", "--parent", &sub,
    ]);
    let issue = created(&issue).to_owned();
    let empty = f.ok(&[
        "capture", "--label", "chore", "--kind", "group", "--accept", "--title", "Empty",
    ]);
    let empty = created(&empty).to_owned();
    let confirmable = f.ok(&[
        "capture", "--label", "chore", "--kind", "group", "--accept", "--title", "Done",
    ]);
    let confirmable = created(&confirmable).to_owned();
    let finished = f.ok(&[
        "capture",
        "--label",
        "chore",
        "--accept",
        "--title",
        "Finished",
        "--parent",
        &confirmable,
    ]);
    let finished = created(&finished).to_owned();
    f.ok(&["start", &finished]);
    f.ok(&["complete", &finished]);
    // The Issue is startable but hidden by the intermediate Group's condition.
    let tasks = f.ok(&["tasks"]);
    assert!(
        tasks.contains(&format!("{group}  Group  Blocked  chore  Plan")),
        "{tasks}"
    );
    assert!(!tasks.contains(&sub) && !tasks.contains(&issue), "{tasks}");
    // show evaluates like tasks: the Group is Blocked by an unsurfaced candidate, the
    // intermediate Group by its own condition, and the Issue names the unsurfaced ancestor.
    let show = f.ok(&["show", &group]);
    assert!(
        show.contains(&format!("{group}  Group  Blocked  chore  Plan")),
        "{show}"
    );
    let stalled = show.split("Stalled\n").nth(1).unwrap();
    assert!(
        stalled.contains(&format!("Open subgroup: {sub}  Group  Blocked  chore  Sub")),
        "{show}"
    );
    assert!(
        stalled.contains(&format!(
            "Unsurfaced candidate: {issue}  Issue  Unsurfaced  chore  Work"
        )),
        "{show}"
    );
    assert!(!show.contains("Unsurfaced ancestor"), "{show}");
    assert!(
        show.contains(&format!("└── {issue}  Issue  Unsurfaced  chore  Work")),
        "{show}"
    );
    // The intermediate Group names its own unsatisfied condition as itself, after the
    // candidates it hides, and no ancestor.
    let show_sub = f.ok(&["show", &sub]);
    assert!(
        show_sub.contains(&format!(
            "Stalled\nUnsurfaced candidate: {issue}  Issue  Unsurfaced  chore  Work\nOwn condition unsatisfied: {sub}  Group  Blocked  chore  Sub\n"
        )) && !show_sub.contains("Unsurfaced ancestor"),
        "{show_sub}"
    );
    assert!(
        !f.ok(&["show", &sub, "--skip-conditions"])
            .contains("Own condition"),
        "{show_sub}"
    );
    // With only an Undecided child, the own condition is the one reason that explains why
    // adopting the child would not surface the Group.
    let quiet = f.ok(&[
        "capture",
        "--label",
        "chore",
        "--kind",
        "group",
        "--accept",
        "--title",
        "Quiet",
        "--command",
        "exit 1",
    ]);
    let quiet = created(&quiet).to_owned();
    let idea = f.ok(&[
        "capture", "--label", "chore", "--title", "Idea", "--parent", &quiet,
    ]);
    let idea = created(&idea).to_owned();
    let show_quiet = f.ok(&["show", &quiet]);
    assert!(
        show_quiet.contains(&format!(
            "Stalled\nUndecided child: {idea}  Issue  Undecided  chore  Idea\nOwn condition unsatisfied: {quiet}  Group  Blocked  chore  Quiet\n"
        )),
        "{show_quiet}"
    );
    assert!(
        !f.ok(&["show", &quiet, "--skip-conditions"])
            .contains("Own condition"),
        "{show_quiet}"
    );
    let show_issue = f.ok(&["show", &issue]);
    assert!(
        show_issue.starts_with(&format!("{issue}  Issue  Unsurfaced  chore  Work\n")),
        "{show_issue}"
    );
    assert!(
        show_issue.contains(&format!(
            "Required to start\nUnsurfaced ancestor: {sub}  Group  Blocked  chore  Sub\n"
        )),
        "{show_issue}"
    );
    // The named ancestor replaces the parent line.
    assert!(!show_issue.contains("Parent:"), "{show_issue}");
    // A Group below an unsurfaced ancestor names it last under Stalled, after its own
    // unsurfaced candidates, and the named parent replaces the parent line.
    let root = f.ok(&[
        "capture",
        "--label",
        "chore",
        "--kind",
        "group",
        "--accept",
        "--title",
        "Root",
        "--command",
        "exit 1",
    ]);
    let root = created(&root).to_owned();
    let nested = f.ok(&[
        "capture", "--label", "chore", "--kind", "group", "--accept", "--title", "Nested",
        "--parent", &root,
    ]);
    let nested = created(&nested).to_owned();
    let leaf = f.ok(&[
        "capture", "--label", "chore", "--accept", "--title", "Leaf", "--parent", &nested,
    ]);
    let leaf = created(&leaf).to_owned();
    let show_nested = f.ok(&["show", &nested]);
    assert!(
        show_nested.contains(&format!(
            "Stalled\nUnsurfaced candidate: {leaf}  Issue  Unsurfaced  chore  Leaf\nUnsurfaced ancestor: {root}  Group  Blocked  chore  Root\n"
        )) && !show_nested.contains("Own condition"),
        "{show_nested}"
    );
    assert!(!show_nested.contains("Parent:"), "{show_nested}");
    let skipped = f.ok(&["show", &nested, "--skip-conditions"]);
    assert!(
        skipped.contains(&format!(
            "{nested}  Group  Ready  chore  Nested\nParent: {root}"
        )) && !skipped.contains("Unsurfaced"),
        "{skipped}"
    );
    // A Group with its own unsatisfied condition below an unsurfaced ancestor names only the
    // ancestor: its own condition is not evaluated.
    let guarded = f.ok(&[
        "capture",
        "--label",
        "chore",
        "--kind",
        "group",
        "--accept",
        "--title",
        "Guarded",
        "--parent",
        &root,
        "--command",
        "exit 1",
    ]);
    let guarded = created(&guarded).to_owned();
    f.ok(&[
        "capture",
        "--label",
        "chore",
        "--accept",
        "--title",
        "Guarded work",
        "--parent",
        &guarded,
    ]);
    let show_guarded = f.ok(&["show", &guarded]);
    assert!(
        show_guarded.contains(&format!("Unsurfaced ancestor: {root}"))
            && !show_guarded.contains("Own condition"),
        "{show_guarded}"
    );
    // A Group that is not stalled shows no reason for its own condition: an Empty one that
    // can complete, and one whose descendant is InProgress.
    let hollow = f.ok(&[
        "capture",
        "--label",
        "chore",
        "--kind",
        "group",
        "--accept",
        "--title",
        "Hollow",
        "--command",
        "exit 1",
    ]);
    let hollow = created(&hollow).to_owned();
    let show_hollow = f.ok(&["show", &hollow]);
    assert!(
        show_hollow.starts_with(&format!("{hollow}  Group  Empty  chore  Hollow\n"))
            && !show_hollow.contains("Stalled"),
        "{show_hollow}"
    );
    let paused = f.ok(&[
        "capture",
        "--label",
        "chore",
        "--kind",
        "group",
        "--accept",
        "--title",
        "Paused",
        "--command",
        "exit 1",
    ]);
    let paused = created(&paused).to_owned();
    let running = f.ok(&[
        "capture", "--label", "chore", "--accept", "--title", "Running", "--parent", &paused,
    ]);
    let running = created(&running).to_owned();
    f.ok(&["start", &running]);
    assert!(!f.ok(&["show", &paused]).contains("Stalled"));
    // Once the started work is done and the rest waits for a dependency, the Group is
    // effectively InProgress, stalled, and names its own condition as itself.
    f.ok(&["complete", &running]);
    let gate = f.accepted("Gate");
    let held = f.ok(&[
        "capture", "--label", "chore", "--accept", "--title", "Held", "--parent", &paused,
        "--needs", &gate,
    ]);
    let held = created(&held).to_owned();
    let show_paused = f.ok(&["show", &paused]);
    assert!(
        show_paused.starts_with(&format!("{paused}  Group  InProgress  chore  Paused\n"))
            && show_paused.contains(&format!(
                "Descendant dependency must complete: {held}  Issue  Unsurfaced  chore  Held  needs  {gate}"
            ))
            && show_paused.contains(&format!(
                "Own condition unsatisfied: {paused}  Group  InProgress  chore  Paused\n"
            )),
        "{show_paused}"
    );
    for output in [
        f.ok(&["list"]),
        f.ok(&["show", &group, "--skip-conditions"]),
    ] {
        assert!(
            output.contains(&format!("{group}  Group  Ready  chore  Plan")),
            "{output}"
        );
        assert!(
            !output.contains("Stalled") && !output.contains("Unsurfaced"),
            "{output}"
        );
    }
    assert!(
        f.ok(&["show", &issue, "--skip-conditions"])
            .starts_with(&format!("{issue}  Issue  Ready  chore  Work\n"))
    );
    let list = f.ok(&["list"]);
    for line in [
        format!("{sub}  Group  Ready  chore  Sub"),
        format!("{issue}  Issue  Ready  chore  Work"),
        format!("{empty}  Group  Empty  chore  Empty"),
        format!("{confirmable}  Group  Confirmable  chore  Done"),
        format!("{finished}  Issue  Completed  chore  Finished"),
    ] {
        assert!(list.contains(&line), "{line}\n{list}");
    }
    assert!(
        f.ok(&["list", "--kind", "group"])
            .contains(&format!("{group}  Group  Ready  chore  Plan"))
    );
    f.ok(&["condition", "unset", &sub]);
    for output in [f.ok(&["tasks"]), f.ok(&["show", &group])] {
        assert!(
            output.contains(&format!("{group}  Group  Ready  chore  Plan")),
            "{output}"
        );
    }
}

#[test]
fn show_fails_on_evaluation_failure_and_names_skip_conditions() {
    let f = Fixture::new();
    f.ok(&["init", "t"]);
    let group = f.ok(&[
        "capture", "--label", "chore", "--kind", "group", "--accept", "--title", "Plan",
    ]);
    let group = created(&group).to_owned();
    let issue = f.ok(&[
        "capture",
        "--label",
        "chore",
        "--accept",
        "--title",
        "Work",
        "--parent",
        &group,
        "--command",
        "exit 3",
    ]);
    let issue = created(&issue).to_owned();
    let help = f.ok(&["help", "show"]);
    assert!(help.contains("--skip-conditions") && help.contains("--trace-conditions"));
    // The failure names the Entity and the way to read saved information.
    let failed = failure(f.run(&["show", &group]));
    assert!(
        failed.contains(&issue) && failed.contains("exit status: 3"),
        "{failed}"
    );
    assert!(
        failed.contains(&format!("axon show {group} --skip-conditions")),
        "{failed}"
    );
    let failed = failure(f.run(&["show", &issue, "--details"]));
    assert!(failed.contains("exit status: 3"), "{failed}");
    assert!(
        failed.contains(&format!("axon show {issue} --details --skip-conditions")),
        "{failed}"
    );
    // A Group without startable descendants still runs its own condition, as tasks does.
    f.ok(&["condition", "set", &issue, "--command", "exit 1"]);
    let hollow = f.ok(&[
        "capture",
        "--label",
        "chore",
        "--kind",
        "group",
        "--accept",
        "--title",
        "Hollow",
        "--command",
        "exit 3",
    ]);
    let hollow = created(&hollow).to_owned();
    f.ok(&[
        "capture", "--label", "chore", "--title", "Idea", "--parent", &hollow,
    ]);
    let failed = failure(f.run(&["show", &hollow]));
    assert!(
        failed.contains(&hollow) && failed.contains("exit status: 3"),
        "{failed}"
    );
    assert!(failure(f.run(&["tasks"])).contains(&hollow));
    // The timeout applies to show as it does to tasks.
    f.ok(&["condition", "set", &hollow, "--command", "sleep 5"]);
    let started = std::time::Instant::now();
    let failed = failure(f.run(&["show", &hollow, "--condition-timeout", "200ms"]));
    assert!(started.elapsed() < std::time::Duration::from_secs(4));
    assert!(
        failed.contains("timed out after 200ms") && failed.contains("--skip-conditions"),
        "{failed}"
    );
    f.ok(&["condition", "unset", &hollow]);
    f.ok(&["condition", "set", &issue, "--command", "exit 3"]);
    assert!(
        f.ok(&["show", &group, "--skip-conditions"])
            .contains(&format!("{group}  Group  Ready  chore  Plan"))
    );
    assert_eq!(
        f.run(&["show", &group, "--skip-conditions", "--trace-conditions"])
            .status
            .code(),
        Some(2)
    );
    // A timeout is accepted, and has nothing to apply to, when conditions are skipped.
    f.ok(&[
        "show",
        &group,
        "--skip-conditions",
        "--condition-timeout",
        "1s",
    ]);
    // Undecided targets evaluate nothing, even with a failing condition.
    f.ok(&["withdraw", &issue]);
    assert!(
        f.ok(&["show", &issue])
            .starts_with(&format!("{issue}  Issue  Undecided  chore  Work\n"))
    );
    f.ok(&["accept", &issue]);
    f.ok(&["condition", "set", &issue, "--command", "echo seen; exit 1"]);
    let traced = f.run(&[
        "show",
        &group,
        "--trace-conditions",
        "--condition-timeout",
        "30s",
    ]);
    assert!(traced.status.success());
    let trace = String::from_utf8(traced.stderr).unwrap();
    assert!(
        trace.contains(&format!("Condition trace: {issue}")) && trace.contains("seen"),
        "{trace}"
    );
    assert!(String::from_utf8(traced.stdout).unwrap().contains(&format!(
        "Unsurfaced candidate: {issue}  Issue  Unsurfaced  chore  Work"
    )));
}

#[test]
fn a_label_is_required_at_registration_and_invalid_values_leave_the_store_unchanged() {
    let f = Fixture::new();
    f.init();
    let id = f.accepted("Work");
    let before = snapshot(&f);
    for args in [
        vec!["capture", "--title", "Unlabeled"],
        vec!["capture", "--label", "fix", "--title", "Outside"],
        vec!["capture", "--label", "", "--title", "Empty"],
        vec!["capture", "--label", "Bug", "--title", "Case"],
        vec!["label", "set", &id, "fix"],
        vec!["label", "set", &id],
        vec!["list", "--label", "fix"],
        vec!["tasks", "--label", "fix"],
        vec!["proposals", "--label", "fix"],
    ] {
        let output = f.run(&args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
        assert!(!output.stderr.is_empty(), "{args:?}");
        assert_eq!(snapshot(&f), before, "{args:?}");
    }
    let missing = String::from_utf8(f.run(&["capture", "--title", "x"]).stderr).unwrap();
    assert!(missing.contains("--label <LABEL>"), "{missing}");
    let outside = String::from_utf8(f.run(&["label", "set", &id, "fix"]).stderr).unwrap();
    assert!(
        outside.contains("bug, feat, chore, docs, test, refactor, spike"),
        "{outside}"
    );
}

#[test]
fn label_set_rows_filters_and_log_show_the_current_label() {
    let f = Fixture::new();
    f.init();
    let bug =
        created(&f.ok(&["capture", "--accept", "--label", "bug", "--title", "Crash"])).to_owned();
    let plan = created(&f.ok(&[
        "capture", "--kind", "group", "--label", "feat", "--title", "Plan",
    ]))
    .to_owned();
    let question =
        created(&f.ok(&["capture", "--label", "spike", "--title", "Question"])).to_owned();
    assert_eq!(
        f.ok(&["list"]),
        format!(
            "{bug}  Issue  Ready  bug  Crash\n{plan}  Group  Undecided  feat  Plan\n{question}  Issue  Undecided  spike  Question\n"
        )
    );
    assert!(
        f.ok(&["show", &bug])
            .starts_with(&format!("{bug}  Issue  Ready  bug  Crash\n"))
    );
    // --label combines with every other filter by AND.
    assert_eq!(
        f.ok(&["list", "--label", "bug"]),
        format!("{bug}  Issue  Ready  bug  Crash\n")
    );
    assert_eq!(f.ok(&["list", "--label", "bug", "--kind", "group"]), "");
    assert_eq!(
        f.ok(&["list", "--label", "spike", "--lifecycle", "undecided"]),
        format!("{question}  Issue  Undecided  spike  Question\n")
    );
    assert_eq!(f.ok(&["list", "--label", "spike", "--search", "Crash"]), "");
    assert_eq!(f.ok(&["list", "--label", "docs"]), "");
    assert_eq!(
        f.ok(&["tasks", "--label", "bug"]),
        format!("{bug}  Issue  Ready  bug  Crash\n")
    );
    assert_eq!(f.ok(&["tasks", "--label", "spike"]), "");
    assert_eq!(
        f.ok(&["proposals", "--label", "feat"]),
        format!("{plan}  Group  Undecided  feat  Plan\n")
    );
    assert_eq!(
        f.ok(&["proposals", "--label", "feat", "--kind", "issue"]),
        ""
    );
    assert_eq!(
        f.ok(&["proposals", "--label", "spike", "--search", "Quest"]),
        format!("{question}  Issue  Undecided  spike  Question\n  Matched: Title\n")
    );

    assert_eq!(
        f.ok(&["label", "set", &bug, "chore"]),
        format!("{bug}  Label updated: chore\n")
    );
    let before = snapshot(&f);
    assert_eq!(
        f.ok(&["label", "set", &bug, "chore"]),
        format!("{bug}  No changes  Label: chore\n")
    );
    assert_eq!(snapshot(&f), before);
    assert_eq!(
        f.ok(&["list", "--label", "chore"]),
        format!("{bug}  Issue  Ready  chore  Crash\n")
    );
    assert_eq!(f.ok(&["list", "--label", "bug"]), "");
    let log = f.ok(&["log", &bug]);
    assert!(
        log.lines().nth(1).unwrap().ends_with("  Label set: chore"),
        "{log}"
    );

    // Converting the kind keeps the label.
    f.ok(&["convert", &plan, "--kind", "issue"]);
    assert!(
        f.ok(&["list", "--label", "feat"])
            .starts_with(&format!("{plan}  Issue  Undecided  feat  Plan\n"))
    );

    // A declaration that changes the label names it among the changed fields.
    let records = f.records();
    let current = f.current(&question);
    let record = records
        .import(
            &eid(&question),
            record::Imported {
                title: current.title,
                description: current.description,
                label: axon::lifecycle::Label::Docs,
                parent: current.parent,
                needs: current.needs,
            },
            context(),
        )
        .unwrap()
        .unwrap();
    f.publish(vec![Entry::Record(record)]);
    let log = f.ok(&["log", &question]);
    assert!(log.ends_with("  Declaration applied: label\n"), "{log}");

    // The label of terminal work is fixed, even to the same value.
    f.ok(&["start", &bug]);
    f.ok(&["complete", &bug]);
    let before = snapshot(&f);
    for value in ["chore", "bug"] {
        let error = failure(f.run(&["label", "set", &bug, value]));
        assert!(
            error.contains("label set: terminal label is fixed"),
            "{error}"
        );
        assert_eq!(snapshot(&f), before);
    }
}

#[test]
fn label_help_lists_every_value_with_its_meaning() {
    let f = Fixture::new();
    for args in [
        vec!["help", "label"],
        vec!["help", "label", "set"],
        vec!["help", "capture"],
        vec!["help", "list"],
        vec!["help", "tasks"],
    ] {
        let help = f.ok(&args);
        for value in ["bug", "feat", "chore", "docs", "test", "refactor", "spike"] {
            assert!(help.contains(value), "{args:?}: {value}");
        }
        assert!(
            help.contains("kind of work") || help.contains("Fix behavior"),
            "{args:?}"
        );
    }
    for args in [
        vec!["help", "label", "set"],
        vec!["help", "capture"],
        vec!["help", "list"],
    ] {
        let help = f.ok(&args);
        assert!(
            help.contains("bug:      Fix behavior that differs from what is expected"),
            "{args:?}\n{help}"
        );
        assert!(
            help.contains("spike:    Investigate in order to decide"),
            "{args:?}"
        );
    }
    let docs = f.ok(&["docs"]);
    assert!(docs.contains("Labels\n"), "{docs}");
    assert!(docs.contains("refactor  reorganize structure without changing behavior"));
    assert!(
        f.ok(&["help", "convert"])
            .contains("text, label, condition")
    );
}

#[test]
fn heads_that_differ_only_by_label_show_their_own_label_and_resolve_takes_one() {
    let f = Fixture::new();
    f.init();
    f.publish(vec![registration(&f.records(), "t-item")]);
    let left = Fixture::new();
    let right = Fixture::new();
    for side in [&left, &right] {
        fs::create_dir(side.0.join(".axon")).unwrap();
        fs::copy(f.header(), side.header()).unwrap();
        merge_records(&f.0, &side.0);
    }
    left.ok(&["label", "set", "t-item", "bug"]);
    right.ok(&["label", "set", "t-item", "docs"]);
    merge_records(&right.0, &left.0);
    let listing = left.ok(&["resolve", "t-item"]);
    let head = |label: &str| {
        let line = listing
            .lines()
            .find(|line| line.ends_with(&format!("  label  NotStarted  Issue  {label}  t-item")))
            .unwrap_or_else(|| panic!("no {label} head in:\n{listing}"));
        line.split_whitespace().next().unwrap().to_owned()
    };
    let (bug, docs) = (head("bug"), head("docs"));
    // The row of a conflicted Entity shows the first head's label in record ID order.
    let first = if bug < docs { "bug" } else { "docs" };
    assert!(
        listing.starts_with(&format!("t-item  Issue  Conflicted  {first}  t-item\n")),
        "{listing}"
    );
    left.ok(&["resolve", "t-item", "--head", &docs, "-r", "keep docs"]);
    let log = left.ok(&["log", "t-item"]);
    assert!(
        log.ends_with(&format!(
            "Resolved: {docs}  NotStarted  Issue  docs  Reason: keep docs\n"
        )),
        "{log}"
    );
    assert!(
        left.ok(&["list", "--label", "docs"])
            .starts_with("t-item  Issue  Ready  docs  t-item\n")
    );
    assert_eq!(left.ok(&["list", "--label", "bug"]), "");
}
