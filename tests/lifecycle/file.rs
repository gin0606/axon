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
    let edited = String::from_utf8(good.clone())
        .unwrap()
        .replacen("\"title\":\"job\"", "\"title\":\"jobs\"", 1)
        .into_bytes();
    // A file that is another content with CRLF line endings is not a converted record.
    let edited_crlf = String::from_utf8(edited.clone())
        .unwrap()
        .replace('\n', "\r\n")
        .into_bytes();
    for (label, corrupt) in [
        ("hash", &good[..good.len() - 2].to_vec()),
        ("empty", &Vec::new()),
        ("edited", &edited),
        ("edited-crlf", &edited_crlf),
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
        // None of these is a converted record, so no line-ending diagnosis appears.
        assert!(
            !error.contains("CRLF") && !error.contains("line-ending"),
            "{label}: {error}"
        );
        if label == "edited-crlf" {
            assert!(error.contains("not canonical"), "{label}: {error}");
        }
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
    fs::write(f.0.join(".axon/.gitattributes"), "* -text\n").unwrap();
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
fn log_of_a_gapped_entity_starts_with_its_creation() {
    // Start then release, and the Start's file is removed as a revert leaves it. The Release
    // is then listable at once; the log still starts with the creation, whatever the IDs.
    let f = Fixture::new();
    f.init();
    let kind_of = |relative: &PathBuf| -> String {
        let bytes = fs::read(f.records_dir().join(relative)).unwrap();
        let row: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        row["operation"]
            .as_str()
            .or_else(|| row["record"].as_str())
            .unwrap()
            .into()
    };
    let (id, start) = (0..64)
        .find_map(|n| {
            let id = f.accepted(&format!("gapped {n}"));
            f.ok(&["start", &id]);
            f.ok(&["release", &id]);
            let paths = record_paths_of(&f.0, &id);
            let find = |kind: &str| paths.iter().find(|p| kind_of(p) == kind).unwrap().clone();
            let (created, start, release) = (find("created"), find("start"), find("release"));
            (file_name(&release) < file_name(&created)).then_some((id, start))
        })
        .expect("some Release has a smaller record ID than its creation");
    fs::remove_file(f.records_dir().join(&start)).unwrap();
    let log = f.ok(&["log", &id]);
    let lines: Vec<_> = log.lines().collect();
    assert!(lines[0].contains("Created: NotStarted"), "{log}");
    assert!(lines[1].starts_with("Concurrent branch"), "{log}");
    assert!(
        lines[2].contains("unknown → NotStarted  parent missing"),
        "{log}"
    );
}

#[test]
fn tracked_worktrees_merge_record_files_without_merge_attributes_or_configuration() {
    let f = Fixture::new();
    git(&f.0, &["init", "-q"]);
    f.init();
    let id = f.accepted("job");
    let other = f.accepted("other");
    track_store(&f.0);
    git_commit(&f.0, &["-qm", "base"]);
    // Only records, the header, the ignore file and the attributes file are tracked.
    let tracked =
        String::from_utf8(git_output(&f.0, &["ls-tree", "-r", "--name-only", "HEAD"]).stdout)
            .unwrap();
    for path in [
        ".axon/header.json",
        ".axon/.gitignore",
        ".axon/.gitattributes",
    ] {
        assert!(tracked.lines().any(|line| line == path), "{tracked}");
    }
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
        git_integration(&a.0, &["merge", "--no-edit", "b"])
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
    let merge = git_integration(&a.0, &["merge", "--no-edit", "remaining"]);
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
    // Taking the Completed head settles the Issue at that value; the Group reads its child
    // again, both branches' Notes stay, and ordinary operations resume.
    let listing = a.ok(&["resolve", &id]);
    let completed = head_line(&listing, "Complete  Completed  Issue  job")
        .split("  ")
        .next()
        .unwrap()
        .to_owned();
    assert_eq!(
        a.ok(&[
            "resolve",
            &id,
            "--head",
            &completed,
            "-r",
            "the work was finished"
        ]),
        format!("{id}  Resolved: {completed}  Completed\n")
    );
    consistent(&a);
    assert!(a.ok(&["show", &id]).contains("Issue  Completed  job"));
    let group_show = a.ok(&["show", &group]);
    assert!(
        group_show.contains("Group  Confirmable  delivery")
            && group_show.contains("1/1 terminal (1 completed, 0 cancelled)"),
        "{group_show}"
    );
    let notes = a.ok(&["note", "list", &id]);
    assert!(
        notes.contains("completed branch evidence") && notes.contains("remaining branch evidence")
    );
    a.ok(&["complete", &group]);
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
    // An explicit root inside the worktree checks the index as discovery does.
    let error = failure(f.run(&["storage", "check", f.0.to_str().unwrap()]));
    assert!(error.contains("unmerged"), "{error}");
    git(&f.0, &["add", ".axon"]);
    f.ok(&["start", &id]);
}
/// Stages and commits the store, as the tracked operation does after every step.
fn commit_store(path: &Path, message: &str) -> String {
    git(path, &["add", ".axon"]);
    git_commit(path, &["-qm", message]);
    String::from_utf8(git_output(path, &["rev-parse", "HEAD"]).stdout)
        .unwrap()
        .trim()
        .to_owned()
}
/// A tracked store with one accepted Issue, committed on the main worktree's branch.
fn tracked_repository(title: &str) -> (Fixture, String) {
    let f = Fixture::new();
    git(&f.0, &["init", "-q"]);
    f.init();
    let id = f.accepted(title);
    commit_store(&f.0, "base");
    (f, id)
}
/// The record IDs of the head lines in a `resolve` listing.
fn listed_heads(listing: &str) -> Vec<String> {
    listing
        .lines()
        .filter_map(|line| line.split("  ").next())
        .filter(|first| first.len() == 64)
        .map(str::to_owned)
        .collect()
}
/// The head line of a `resolve` listing that carries the given text.
fn head_line<'a>(listing: &'a str, text: &str) -> &'a str {
    listing
        .lines()
        .find(|line| line.contains(text))
        .unwrap_or_else(|| panic!("no head line with {text:?} in:\n{listing}"))
}
fn consistent(f: &Fixture) {
    let check = f.run(&["storage", "check"]);
    assert!(
        check.status.success() && check.stdout.is_empty(),
        "{}{}",
        String::from_utf8_lossy(&check.stdout),
        String::from_utf8_lossy(&check.stderr)
    );
    assert!(String::from_utf8_lossy(&check.stderr).contains("consistent"));
}
fn unmerged_paths(path: &Path) -> Vec<u8> {
    git_output(path, &["ls-files", "-u"]).stdout
}

