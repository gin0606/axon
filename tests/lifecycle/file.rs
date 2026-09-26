//! The record directory under real processes and real Git: concurrency, corruption, the index
//! guard, tracked worktrees merging record files, and what `axon storage check` reports.
use super::*;

/// The record files of an Entity's records, by the `entity` key of their JSON.
fn record_paths_of(root: &Path, entity: &str) -> Vec<PathBuf> {
    record_files(root)
        .into_iter()
        .filter(|relative| {
            let bytes = fs::read(root.join(".axon/records").join(relative)).unwrap();
            let row: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            row["entity"] == entity
        })
        .collect()
}
/// A copy of the store in another fixture, as a checkout of the same commit would leave it.
fn copy_store(from: &Fixture) -> Fixture {
    let copy = Fixture::new();
    fs::create_dir(copy.0.join(".axon")).unwrap();
    fs::copy(from.header(), copy.header()).unwrap();
    merge_records(&from.0, &copy.0);
    copy
}

#[test]
fn file_cli_roundtrip_and_atomic_concurrency() {
    let f = Fixture::new();
    f.init();
    let group = f
        .ok(&["capture", "--kind", "group", "--accept", "--title", "group"])
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    let issue = f
        .ok(&[
            "capture", "--accept", "--title", "child", "--parent", &group,
        ])
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    failure(f.run(&["start", &group]));
    let mut processes = (0..6)
        .map(|_| f.command().args(["start", &issue]).spawn().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        processes
            .iter_mut()
            .filter_map(|p| p.wait().ok())
            .filter(|s| s.success())
            .count(),
        1
    );
    let mut processes = (0..8)
        .map(|i| {
            f.command()
                .args(["note", "add", &issue, "-m", &format!("note {i}")])
                .spawn()
                .unwrap()
        })
        .collect::<Vec<_>>();
    for p in &mut processes {
        assert!(p.wait().unwrap().success());
    }
    assert_eq!(f.records().notes_of(&eid(&issue)).len(), 8);
    f.ok(&["release", &issue]);
    f.ok(&["start", &issue]);
    f.ok(&["complete", &issue]);
    assert!(
        f.ok(&["show", &group])
            .contains("Awaiting final confirmation")
    );
    f.ok(&["complete", &group]);
    assert!(f.ok(&["log", &issue]).contains("Completed"));
    assert!(f.ok(&["tasks"]).is_empty());
    // Every operation added exactly one file, named by the hash of its content.
    let files = f.record_files();
    assert_eq!(files.len(), 2 + 1 + 8 + 3 + 1);
    for relative in &files {
        let name = relative.file_name().unwrap().to_str().unwrap();
        assert_eq!(name.len(), 64);
        assert_eq!(relative.parent().unwrap().to_str().unwrap(), &name[..2]);
        let bytes = fs::read(f.records_dir().join(relative)).unwrap();
        assert_eq!(blake3::hash(&bytes).to_hex().as_str(), name);
    }
    failure(f.run(&["start", &issue]));
    assert_eq!(f.record_files(), files);
    let check = f.run(&["storage", "check"]);
    assert!(check.status.success() && check.stdout.is_empty());
    assert!(String::from_utf8_lossy(&check.stderr).contains("consistent"));
}

