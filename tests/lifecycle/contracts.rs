use super::*;

fn created(text: &str) -> &str {
    text.split_whitespace().next().unwrap()
}
fn suffix(id: &str) -> &str {
    id.rsplit('-').next().unwrap()
}
fn snapshot(f: &Fixture) -> Snapshot {
    axon::location::Location::discover(&f.0, false)
        .unwrap()
        .open()
        .unwrap()
        .read()
        .unwrap()
        .1
}
fn seed(f: &Fixture, id: &str) {
    axon::location::Location::discover(&f.0, false)
        .unwrap()
        .open()
        .unwrap()
        .update(|_, s| {
            s.create(eid(id), Kind::Issue, current(id), context())?;
            Ok(())
        })
        .unwrap();
}

#[test]
fn short_ids_and_suffixes_work_across_mutations_and_preserve_long_ids() {
    for backend in ["sqlite", "file"] {
        let f = Fixture::new();
        f.ok(&["init", "project", "--backend", backend]);
        let group = f.ok(&["group", "plan", "--title", "Parent"]);
        let group = created(&group);
        let dep = f.plan("Prerequisite");
        for id in [group, &dep] {
            assert!(id.starts_with("project-"));
            assert_eq!(suffix(id).len(), 6);
            assert!(
                suffix(id)
                    .bytes()
                    .all(|b| b"0123456789abcdefghjkmnpqrstvwxyz".contains(&b))
            );
        }
        let issue = f.ok(&[
            "capture",
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
        f.ok(&["start", suffix(group)]);
        f.ok(&["start", suffix(&dep)]);
        f.ok(&["done", suffix(&dep)]);
        f.ok(&[
            "write",
            short,
            "--title",
            "Updated work",
            "--description",
            "Updated body",
        ]);
        f.ok(&["group", "unset", short]);
        f.ok(&["group", "set", short, "--parent", suffix(group)]);
        f.ok(&["dep", "rm", short, "--needs", suffix(&dep)]);
        f.ok(&["dep", "add", short, "--needs", suffix(&dep)]);
        f.ok(&["when", "clear", short]);
        f.ok(&[
            "when",
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
        f.ok(&["done", short]);
        assert!(f.ok(&["log", short]).contains("InProgress → Completed"));
        assert!(!f.0.join("observed").exists());
        let long = "project-454e0188d65fbdee8090f4c245831b2a";
        seed(&f, long);
        assert!(f.ok(&["show", "5831b2a"]).starts_with(long));
        f.ok(&["start", "5831b2a"]);
        assert!(snapshot(&f).entity(&eid(long)).is_ok());
        seed(&f, "project-0000zz");
        seed(&f, "project-1111zz");
        let before = snapshot(&f);
        let error = failure(f.run(&["start", "zz"]));
        assert!(error.starts_with("Error: zz start:"));
        assert!(
            error.contains("ambiguous")
                && error.contains("project-0000zz")
                && error.contains("project-1111zz")
        );
        assert_eq!(snapshot(&f), before);
    }
}

#[test]
fn filters_search_all_notes_and_do_not_evaluate_excluded_candidates() {
    for backend in ["sqlite", "file"] {
        let f = Fixture::new();
        f.ok(&["init", "q", "--backend", backend]);
        let parent = f.ok(&[
            "group",
            "plan",
            "--title",
            "Parent",
            "--command",
            "echo parent >> observed",
        ]);
        let parent = created(&parent);
        let a = f.ok(&[
            "plan",
            "--title",
            "Needle %_",
            "--parent",
            parent,
            "-m",
            "Needle %_ body",
        ]);
        let a = created(&a);
        let b = f.plan("Other");
        let n = f.ok(&["note", "add", &b, "-m", "Needle %_ historical"]);
        let note = n.split_whitespace().nth(2).unwrap();
        f.ok(&["write", &b, "-m", "current text without query"]);
        let unrelated = f.ok(&[
            "group",
            "plan",
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
        assert!(rows.contains(a) && rows.contains(&b));
        assert!(!rows.contains(parent) && !rows.contains(unrelated));
        assert!(
            rows.contains("Matched: Title, Description") && rows.contains(&format!("Note {note}"))
        );
        assert!(!f.0.join("observed").exists());
        let candidates = f.ok(&["tasks", "--kind", "issue", "--search", "Needle %_"]);
        assert!(candidates.contains(a) && candidates.contains(&b));
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
}

#[test]
fn details_show_saved_relationships_once_without_running_conditions() {
    let f = Fixture::new();
    f.init();
    let parent = f.ok(&["group", "plan", "--title", "Plan"]);
    let parent = created(&parent);
    let dep = f.plan("Dependency");
    f.ok(&["start", &dep]);
    f.ok(&["done", &dep]);
    let a = f.ok(&[
        "plan",
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
    let dependent = f.ok(&["plan", "--title", "Consumer", "--needs", a]);
    let dependent = created(&dependent);
    let plain = f.ok(&["show", a]);
    assert!(
        plain.contains("Required to start") && !plain.contains(&dep) && !plain.contains(dependent)
    );
    let details = f.ok(&["show", a, "--details"]);
    for id in [a, parent, &dep, dependent] {
        assert_eq!(details.matches(id).count(), 1, "{details}");
    }
    assert!(!details.contains("Required to start"));
    assert!(
        details.contains("Lifecycle: NotStarted")
            && details.contains("Condition: echo wrong > observed")
    );
    assert!(!f.0.join("observed").exists());
}

#[test]
fn no_op_confirmation_uses_locked_state_and_keeps_snapshot_and_bytes() {
    for backend in ["sqlite", "file"] {
        let f = Fixture::new();
        f.ok(&["init", "n", "--backend", backend]);
        let a = f.plan("Text");
        let b = f.plan("Needs");
        f.ok(&["dep", "add", &a, "--needs", &b]);
        let before = snapshot(&f);
        let path = if backend == "file" {
            f.0.join(".axon/state.jsonl")
        } else {
            f.db()
        };
        let bytes = fs::read(&path).unwrap();
        for args in [
            vec!["write", &a, "--title", "Text"],
            vec!["group", "unset", &a],
            vec!["dep", "add", &a, "--needs", &b],
            vec!["when", "clear", &a],
        ] {
            let out = f.ok(&args);
            assert!(out.starts_with(&a) && out.contains("No changes"));
            assert_eq!(snapshot(&f), before);
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
        assert!(
            f.ok(&["write", &a, "--title", "Text", "-m", "new description"])
                .contains("Description updated")
        );
        assert!(
            !f.ok(&["write", &a, "--title", "Text", "-m", ""])
                .contains("Title updated")
        );
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
    for heading in [
        "Workflow:",
        "Inspect:",
        "Plan management:",
        "Setup & utilities:",
    ] {
        assert!(help.contains(heading));
    }
    assert!(f.ok(&["docs"]).contains("Cancelled is terminal"));
    assert!(f.ok(&["help", "note", "show"]).contains("<NOTE_ID>"));
    assert!(!f.ok(&["actor"]).is_empty());
    let version = f.ok(&["--version"]);
    assert_eq!(f.ok(&["-V"]), version);
    assert!(
        version.contains("commit") && version.contains("source"),
        "{version}"
    );
    for shell in ["bash", "zsh", "fish", "powershell", "elvish"] {
        let script = f.ok(&["completion", shell]);
        assert!(!script.contains('\x1b'));
        assert!(script.contains("tasks"));
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
        assert_eq!(
            f.run(&["tasks", &format!("--condition-timeout={value}")])
                .status
                .code(),
            Some(2)
        );
    }
    assert!(!f.0.join(".axon").exists());
    for args in [
        vec!["plan", "old positional title"],
        vec!["capture", "--title", "Title", "--message", "old body"],
        vec!["plan", "--title", "Title", "--file", "old.md"],
    ] {
        assert_eq!(f.run(&args).status.code(), Some(2));
    }
    fs::remove_file(f.0.join(".git")).unwrap();
    f.init();
    f.plan("work");
    for value in ["500ms", "30s", "2m", "1h"] {
        f.ok(&["tasks", "--condition-timeout", value]);
    }
}

#[test]
fn default_prefix_preserves_management_root_names_including_unicode() {
    let f = Fixture::new();
    let directory = f.0.join("日本語 project");
    fs::create_dir(&directory).unwrap();
    success(
        command(&directory)
            .args(["init", "--backend", "file"])
            .output()
            .unwrap(),
    );
    let output = success(
        command(&directory)
            .args(["plan", "--title", "Work"])
            .output()
            .unwrap(),
    );
    assert!(output.starts_with("日本語 project-"));
    let (prefix, saved) =
        axon::file::decode(&fs::read(directory.join(".axon/state.jsonl")).unwrap()).unwrap();
    assert_eq!(prefix, "日本語 project");
    let id = saved.entities().next().unwrap().id.to_string();
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
        .args(["plan", "--title", "Retained"])
        .stdout(Stdio::from(writer))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let error = String::from_utf8_lossy(&out.stderr);
    assert!(error.contains("Applied:") && error.contains("storage applied; output failed"));
    assert!(f.ok(&["list"]).contains("Retained"));
}

#[cfg(unix)]
fn terminal_output(f: &Fixture, args: &[&str], no_color: bool) -> String {
    use std::io::Read;
    use std::os::fd::FromRawFd;
    let (mut master, mut slave) = (0, 0);
    assert_eq!(
        unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
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
    let id = f.plan("USER_TITLE");
    for args in [vec!["show", &id], vec!["--help"], vec!["done", "--help"]] {
        let plain = f.ok(&args);
        let colored = terminal_output(&f, &args, false);
        assert!(colored.contains('\x1b'), "{colored}");
        assert_eq!(strip_sgr(&colored), plain);
        assert_eq!(terminal_output(&f, &args, true), plain);
        if args[0] == "show" {
            assert!(colored.contains("  USER_TITLE\n"));
        }
    }
}