#[test]
fn rebase_keeps_records_of_different_issues_and_the_same_issue_reads_as_a_conflict_that_resolve_settles()
 {
    let (f, x) = tracked_repository("x");
    // Registered after x under an ID that sorts before it, so that creation order and ID
    // order disagree.
    let y = "t-00000000".to_owned();
    f.publish(vec![Entry::Record(
        f.records()
            .create(eid(&y), current("y"), context())
            .unwrap(),
    )]);
    assert!(y < x);
    commit_store(&f.0, "y");
    let a = add_worktree(&f.0, "a");
    let b = add_worktree(&f.0, "b");
    a.ok(&["start", &x]);
    commit_store(&a.0, "start x");
    b.ok(&["start", &y]);
    commit_store(&b.0, "start y");
    // Work on different Issues rebases unattended and reads as both.
    let rebase = git_integration(&b.0, &["rebase", "-q", "a"]);
    assert!(
        rebase.status.success(),
        "{}",
        String::from_utf8_lossy(&rebase.stderr)
    );
    assert!(unmerged_paths(&b.0).is_empty());
    consistent(&b);
    for id in [&x, &y] {
        assert!(b.ok(&["show", id]).contains("InProgress"), "{id}");
    }
    // The same Issues started on both sides rebase without a Git conflict and read as Axon
    // conflicts with two heads each, which stops every ordinary operation.
    let c = add_worktree(&f.0, "c");
    c.ok(&["start", &x]);
    c.ok(&["start", &y]);
    commit_store(&c.0, "start x and y again");
    let rebase = git_integration(&c.0, &["rebase", "-q", "b"]);
    assert!(
        rebase.status.success(),
        "{}",
        String::from_utf8_lossy(&rebase.stderr)
    );
    assert!(unmerged_paths(&c.0).is_empty());
    let error = failure(c.run(&["storage", "check"]));
    assert!(error.contains("2 problems"), "{error}");
    for (id, title) in [(&x, "x"), (&y, "y")] {
        assert!(
            error.contains(&format!(
                "Conflicted: {id}  Issue  Conflicted  {title}  2 heads"
            )),
            "{error}"
        );
    }
    assert!(failure(c.run(&["release", &x])).contains("resolve"));
    // The listing shows every conflicted Entity, each row followed by its own heads and the
    // Entities separated by a blank line; with an ID, that Entity alone.
    let listing = c.ok(&["resolve"]);
    let blocks: Vec<&str> = listing.split("\n\n").collect();
    assert_eq!(blocks.len(), 2, "{listing}");
    for (block, (id, title)) in blocks.iter().zip([(&x, "x"), (&y, "y")]) {
        assert!(
            block.starts_with(&format!("{id}  Issue  Conflicted  {title}\n")),
            "{listing}"
        );
        assert_eq!(listed_heads(block).len(), 2, "{listing}");
        assert_eq!(
            block
                .matches(&format!("Start  InProgress  Issue  {title}"))
                .count(),
            2,
            "{listing}"
        );
    }
    assert!(!listing.contains("parent missing"), "{listing}");
    assert_eq!(c.ok(&["resolve", &x]), format!("{}\n", blocks[0]));
    assert_eq!(c.ok(&["resolve", &y]), blocks[1]);
    let listing = c.ok(&["resolve", &x]);
    let heads = listed_heads(&listing);
    assert_eq!(
        c.ok(&["show", &x])
            .lines()
            .filter(|line| heads.iter().any(|head| line.starts_with(head)))
            .count(),
        2,
        "show lists the same heads"
    );
    // Only a head of the Entity can be chosen; a head needs an ID and a reason a head.
    let files = c.record_files();
    let error = failure(c.run(&["resolve", &x, "--head", &"0".repeat(64)]));
    assert!(error.contains("not a head"), "{error}");
    assert_eq!(
        c.run(&["resolve", &x, "-r", "no head"]).status.code(),
        Some(2)
    );
    assert_eq!(
        c.run(&["resolve", "--head", &heads[0]]).status.code(),
        Some(2)
    );
    assert_eq!(c.record_files(), files);
    let resolved = c.ok(&["resolve", &x, "--head", &heads[0], "-r", "keep this side"]);
    assert_eq!(
        resolved,
        format!("{x}  Resolved: {}  InProgress\n", heads[0])
    );
    assert_eq!(c.record_files().len(), files.len() + 1);
    // The other Entity stays conflicted and keeps blocking ordinary operations until it is
    // resolved as well.
    let remaining = c.ok(&["resolve"]);
    assert!(
        remaining.starts_with(&format!("{y}  Issue  Conflicted  y\n")) && !remaining.contains(&x),
        "{remaining}"
    );
    let error = failure(c.run(&["release", &x]));
    assert!(error.contains(&y) && error.contains("resolve"), "{error}");
    let y_head = listed_heads(&remaining).remove(0);
    c.ok(&["resolve", &y, "--head", &y_head]);
    consistent(&c);
    assert!(c.ok(&["show", &x]).contains("Issue  InProgress  x"));
    let log = c.ok(&["log", &x]);
    assert!(
        log.contains(&format!(
            "Resolved: {}  InProgress  Issue  Reason: keep this side",
            heads[0]
        )),
        "{log}"
    );
    let none = c.run(&["resolve"]);
    assert!(none.status.success() && none.stdout.is_empty());
    assert!(String::from_utf8_lossy(&none.stderr).contains("No conflicted Entities"));
    assert!(failure(c.run(&["resolve", &x])).contains("not conflicted"));
    c.ok(&["release", &x]);
}