#[test]
fn corrupt_record_files_stop_reads_and_writes_and_temporary_files_are_ignored() {
    let f = Fixture::new();
    f.init();
    let id = f.accepted("job");
    let group = f
        .ok(&["capture", "--kind", "group", "--accept", "--title", "G"])
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    // A conflict elsewhere, so that a corruption report can be seen to stop before it.
    let contested = f.accepted("contested");
    let other = copy_store(&f);
    f.ok(&["start", &contested]);
    other.ok(&["start", &contested]);
    merge_records(&other.0, &f.0);
    assert!(failure(f.run(&["storage", "check"])).contains("Conflicted:"));
    let files = record_paths_of(&f.0, &id);
    let record = f.records_dir().join(&files[0]);
    let good = fs::read(&record).unwrap();
    // A stale temporary file is invisible to every read and to the check.
    let temporary = record.with_file_name(format!(
        "{}.tmp",
        record.file_name().unwrap().to_str().unwrap()
    ));
    fs::write(&temporary, b"partial").unwrap();
    assert!(f.ok(&["list"]).contains(&id));
    let check = failure(f.run(&["storage", "check"]));
    assert!(
        check.contains("1 problems") && !check.contains("tmp"),
        "{check}"
    );
    fs::remove_file(&temporary).unwrap();
    // Content that does not hash to the file name, a truncated file, an empty file and a
    // file whose name is not a record ID stop every read and every write.
    let stray = f.records_dir().join("notes.txt");
    let forged = f.records_dir().join("00").join("0".repeat(64));
    fs::create_dir_all(forged.parent().unwrap()).unwrap();
    for (label, corrupt) in [
        ("hash", &good[..good.len() - 2].to_vec()),
        ("empty", &Vec::new()),
        (
            "edited",
            &String::from_utf8(good.clone())
                .unwrap()
                .replacen("\"title\":\"job\"", "\"title\":\"jobs\"", 1)
                .into_bytes(),
        ),
    ] {
        fs::write(&record, corrupt).unwrap();
        fs::write(&stray, b"kept").unwrap();
        fs::write(&forged, &good).unwrap();
        let before = f.record_files();
        for args in [
            vec!["list"],
            vec!["show", &id],
            vec!["start", &id],
            vec!["note", "add", &id, "-m", "rejected"],
            vec!["capture", "--title", "rejected"],
        ] {
            let error = failure(f.run(&args));
            assert!(error.contains("corrupt"), "{label} {args:?}: {error}");
            assert_eq!(f.record_files(), before, "{label} {args:?}");
        }
        let error = failure(f.run(&["storage", "check"]));
        assert!(error.contains("3 corrupt files"), "{label}: {error}");
        assert!(error.contains("notes.txt"), "{label}: {error}");
        assert!(
            error.contains(files[0].to_str().unwrap()),
            "{label}: {error}"
        );
        assert!(error.contains("0000000000"), "{label}: {error}");
        assert!(
            !error.contains("Conflicted:") && !error.contains("Violation:"),
            "{label}: corruption is reported alone, before any derivation: {error}"
        );
        assert_eq!(fs::read(&stray).unwrap(), b"kept");
    }
    fs::write(&record, &good).unwrap();
    fs::remove_file(&stray).unwrap();
    fs::remove_file(&forged).unwrap();
    assert!(f.ok(&["list"]).contains(&id));
    // The conflict is reported again once the corruption is gone.
    assert!(failure(f.run(&["storage", "check"])).contains("Conflicted:"));
    // A record file whose subdirectory does not match its name is corruption too.
    let misplaced = f
        .records_dir()
        .join("zz")
        .join(files[0].file_name().unwrap());
    fs::create_dir_all(misplaced.parent().unwrap()).unwrap();
    fs::write(&misplaced, &good).unwrap();
    let error = failure(f.run(&["storage", "check"]));
    assert!(
        error.contains("1 corrupt files") && error.contains("Corrupt: zz:"),
        "{error}"
    );
    fs::remove_dir_all(misplaced.parent().unwrap()).unwrap();
    // The check of an explicit root does not discover: a nested directory checks itself.
    let nested = f.0.join("nested");
    fs::create_dir(&nested).unwrap();
    let error = failure(f.run(&["storage", "check", nested.to_str().unwrap()]));
    assert!(
        error.contains("not initialized") && !error.contains(&id),
        "{error}"
    );
    let check = f.run(&["storage", "check", f.0.to_str().unwrap()]);
    assert!(!check.status.success());
    // A record that decodes on its own but does not continue its parent record (a Complete
    // forged onto a registration, from another Issue's Start) is corruption too, reported by
    // file. The Group registered above keeps the store otherwise readable.
    let job_created = record_paths_of(&f.0, &id);
    let contested_records = record_paths_of(&f.0, &contested);
    let start = contested_records
        .iter()
        .find(|relative| {
            fs::read_to_string(f.records_dir().join(relative))
                .unwrap()
                .contains("\"operation\":\"start\"")
        })
        .unwrap();
    let text = fs::read_to_string(f.records_dir().join(start)).unwrap();
    let owner_start = text.find("\"owner\":").unwrap() + "\"owner\":".len();
    let owner_end = owner_start + text[owner_start..].find(",\"title\"").unwrap();
    let forged = format!("{}null{}", &text[..owner_start], &text[owner_end..])
        .replace(
            &format!("\"entity\":\"{contested}\""),
            &format!("\"entity\":\"{id}\""),
        )
        .replace("\"operation\":\"start\"", "\"operation\":\"complete\"")
        .replace(
            "\"lifecycle\":\"in-progress\"",
            "\"lifecycle\":\"completed\"",
        )
        .replace("\"title\":\"contested\"", "\"title\":\"job\"");
    // Its parent becomes the registration of `job`, the only record that Issue has.
    let parents_start = forged.find("\"parents\":[\"").unwrap() + "\"parents\":[\"".len();
    let parents_end = parents_start + forged[parents_start..].find('"').unwrap();
    let forged = format!(
        "{}{}{}",
        &forged[..parents_start],
        file_name(&job_created[0]),
        &forged[parents_end..]
    );
    let forged_id = blake3::hash(forged.as_bytes()).to_hex().to_string();
    let forged_path = f.records_dir().join(&forged_id[..2]).join(&forged_id);
    fs::create_dir_all(forged_path.parent().unwrap()).unwrap();
    fs::write(&forged_path, &forged).unwrap();
    let error = failure(f.run(&["storage", "check"]));
    assert!(error.contains("1 corrupt files"), "{error}");
    assert!(
        error.contains(&format!(
            "Corrupt: {}/{forged_id}: cannot Complete from NotStarted",
            &forged_id[..2]
        )),
        "{error}"
    );
    assert!(failure(f.run(&["list"])).contains("corrupt"));
    fs::remove_file(&forged_path).unwrap();
    assert!(f.ok(&["list"]).contains(&group));
    assert!(f.ok(&["show", &id]).contains("Issue  Ready  job"));
}
fn file_name(relative: &Path) -> String {
    relative.file_name().unwrap().to_str().unwrap().to_string()
}