#[test]
fn cherry_pick_of_a_later_commit_reports_a_gap_and_a_false_conflict_until_the_rest_arrives() {
    let (f, x) = tracked_repository("x");
    let y = f.accepted("y");
    commit_store(&f.0, "y");
    let a = add_worktree(&f.0, "a");
    a.ok(&["start", &y]);
    let start_y = commit_store(&a.0, "start y");
    a.ok(&["start", &x]);
    let start = commit_store(&a.0, "start");
    a.ok(&["release", &x]);
    let release = commit_store(&a.0, "release");
    // A commit whose record continues a record both sides have picks cleanly beside work on
    // another Entity, and nothing is reported.
    f.ok(&["note", "add", &x, "-m", "meanwhile"]);
    commit_store(&f.0, "note");
    let pick = git_integration(&f.0, &["cherry-pick", &start_y]);
    assert!(
        pick.status.success(),
        "{}",
        String::from_utf8_lossy(&pick.stderr)
    );
    assert!(unmerged_paths(&f.0).is_empty());
    consistent(&f);
    assert!(f.ok(&["show", &y]).contains("Issue  InProgress  y"));
    // Picking the release without the start it continues brings one record file whose parent
    // is missing: it stands beside the registration as a head.
    let pick = git_integration(&f.0, &["cherry-pick", &release]);
    assert!(
        pick.status.success(),
        "{}",
        String::from_utf8_lossy(&pick.stderr)
    );
    let error = failure(f.run(&["storage", "check"]));
    assert!(error.contains(&format!("Conflicted: {x}")), "{error}");
    assert!(
        error.contains(&format!("Missing parent records: {x}")),
        "{error}"
    );
    let listing = f.ok(&["resolve", &x]);
    let newer = head_line(&listing, "parent missing; likely newer");
    assert!(newer.contains("Release  NotStarted  Issue  x"), "{newer}");
    assert!(
        !head_line(&listing, "created").contains("parent missing"),
        "{listing}"
    );
    let newer = newer.split("  ").next().unwrap().to_owned();
    assert_eq!(
        f.ok(&["resolve", &x, "--head", &newer]),
        format!("{x}  Resolved: {newer}  Ready\n")
    );
    // Settled at the newer value; the gap stays as information and stops nothing.
    let check = f.run(&["storage", "check"]);
    assert!(check.status.success());
    let report = String::from_utf8(check.stdout).unwrap();
    assert!(
        report.contains(&format!("Missing parent records: {x}")),
        "{report}"
    );
    assert!(!report.contains("Conflicted"), "{report}");
    assert!(f.ok(&["show", &x]).contains("Issue  Ready  x"));
    commit_store(&f.0, "resolve");
    // The missing record arrives: the value does not change and the gap closes.
    let pick = git_integration(&f.0, &["cherry-pick", &start]);
    assert!(
        pick.status.success(),
        "{}",
        String::from_utf8_lossy(&pick.stderr)
    );
    consistent(&f);
    assert!(f.ok(&["show", &x]).contains("Issue  Ready  x"));
    assert!(!f.ok(&["log", &x]).contains("parent missing"));
}

#[test]
fn revert_of_a_continued_commit_reports_a_gap_and_of_an_uncontinued_commit_nothing() {
    let (f, x) = tracked_repository("x");
    f.ok(&["start", &x]);
    let start = commit_store(&f.0, "start");
    let start_record = record_paths_of(&f.0, &x)
        .into_iter()
        .find(|relative| {
            fs::read_to_string(f.records_dir().join(relative))
                .unwrap()
                .contains("\"operation\":\"start\"")
        })
        .unwrap();
    f.ok(&["release", &x]);
    commit_store(&f.0, "release");
    // Reverting the start removes its file; the release that continues it is left with a
    // missing parent and stands beside the registration as a head.
    let revert = git_integration(&f.0, &["revert", "--no-edit", &start]);
    assert!(
        revert.status.success(),
        "{}",
        String::from_utf8_lossy(&revert.stderr)
    );
    assert!(!f.records_dir().join(&start_record).exists());
    let error = failure(f.run(&["storage", "check"]));
    assert!(error.contains(&format!("Conflicted: {x}")), "{error}");
    assert!(
        error.contains(&format!("Missing parent records: {x}")),
        "{error}"
    );
    let show = f.ok(&["show", &x]);
    assert!(show.contains("Issue  Conflicted  x"), "{show}");
    assert!(show.contains("parent missing; likely newer"), "{show}");
    let listing = f.ok(&["resolve", &x]);
    let newer = head_line(&listing, "likely newer")
        .split("  ")
        .next()
        .unwrap()
        .to_owned();
    f.ok(&["resolve", &x, "--head", &newer]);
    commit_store(&f.0, "resolve");
    let check = f.run(&["storage", "check"]);
    assert!(check.status.success());
    assert!(
        String::from_utf8_lossy(&check.stdout).contains("Missing parent records"),
        "the gap remains as information"
    );
    // The reverted file comes back from history and the gap closes.
    git(
        &f.0,
        &[
            "checkout",
            &start,
            "--",
            &format!(".axon/records/{}", start_record.display()),
        ],
    );
    commit_store(&f.0, "restore");
    consistent(&f);
    assert!(f.ok(&["show", &x]).contains("Issue  Ready  x"));
    // A commit whose record nothing continues reverts without any report; the state is
    // simply the one before it.
    f.ok(&["start", &x]);
    let again = commit_store(&f.0, "start again");
    assert!(f.ok(&["show", &x]).contains("InProgress"));
    let revert = git_integration(&f.0, &["revert", "--no-edit", &again]);
    assert!(
        revert.status.success(),
        "{}",
        String::from_utf8_lossy(&revert.stderr)
    );
    consistent(&f);
    assert!(f.ok(&["show", &x]).contains("Issue  Ready  x"));
    assert!(!f.ok(&["log", &x]).contains("parent missing"));
}