#[test]
fn a_store_without_a_records_directory_reads_as_empty_and_the_first_write_creates_it() {
    // Git does not track an empty directory, so a checkout of a store committed before its
    // first record has the header and the ignore file only.
    let f = Fixture::new();
    f.init();
    fs::remove_dir(f.records_dir()).unwrap();
    assert!(f.ok(&["list"]).is_empty());
    let check = f.run(&["storage", "check"]);
    assert!(check.status.success());
    let id = f.accepted("first");
    assert!(f.records_dir().is_dir());
    assert_eq!(f.record_files().len(), 1);
    assert!(f.ok(&["list"]).contains(&id));
    assert!(f.run(&["storage", "check"]).status.success());
}

#[test]
fn storage_check_treats_initialization_residue_as_uninitialized() {
    let f = Fixture::new();
    fs::create_dir_all(f.0.join(".axon/records")).unwrap();
    fs::write(f.0.join(".axon/.gitignore"), "*.lock\n*.tmp\n").unwrap();
    fs::write(f.0.join(".axon/header.json.tmp"), "partial").unwrap();
    for args in [
        vec!["storage", "check"],
        vec!["storage", "check", f.0.to_str().unwrap()],
    ] {
        let error = failure(f.run(&args));
        assert!(error.contains("not initialized"), "{args:?}: {error}");
        assert!(!error.contains("Corrupt"), "{args:?}: {error}");
    }
}

#[test]
fn storage_check_reports_a_missing_header_outside_git_without_a_root_argument() {
    let f = Fixture::new();
    f.init();
    f.accepted("kept");
    fs::remove_file(f.header()).unwrap();
    let nested = f.0.join("nested");
    fs::create_dir(&nested).unwrap();
    let error = failure(
        command(&nested)
            .args(["storage", "check"])
            .output()
            .unwrap(),
    );
    assert!(error.contains("Corrupt: header.json: missing"), "{error}");
}

#[test]
fn storage_check_outside_git_skips_initialization_residue_below_a_headerless_store() {
    let f = Fixture::new();
    f.init();
    f.accepted("kept");
    fs::remove_file(f.header()).unwrap();
    // Discovery passes the residue of an interrupted initialization in the nested directory
    // and stops at the headerless store above it; the check reports that store.
    let nested = f.0.join("nested");
    fs::create_dir_all(nested.join(".axon/records")).unwrap();
    fs::write(nested.join(".axon/header.json.tmp"), "partial").unwrap();
    let error = failure(
        command(&nested)
            .args(["storage", "check"])
            .output()
            .unwrap(),
    );
    assert!(error.contains("Corrupt: header.json: missing"), "{error}");
    assert!(
        error.contains(f.0.join(".axon").to_str().unwrap()),
        "{error}"
    );
}

#[test]
fn storage_check_inside_git_does_not_climb_past_the_repository() {
    let f = Fixture::new();
    // An obstructing `.axon` above the repository is never the store to report on.
    fs::create_dir(f.0.join(".axon")).unwrap();
    fs::write(f.0.join(".axon/state.jsonl"), b"old\n").unwrap();
    let repo = f.0.join("repo");
    fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    let error = failure(command(&repo).args(["storage", "check"]).output().unwrap());
    assert!(
        error.contains("not initialized") && !error.contains("Corrupt"),
        "{error}"
    );
}