#[test]
fn squash_merge_adds_the_records_of_a_branch_without_a_report() {
    let (f, x) = tracked_repository("x");
    let y = f.accepted("y");
    commit_store(&f.0, "y");
    let a = add_worktree(&f.0, "a");
    a.ok(&["start", &x]);
    commit_store(&a.0, "start");
    a.ok(&["note", "add", &x, "-m", "evidence"]);
    commit_store(&a.0, "note");
    // Work on another Issue meanwhile on the receiving side.
    f.ok(&["start", &y]);
    commit_store(&f.0, "start y");
    let squash = git_integration(&f.0, &["merge", "--squash", "a"]);
    assert!(
        squash.status.success(),
        "{}",
        String::from_utf8_lossy(&squash.stderr)
    );
    assert!(unmerged_paths(&f.0).is_empty());
    git_commit(&f.0, &["-qm", "squashed"]);
    consistent(&f);
    for (id, title) in [(&x, "x"), (&y, "y")] {
        assert!(
            f.ok(&["show", id])
                .contains(&format!("Issue  InProgress  {title}"))
        );
    }
    assert!(f.ok(&["note", "list", &x]).contains("evidence"));
    assert!(!f.ok(&["log", &x]).contains("Concurrent branch"));
}

#[test]
fn a_child_registered_beside_a_completed_group_is_a_violation_that_reopen_repairs() {
    let f = Fixture::new();
    git(&f.0, &["init", "-q"]);
    f.init();
    let group = f
        .ok(&["capture", "--kind", "group", "--accept", "--title", "plan"])
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    commit_store(&f.0, "base");
    let a = add_worktree(&f.0, "a");
    let b = add_worktree(&f.0, "b");
    a.ok(&["complete", &group]);
    commit_store(&a.0, "complete");
    let child = b
        .ok(&[
            "capture", "--accept", "--title", "child", "--parent", &group,
        ])
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    commit_store(&b.0, "child");
    let merge = git_integration(&a.0, &["merge", "--no-edit", "b"]);
    assert!(
        merge.status.success(),
        "{}",
        String::from_utf8_lossy(&merge.stderr)
    );
    assert!(unmerged_paths(&a.0).is_empty());
    // Both sides were valid; together they leave an unfinished child under a Completed Group.
    let error = failure(a.run(&["storage", "check"]));
    assert!(error.contains("1 problems"), "{error}");
    assert!(
        error.contains(&format!("Violation: {child}"))
            && error.contains("unfinished under a terminal parent"),
        "{error}"
    );
    let show = a.ok(&["show", &child]);
    assert!(show.contains("+Invalid"), "{show}");
    assert!(
        show.contains("Invalid\nunfinished under a terminal parent"),
        "{show}"
    );
    // Reopening the Group is the ordinary operation that removes the violation.
    a.ok(&["reopen", &group]);
    consistent(&a);
    assert!(a.ok(&["show", &child]).contains("Issue  Ready  child"));
    assert!(a.ok(&["show", &group]).contains("Group  Ready  plan"));
}

#[test]
fn work_left_below_a_completed_group_names_it_as_unadopted_and_not_as_unsurfaced() {
    let f = Fixture::new();
    git(&f.0, &["init", "-q"]);
    f.init();
    let capture = |f: &Fixture, args: &[&str]| -> String {
        f.ok(&[&["capture", "--accept"], args].concat())
            .split_whitespace()
            .next()
            .unwrap()
            .into()
    };
    let done = capture(&f, &["--kind", "group", "--title", "done"]);
    commit_store(&f.0, "base");
    let a = add_worktree(&f.0, "a");
    let b = add_worktree(&f.0, "b");
    a.ok(&["complete", &done]);
    commit_store(&a.0, "complete");
    let plan = capture(
        &b,
        &["--kind", "group", "--title", "plan", "--parent", &done],
    );
    let work = capture(&b, &["--title", "work", "--parent", &plan]);
    commit_store(&b.0, "plan");
    let merge = git_integration(&a.0, &["merge", "--no-edit", "b"]);
    assert!(
        merge.status.success(),
        "{}",
        String::from_utf8_lossy(&merge.stderr)
    );
    // The Completed Group has no condition, so it is named only as an unadopted ancestor.
    let show = a.ok(&["show", &plan]);
    assert!(
        show.contains(&format!(
            "Stalled\nAncestor must be adopted: {done}  Group  Completed  done\n"
        )),
        "{show}"
    );
    assert!(!show.contains("Unsurfaced ancestor:"), "{show}");
    assert!(!show.contains("Parent:"), "{show}");
    // The Issue still does not surface below the terminal ancestor, which is named as the
    // reason instead of as an unsurfaced ancestor.
    let show = a.ok(&["show", &work]);
    assert!(
        show.starts_with(&format!("{work}  Issue  Unsurfaced  work\n")),
        "{show}"
    );
    assert!(
        show.contains(&format!(
            "Required to start\nAncestor must be adopted: {done}  Group  Completed  done\n"
        )),
        "{show}"
    );
    assert!(!show.contains("Unsurfaced ancestor:"), "{show}");
}