#[test]
fn storage_check_reports_a_missing_header_inside_git_without_a_root_argument() {
    let f = Fixture::new();
    git(&f.0, &["init", "-q"]);
    f.init();
    f.accepted("kept");
    fs::remove_file(f.header()).unwrap();
    let error = failure(f.run(&["storage", "check"]));
    assert!(error.contains("Corrupt: header.json: missing"), "{error}");
    assert!(!error.contains("not a store"), "{error}");
}

#[test]
fn a_foreign_record_subdirectory_is_reported_once_even_when_empty() {
    let f = Fixture::new();
    f.init();
    f.accepted("kept");
    fs::create_dir(f.records_dir().join("foo")).unwrap();
    let error = failure(f.run(&["storage", "check"]));
    assert!(
        error.contains("1 corrupt files") && error.contains("Corrupt: foo:"),
        "{error}"
    );
    fs::write(f.records_dir().join("foo/x.tmp"), b"").unwrap();
    fs::write(f.records_dir().join("foo/y"), b"").unwrap();
    let error = failure(f.run(&["storage", "check"]));
    assert!(
        error.contains("1 corrupt files") && error.contains("Corrupt: foo:"),
        "{error}"
    );
}

#[test]
fn storage_check_reports_a_missing_or_unreadable_header_by_path() {
    for (kind, content) in [
        ("missing", None),
        (
            "unknown-format",
            Some("{\"format\":\"axon-records/v2\",\"store\":\"store-1\",\"prefix\":\"t\"}\n"),
        ),
        ("corrupt", Some("not a header\n")),
    ] {
        let f = Fixture::new();
        f.init();
        f.accepted("kept");
        match content {
            None => fs::remove_file(f.header()).unwrap(),
            Some(text) => fs::write(f.header(), text).unwrap(),
        }
        for args in [
            vec!["storage", "check"],
            vec!["storage", "check", f.0.to_str().unwrap()],
        ] {
            let error = failure(f.run(&args));
            assert!(error.contains("header.json"), "{kind} {args:?}: {error}");
            assert!(!error.contains("run axon init"), "{kind} {args:?}: {error}");
            assert!(!error.contains("consistent"), "{kind}: {error}");
        }
    }
}

#[test]
fn init_refuses_to_replace_an_existing_store() {
    let f = Fixture::new();
    f.init();
    let id = f.accepted("kept");
    let before = f.record_files();
    assert!(failure(f.run(&["init"])).contains("already initialized"));
    assert!(failure(f.run(&["init", "other"])).contains("already initialized"));
    assert_eq!(f.record_files(), before);
    assert!(f.ok(&["list"]).contains(&id));
}