#[test]
fn an_issue_below_a_group_whose_parent_registration_is_missing_names_that_ancestor() {
    let f = Fixture::new();
    f.init();
    let capture = |args: &[&str]| -> String {
        f.ok(&[&["capture", "--accept"], args].concat())
            .split_whitespace()
            .next()
            .unwrap()
            .into()
    };
    let outer = capture(&["--kind", "group", "--title", "outer"]);
    let plan = capture(&["--kind", "group", "--title", "plan", "--parent", &outer]);
    let work = capture(&["--title", "work", "--parent", &plan]);
    // The outer Group's records vanish, as a cherry-pick or revert of its files leaves them.
    for relative in record_paths_of(&f.0, &outer) {
        fs::remove_file(f.records_dir().join(relative)).unwrap();
    }
    let show = f.ok(&["show", &work]);
    assert!(
        show.starts_with(&format!("{work}  Issue  Blocked  work\n")),
        "{show}"
    );
    assert!(
        show.contains(&format!(
            "Required to start\nAncestor must be adopted: {outer}  (missing)\n"
        )),
        "{show}"
    );
    let show = f.ok(&["show", &plan]);
    assert!(
        show.contains(&format!(
            "Stalled\nAncestor must be adopted: {outer}  (missing)\n"
        )),
        "{show}"
    );
    // The parent line is left to the reason that already names the missing parent.
    assert!(!show.contains("Parent:"), "{show}");
    let tasks = f.ok(&["tasks"]);
    assert!(
        tasks.contains(&format!("{plan}  Group  Blocked+Invalid  plan"))
            && tasks.contains(&format!("{work}  Issue  Blocked  work")),
        "{tasks}"
    );
}

#[test]
fn resolve_shows_the_violation_the_chosen_head_leaves_behind() {
    let f = Fixture::new();
    git(&f.0, &["init", "-q"]);
    f.init();
    let group = f
        .ok(&["capture", "--kind", "group", "--accept", "--title", "plan"])
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    let x = f.accepted("x");
    commit_store(&f.0, "base");
    let a = add_worktree(&f.0, "a");
    let b = add_worktree(&f.0, "b");
    a.ok(&["complete", &group]);
    a.ok(&["start", &x]);
    commit_store(&a.0, "complete the plan and start x");
    b.ok(&["parent", "set", &x, "--parent", &group]);
    commit_store(&b.0, "move x into the plan");
    assert!(
        git_integration(&a.0, &["merge", "--no-edit", "b"])
            .status
            .success()
    );
    // Taking the move leaves x unfinished under the Completed Group: the confirmation shows
    // the situation with the violation, like every listing does.
    let listing = a.ok(&["resolve", &x]);
    let moved = head_line(&listing, "parent  NotStarted")
        .split("  ")
        .next()
        .unwrap()
        .to_owned();
    assert_eq!(
        a.ok(&["resolve", &x, "--head", &moved]),
        format!("{x}  Resolved: {moved}  Blocked+Invalid\n")
    );
    let error = failure(a.run(&["storage", "check"]));
    assert!(
        error.contains(&format!("Violation: {x}  Issue  Blocked+Invalid  x")),
        "{error}"
    );
    a.ok(&["reopen", &group]);
    consistent(&a);
}

/// A clone whose checkout converts line endings, as Git for Windows does by default.
fn clone_with_autocrlf(source: &Path) -> Fixture {
    let clone = Fixture::new();
    git(
        source,
        &[
            "clone",
            "-q",
            "-c",
            "core.autocrlf=true",
            source.to_str().unwrap(),
            clone.0.to_str().unwrap(),
        ],
    );
    clone
}
fn eol_lines(root: &Path) -> String {
    String::from_utf8(git_output(root, &["ls-files", "--eol", "--", ".axon"]).stdout).unwrap()
}