#[test]
fn storage_check_reports_conflicts_violations_and_gaps_by_severity() {
    // A conflict: the same Issue started on two copies of the store.
    let f = Fixture::new();
    f.init();
    let id = f.accepted("contested");
    let other = copy_store(&f);
    f.ok(&["start", &id]);
    other.ok(&["start", &id]);
    merge_records(&other.0, &f.0);
    let error = failure(f.run(&["storage", "check"]));
    assert!(error.contains("1 problems"), "{error}");
    assert!(
        error.contains(&format!(
            "Conflicted: {id}  Issue  Conflicted  contested  2 heads"
        )),
        "{error}"
    );
    let show = f.ok(&["show", &id]);
    assert!(show.contains("Conflicted\n"), "{show}");
    assert_eq!(show.matches("Start").count(), 2, "{show}");
    assert!(!f.ok(&["tasks"]).contains(&id));
    assert!(f.ok(&["list"]).contains("Conflicted"));

    // A violation: two copies each move one Group under the other.
    let f = Fixture::new();
    f.init();
    let a = f
        .ok(&["capture", "--kind", "group", "--accept", "--title", "A"])
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    let b = f
        .ok(&["capture", "--kind", "group", "--accept", "--title", "B"])
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    let leaf = f
        .ok(&["capture", "--accept", "--title", "Leaf", "--parent", &b])
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    let other = copy_store(&f);
    f.ok(&["parent", "set", &b, "--parent", &a]);
    other.ok(&["parent", "set", &a, "--parent", &b]);
    merge_records(&other.0, &f.0);
    // Each Group is on the containment cycle and, as each is the other's completion
    // prerequisite, on the completion cycle: one line per Entity and kind.
    let error = failure(f.run(&["storage", "check"]));
    assert!(error.contains("4 problems"), "{error}");
    for id in [&a, &b] {
        for kind in ["containment cycle", "completion cycle"] {
            assert!(
                error.contains(&format!("Violation: {id}  Group  Ready+Invalid"))
                    && error.contains(kind),
                "{kind}: {error}"
            );
        }
    }
    // Every walk over the cycle terminates and marks the members.
    let show = f.ok(&["show", &a]);
    assert!(show.contains("+Invalid"), "{show}");
    assert!(show.contains("Invalid\ncontainment cycle:"), "{show}");
    let tasks = f.ok(&["tasks"]);
    assert!(
        tasks.contains(&format!("{a}  Group")) && tasks.contains("+Invalid"),
        "{tasks}"
    );
    assert!(tasks.contains(&leaf), "{tasks}");
    let list = f.run(&["list"]);
    assert!(list.status.success());
    assert!(String::from_utf8_lossy(&list.stderr).contains("4 violations"));
    // Ordinary operations continue while the violation is unrelated to them, and a repair
    // that removes the violation is an ordinary operation.
    f.ok(&["start", &leaf]);
    f.ok(&["parent", "unset", &a]);
    let check = f.run(&["storage", "check"]);
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );

    // A dependency whose registration is missing (its file reverted) is a violation that
    // `dep rm` repairs although the target cannot be resolved as an Entity.
    let f = Fixture::new();
    f.init();
    let user = f.accepted("user");
    let dep = f.accepted("dep");
    f.ok(&["dep", "add", &user, "--needs", &dep]);
    for relative in record_paths_of(&f.0, &dep) {
        fs::remove_file(f.records_dir().join(relative)).unwrap();
    }
    let error = failure(f.run(&["storage", "check"]));
    assert!(error.contains("unknown dependency"), "{error}");
    failure(f.run(&["dep", "add", &user, "--needs", &dep]));
    // Among the Entity's own dependencies a unique suffix resolves; an ambiguous one is
    // refused without a record; a complete ID of another Entity resolves to that Entity.
    f.publish(vec![
        registration(&f.records(), "t-twin-ab"),
        registration(&f.records(), "t-other-ab"),
    ]);
    f.ok(&["dep", "add", &user, "--needs", "t-twin-ab"]);
    f.publish(vec![registration(&f.records(), "t-gone-ab")]);
    f.ok(&["dep", "add", &user, "--needs", "t-gone-ab"]);
    for relative in record_paths_of(&f.0, "t-gone-ab") {
        fs::remove_file(f.records_dir().join(relative)).unwrap();
    }
    let files = f.record_files();
    assert!(failure(f.run(&["dep", "rm", &user, "--needs", "ab"])).contains("ambiguous"));
    assert_eq!(f.record_files(), files);
    assert!(
        f.ok(&["dep", "rm", &user, "--needs", "other-ab"])
            .contains("already absent")
    );
    assert_eq!(f.record_files(), files);
    f.ok(&["dep", "rm", &user, "--needs", "gone-ab"]);
    f.ok(&["dep", "rm", &user, "--needs", &dep[dep.len() - 5..]]);
    f.ok(&["dep", "rm", &user, "--needs", "t-twin-ab"]);
    assert!(f.run(&["storage", "check"]).status.success());
    // The show of an Entity waiting for a missing dependency names it without a row.
    f.ok(&["dep", "add", &user, "--needs", "t-twin-ab"]);
    for relative in record_paths_of(&f.0, "t-twin-ab") {
        fs::remove_file(f.records_dir().join(relative)).unwrap();
    }
    let show = f.ok(&["show", &user]);
    assert!(
        show.contains("Dependency must complete: t-twin-ab  (missing)"),
        "{show}"
    );
    f.ok(&["dep", "rm", &user, "--needs", "t-twin-ab"]);
    // Files beside the header that are not the store's are neither read nor reported.
    fs::write(f.0.join(".axon/state.jsonl"), b"earlier format\n").unwrap();
    fs::write(f.0.join(".axon/notes.txt"), b"mine").unwrap();
    assert!(f.ok(&["list"]).contains(&user));
    let check = f.run(&["storage", "check"]);
    assert!(check.status.success());
    assert!(!String::from_utf8_lossy(&check.stderr).contains("state.jsonl"));
    f.ok(&["note", "add", &user, "-m", "still writable"]);
    assert_eq!(
        fs::read(f.0.join(".axon/state.jsonl")).unwrap(),
        b"earlier format\n"
    );
    assert_eq!(fs::read(f.0.join(".axon/notes.txt")).unwrap(), b"mine");

    // A gap: a record whose parent record file is gone, as a revert leaves it.
    let f = Fixture::new();
    f.init();
    let id = f.accepted("gapped");
    let created = record_paths_of(&f.0, &id);
    f.ok(&["start", &id]);
    f.ok(&["note", "add", &id, "-m", "kept"]);
    f.ok(&["release", &id]);
    let path = f.records_dir().join(&created[0]);
    fs::remove_file(&path).unwrap();
    let check = f.run(&["storage", "check"]);
    assert!(check.status.success());
    let report = String::from_utf8(check.stdout).unwrap();
    assert!(
        report.contains(&format!("Missing parent records: {id}")),
        "{report}"
    );
    assert!(
        report.contains(created[0].file_name().unwrap().to_str().unwrap()),
        "{report}"
    );
    // The Entity keeps its current value and its history reads with the gap marked.
    assert!(f.ok(&["show", &id]).contains("Issue  Ready  gapped"));
    let log = f.ok(&["log", &id]);
    assert!(log.contains("parent missing"), "{log}");
    assert!(log.contains("unknown → InProgress"), "{log}");
    assert!(f.ok(&["note", "list", &id]).contains("kept"));
    let list = f.run(&["list"]);
    assert!(String::from_utf8_lossy(&list.stderr).contains("missing records"));
    f.ok(&["start", &id]);
    // Notes of an Entity without any other record are kept and reported as information.
    for relative in record_paths_of(&f.0, &id) {
        let path = f.records_dir().join(&relative);
        let bytes = fs::read(&path).unwrap();
        if !bytes.windows(15).any(|w| w == br#""record":"note""#) {
            fs::remove_file(&path).unwrap();
        }
    }
    let check = f.run(&["storage", "check"]);
    assert!(check.status.success());
    assert!(
        String::from_utf8(check.stdout)
            .unwrap()
            .contains(&format!("Missing records: {id}  Notes only"))
    );
    assert!(failure(f.run(&["show", &id])).contains("missing Entity"));
    assert!(f.ok(&["note", "list", &id]).contains("kept"));
    assert!(failure(f.run(&["note", "add", &id, "-m", "rejected"])).contains("missing Entity"));
}

#[test]
fn tracked_worktrees_merge_record_files_without_attributes_or_configuration() {
    let f = Fixture::new();
    git(&f.0, &["init", "-q"]);
    f.init();
    let id = f.accepted("job");
    let other = f.accepted("other");
    track_store(&f.0);
    git_commit(&f.0, &["-qm", "base"]);
    // Only records, the header and the ignore file are tracked.
    let tracked =
        String::from_utf8(git_output(&f.0, &["ls-tree", "-r", "--name-only", "HEAD"]).stdout)
            .unwrap();
    assert!(tracked.contains(".axon/header.json") && tracked.contains(".axon/.gitignore"));
    assert!(!tracked.contains("write.lock"));
    assert_eq!(tracked.matches(".axon/records/").count(), 2);
    let a = Fixture::new();
    let b = Fixture::new();
    git(
        &f.0,
        &["worktree", "add", "-qb", "a", a.0.to_str().unwrap()],
    );
    git(
        &f.0,
        &["worktree", "add", "-qb", "b", b.0.to_str().unwrap()],
    );
    let before = f.record_files();
    a.ok(&["start", &id]);
    a.ok(&["note", "add", &id, "-m", "A"]);
    assert_eq!(f.record_files(), before);
    assert_eq!(b.record_files(), before);
    b.ok(&["start", &other]);
    b.ok(&["note", "add", &id, "-m", "B"]);
    for branch in [&a, &b] {
        git(&branch.0, &["add", ".axon"]);
        git_commit(&branch.0, &["-qm", "branch"]);
    }
    assert!(
        git_output(
            &a.0,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "-c",
                "core.hooksPath=/dev/null",
                "merge",
                "--no-edit",
                "b",
            ],
        )
        .status
        .success()
    );
    assert!(git_output(&a.0, &["ls-files", "-u"]).stdout.is_empty());
    assert_eq!(a.records().notes_of(&eid(&id)).len(), 2);
    for started in [&id, &other] {
        assert!(a.ok(&["show", started]).contains("InProgress"));
    }
    let check = a.run(&["storage", "check"]);
    assert!(check.status.success());
    assert_eq!(f.record_files(), before);
    git(&f.0, &["merge", "--ff-only", "a"]);
    assert_eq!(f.records().notes_of(&eid(&id)).len(), 2);
    // A corrupt record file fast-forwarded in stops reads until it is repaired.
    let broken = a.records_dir().join(&a.record_files()[0]);
    fs::write(&broken, b"broken record\n").unwrap();
    git(&a.0, &["add", ".axon"]);
    git_commit(&a.0, &["-qm", "corrupt record"]);
    git(&f.0, &["merge", "--ff-only", "a"]);
    assert!(failure(f.run(&["list"])).contains("corrupt"));
    assert_eq!(
        fs::read(f.records_dir().join(&f.record_files()[0])).unwrap(),
        b"broken record\n"
    );
}