#[test]
fn the_attributes_file_keeps_a_checkout_with_autocrlf_from_corrupting_the_store() {
    let (f, x) = tracked_repository("x");
    let clone = clone_with_autocrlf(&f.0);
    let eol = eol_lines(&clone.0);
    assert!(eol.contains(".axon/.gitattributes"), "{eol}");
    assert!(
        eol.lines()
            .all(|line| line.contains("w/lf") && line.contains("attr/-text")),
        "{eol}"
    );
    assert!(clone.ok(&["list"]).contains(&x));
    consistent(&clone);
    // Records written in the clone stay readable there.
    clone.ok(&["start", &x]);
    consistent(&clone);
}

#[test]
fn a_tracked_store_without_the_attributes_file_reads_as_corrupt_with_the_line_ending_hint() {
    let f = Fixture::new();
    git(&f.0, &["init", "-q"]);
    f.init();
    fs::remove_file(f.0.join(".axon/.gitattributes")).unwrap();
    let x = f.accepted("x");
    f.accepted("y");
    commit_store(&f.0, "base");
    let clone = clone_with_autocrlf(&f.0);
    let eol = eol_lines(&clone.0);
    assert_eq!(
        eol.lines()
            .filter(|line| line.contains("w/crlf") && line.contains(".axon/records/"))
            .count(),
        2,
        "{eol}"
    );
    let files = clone.record_files();
    let relative = files[0].clone();
    let converted = fs::read(clone.records_dir().join(&relative)).unwrap();
    assert!(converted.ends_with(b"\r\n"));
    // Every read and write stops as corruption; the report names the file, says that the
    // content is the record with CRLF line endings, and points at the guide.
    for args in [
        vec!["list"],
        vec!["show", &x],
        vec!["start", &x],
        vec!["note", "add", &x, "-m", "rejected"],
        vec!["storage", "check"],
    ] {
        let error = failure(clone.run(&args));
        for text in [
            "corrupt",
            relative.to_str().unwrap(),
            "CRLF line endings; with LF the content hashes to the name",
            "Git line-ending conversion is likely",
            "docs/guide/storage.md",
        ] {
            assert!(
                error.contains(text),
                "{args:?}: missing {text:?} in {error}"
            );
        }
    }
    // Every converted file is listed with the reason; the hint follows once, at the end.
    let error = failure(clone.run(&["storage", "check"]));
    assert!(error.contains("2 corrupt files"), "{error}");
    for relative in &files {
        assert!(
            error.contains(&format!("Corrupt: {}: CRLF", relative.display())),
            "{error}"
        );
    }
    assert_eq!(
        error
            .matches("Git line-ending conversion is likely")
            .count(),
        1
    );
    assert!(error.trim_end().ends_with("the details"), "{error}");
    assert!(error.contains("is missing from the store"), "{error}");
    assert_eq!(
        fs::read(clone.records_dir().join(&relative)).unwrap(),
        converted,
        "the reader does not repair the file"
    );
    // The guide's repair: commit the attributes file alone, then take the tracked files out of
    // the index again.
    fs::write(clone.0.join(".axon/.gitattributes"), "* -text\n").unwrap();
    git(&clone.0, &["add", ".axon/.gitattributes"]);
    git_commit(
        &clone.0,
        &[
            "-qm",
            "Stop line-ending conversion",
            "--",
            ".axon/.gitattributes",
        ],
    );
    let tracked =
        String::from_utf8(git_output(&clone.0, &["ls-files", "--", ".axon"]).stdout).unwrap();
    for path in tracked.lines() {
        fs::remove_file(clone.0.join(path)).unwrap();
    }
    git(&clone.0, &["checkout", "--", ".axon"]);
    let eol = eol_lines(&clone.0);
    assert!(eol.lines().all(|line| line.contains("w/lf")), "{eol}");
    assert!(clone.ok(&["list"]).contains(&x));
    consistent(&clone);
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
    // its own, which checks its own index and then looks at the main worktree. Reads check the
    // store's index once, writes twice.
    for (worktree, rev_parse, own_index) in [(&f.0, 1, 0), (&linked.0, 2, 1)] {
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
                rev_parse + own_index + unmerged,
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
                own_index + unmerged,
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