#[test]
fn concurrent_work_on_one_issue_merges_in_git_and_reads_as_a_conflict() {
    let f = Fixture::new();
    git(&f.0, &["init", "-q"]);
    f.init();
    let group = f
        .ok(&[
            "capture", "--kind", "group", "--accept", "--title", "delivery",
        ])
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    let id = f
        .ok(&["capture", "--accept", "--title", "job", "--parent", &group])
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    f.ok(&["start", &id]);
    track_store(&f.0);
    let commit = |path: &Path, message: &str| {
        git(path, &["add", ".axon"]);
        git_commit(path, &["-qm", message]);
    };
    commit(&f.0, "base");
    let a = Fixture::new();
    let b = Fixture::new();
    git(
        &f.0,
        &["worktree", "add", "-qb", "finished", a.0.to_str().unwrap()],
    );
    git(
        &f.0,
        &["worktree", "add", "-qb", "remaining", b.0.to_str().unwrap()],
    );
    a.ok(&["complete", &id]);
    a.ok(&["note", "add", &id, "-m", "completed branch evidence"]);
    b.ok(&["release", &id, "-r", "remaining work"]);
    b.ok(&["note", "add", &id, "-m", "remaining branch evidence"]);
    commit(&a.0, "finish");
    commit(&b.0, "release");
    let merge = git_output(
        &a.0,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "core.hooksPath=/dev/null",
            "merge",
            "--no-edit",
            "remaining",
        ],
    );
    assert!(
        merge.status.success(),
        "{}",
        String::from_utf8_lossy(&merge.stderr)
    );
    assert!(git_output(&a.0, &["ls-files", "-u"]).stdout.is_empty());
    // Git merged the record files; the next read sees the two heads as a conflict.
    let error = failure(a.run(&["storage", "check"]));
    assert!(error.contains(&format!("Conflicted: {id}")), "{error}");
    let show = a.ok(&["show", &id]);
    assert!(show.contains("Issue  Conflicted  job"), "{show}");
    assert!(!show.contains("parent missing"), "{show}");
    let log = a.ok(&["log", &id]);
    assert!(log.contains("InProgress → Completed"));
    assert!(log.contains("InProgress → NotStarted") && log.contains("remaining work"));
    assert!(log.contains("Concurrent branch"));
    let notes = a.ok(&["note", "list", &id]);
    assert!(notes.contains("completed branch evidence"));
    assert!(notes.contains("remaining branch evidence"));
    // The Group is read with the conflicted child absent, and every ordinary operation but a
    // Note is refused until the conflict is resolved.
    let group_show = a.ok(&["show", &group]);
    assert!(
        group_show.contains("Group  Empty  delivery"),
        "{group_show}"
    );
    failure(a.run(&["start", &id]));
    failure(a.run(&["complete", &group]));
    a.ok(&["note", "add", &id, "-m", "seen the conflict"]);
    assert!(!a.ok(&["tasks"]).contains(&id));
}

#[test]
fn valid_store_with_unmerged_index_rejects_normal_operations() {
    let f = Fixture::new();
    git(&f.0, &["init", "-q"]);
    f.init();
    let id = f.accepted("job");
    track_store(&f.0);
    let relative = f.record_files()[0].clone();
    let path = format!(".axon/records/{}", relative.display());
    let blob = String::from_utf8(git_output(&f.0, &["hash-object", &path]).stdout).unwrap();
    git(&f.0, &["update-index", "--force-remove", &path]);
    let line = format!(
        "100644 {} 1\t{path}\n100644 {} 2\t{path}\n100644 {} 3\t{path}\n",
        blob.trim(),
        blob.trim(),
        blob.trim()
    );
    let mut p = isolated_git(&f.0)
        .args(["update-index", "--index-info"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    p.stdin.take().unwrap().write_all(line.as_bytes()).unwrap();
    assert!(p.wait().unwrap().success());
    let before = f.record_files();
    assert!(failure(f.run(&["list"])).contains("unmerged"));
    failure(f.run(&["start", &id]));
    assert_eq!(f.record_files(), before);
    // An explicit root is checked without Git.
    let check = f.run(&["storage", "check", f.0.to_str().unwrap()]);
    assert!(check.status.success());
    git(&f.0, &["add", ".axon"]);
    f.ok(&["start", &id]);
}
#[cfg(unix)]
fn git_on_path() -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|dir| dir.join("git"))
        .find(|path| path.is_file())
        .expect("git on PATH")
}

/// A `git` that logs its arguments and hands over to the real one.
#[cfg(unix)]
fn git_call_log(shim: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let log = shim.join("git-calls");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nexec '{}' \"$@\"\n",
        log.display(),
        git_on_path().display()
    );
    let binary = shim.join("git");
    fs::write(&binary, script).unwrap();
    fs::set_permissions(&binary, PermissionsExt::from_mode(0o755)).unwrap();
    log
}

#[cfg(unix)]
#[test]
fn git_worktree_operations_start_few_git_processes() {
    let f = Fixture::new();
    git(&f.0, &["init", "-q"]);
    git_commit(&f.0, &["-q", "--allow-empty", "-m", "base"]);
    f.init();
    let id = f.accepted("counted");
    let linked = Fixture::new();
    git(
        &f.0,
        &[
            "worktree",
            "add",
            "-qb",
            "linked",
            linked.0.to_str().unwrap(),
        ],
    );
    let shim = Fixture::new();
    let log = git_call_log(&shim.0);
    let calls = |cwd: &Path, args: &[&str]| -> Vec<String> {
        fs::write(&log, "").unwrap();
        success(
            command(cwd)
                .env(
                    "PATH",
                    format!(
                        "{}:{}",
                        shim.0.display(),
                        std::env::var("PATH").unwrap_or_default()
                    ),
                )
                .args(args)
                .output()
                .unwrap(),
        );
        fs::read_to_string(&log)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect()
    };
    // Discovery runs one rev-parse, and one more only from a linked worktree without a store of
    // its own, which has to look at the main worktree. Reads check the index once, writes twice.
    for (worktree, rev_parse) in [(&f.0, 1), (&linked.0, 2)] {
        for (args, unmerged) in [
            (vec!["list"], 1),
            (vec!["proposals"], 1),
            (vec!["tasks"], 1),
            (vec!["show", &id], 1),
            (vec!["capture", "--title", "written"], 2),
            (vec!["note", "add", &id, "-m", "evidence"], 2),
        ] {
            let observed = calls(worktree, &args);
            assert_eq!(
                observed.len(),
                rev_parse + unmerged,
                "{args:?}: {observed:?}"
            );
            assert_eq!(
                observed
                    .iter()
                    .filter(|call| call.starts_with("rev-parse"))
                    .count(),
                rev_parse,
                "{args:?}: {observed:?}"
            );
            assert_eq!(
                observed
                    .iter()
                    .filter(|call| call.contains("ls-files --unmerged"))
                    .count(),
                unmerged,
                "{args:?}: {observed:?}"
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn newline_in_a_git_worktree_path_fails_discovery_explicitly() {
    let f = Fixture::new();
    let repo = f.0.join("repo\nline");
    fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    for args in [vec!["init", "t"], vec!["list"]] {
        let error = failure(command(&repo).args(args).output().unwrap());
        assert!(error.contains("newline"), "{error}");
    }
    assert!(!repo.join(".axon").exists());
}

#[test]
fn init_refuses_an_unmerged_index_without_creating_a_store() {
    let f = Fixture::new();
    git(&f.0, &["init", "-q"]);
    f.init();
    track_store(&f.0);
    git_commit(&f.0, &["-qm", "base"]);
    let blob =
        String::from_utf8(git_output(&f.0, &["hash-object", "-w", ".axon/header.json"]).stdout)
            .unwrap();
    let line = format!("100644 {} 2\t.axon/header.json\n", blob.trim());
    let mut p = isolated_git(&f.0)
        .args(["update-index", "--index-info"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    p.stdin.take().unwrap().write_all(line.as_bytes()).unwrap();
    assert!(p.wait().unwrap().success());
    fs::remove_file(f.header()).unwrap();
    assert!(failure(f.run(&["init"])).contains("unmerged"));
    assert!(!f.header().exists());
    assert!(!f.0.join(".axon/header.json.tmp").exists());
}

#[test]
fn git_index_fixtures_do_not_touch_an_inherited_hook_index() {
    let f = Fixture::new();
    let index = f.0.join("foreign-index");
    fs::write(&index, b"foreign index must remain untouched").unwrap();
    for test in [
        "file_lifecycle::valid_store_with_unmerged_index_rejects_normal_operations",
        "file_lifecycle::init_refuses_an_unmerged_index_without_creating_a_store",
    ] {
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test])
            .env("GIT_INDEX_FILE", &index)
            .env("GIT_DIR", &f.0)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            fs::read(&index).unwrap(),
            b"foreign index must remain untouched"
        );
    }
}
