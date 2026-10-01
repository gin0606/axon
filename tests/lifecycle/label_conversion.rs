//! The one-time conversion of an `axon-records/v1` store to the labelled format, against stores
//! this checkout's binary wrote and turned back into their v1 form.
use super::*;
use axon_label_conversion::{Labels, convert, read_files, run, verify};

fn capture(f: &Fixture, label: &str, args: &[&str]) -> String {
    let mut all = vec!["capture", "--label", label];
    all.extend_from_slice(args);
    f.ok(&all).split_whitespace().next().unwrap().to_string()
}
fn copy(from: &Fixture) -> Fixture {
    let copy = Fixture::new();
    fs::create_dir(copy.0.join(".axon")).unwrap();
    for name in ["header.json", ".gitignore", ".gitattributes"] {
        fs::copy(
            from.0.join(".axon").join(name),
            copy.0.join(".axon").join(name),
        )
        .unwrap();
    }
    merge_records(&from.0, &copy.0);
    copy
}
/// Every file under `.axon/` but the locks, by relative path.
fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let base = root.join(".axon");
    let mut files = BTreeMap::new();
    let mut pending = vec![base.clone()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_none_or(|extension| extension != "lock") {
                files.insert(
                    path.strip_prefix(&base).unwrap().to_path_buf(),
                    fs::read(&path).unwrap(),
                );
            }
        }
    }
    files
}
fn record_bytes(root: &Path) -> BTreeMap<String, Vec<u8>> {
    record_files(root)
        .into_iter()
        .map(|relative| {
            let bytes = fs::read(root.join(".axon/records").join(&relative)).unwrap();
            (
                relative.file_name().unwrap().to_string_lossy().into_owned(),
                bytes,
            )
        })
        .collect()
}

/// Writes the v1 form of the v2 store `from` to `to`: every record without its label, with
/// parents and chosen heads pointing at the v1 IDs, and the earlier header format. Notes keep
/// their bytes. Returns each v2 record ID's v1 ID.
fn downgrade(from: &Path, to: &Path) -> BTreeMap<String, String> {
    let records = record_bytes(from);
    let mut ids: BTreeMap<String, String> = BTreeMap::new();
    let mut pending: Vec<&String> = records.keys().collect();
    fs::create_dir_all(to.join(".axon/records")).unwrap();
    while !pending.is_empty() {
        let before = pending.len();
        pending.retain(|id| {
            let text = String::from_utf8(records[*id].clone()).unwrap();
            let row: serde_json::Value = serde_json::from_str(&text).unwrap();
            let mut earlier = text.clone();
            if row["record"] != "note" {
                let parents: Vec<String> = row["parents"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|parent| parent.as_str().unwrap().to_string())
                    .collect();
                if !parents.iter().all(|parent| ids.contains_key(parent)) {
                    return true;
                }
                let label = format!(",\"label\":\"{}\"", row["after"]["label"].as_str().unwrap());
                let at = earlier.rfind(&label).unwrap();
                earlier.replace_range(at..at + label.len(), "");
                // The parents stay in ID order, which the earlier IDs change.
                let start = earlier.find("\"parents\":[").unwrap();
                let end = start + earlier[start..].find(']').unwrap() + 1;
                let mapped: BTreeSet<&String> = parents.iter().map(|parent| &ids[parent]).collect();
                earlier.replace_range(
                    start..end,
                    &format!("\"parents\":{}", serde_json::to_string(&mapped).unwrap()),
                );
                for parent in &parents {
                    earlier = earlier.replace(parent, &ids[parent]);
                }
            }
            let bytes = earlier.into_bytes();
            let earlier_id = blake3::hash(&bytes).to_hex().to_string();
            let directory = to.join(".axon/records").join(&earlier_id[..2]);
            fs::create_dir_all(&directory).unwrap();
            fs::write(directory.join(&earlier_id), &bytes).unwrap();
            ids.insert((*id).clone(), earlier_id);
            false
        });
        assert!(pending.len() < before);
    }
    let header = fs::read_to_string(from.join(".axon/header.json")).unwrap();
    fs::write(
        to.join(".axon/header.json"),
        header.replace("axon-records/v2", "axon-records/v1"),
    )
    .unwrap();
    for name in [".gitignore", ".gitattributes"] {
        fs::copy(from.join(".axon").join(name), to.join(".axon").join(name)).unwrap();
    }
    ids
}

/// A store with every record kind the CLI writes, Notes, a description that needs escaping and
/// a resolved conflict. `unlisted` is Cancelled with the label `chore` and has no line in the
/// labels this returns.
fn labelled_store() -> (Fixture, String, String) {
    let f = Fixture::new();
    f.init();
    let group = capture(
        &f,
        "feat",
        &["--kind", "group", "--accept", "--title", "plan"],
    );
    let needed = capture(&f, "docs", &["--accept", "--title", "needed"]);
    let issue = capture(
        &f,
        "bug",
        &[
            "--accept",
            "--title",
            "fix",
            "--parent",
            &group,
            "-m",
            "a \"quoted\" line,\"condition\":\nand 日本語",
        ],
    );
    f.ok(&["dep", "add", &issue, "--needs", &needed]);
    f.ok(&["start", &needed]);
    f.ok(&["complete", &needed]);
    f.ok(&["start", &issue]);
    f.ok(&["note", "add", &issue, "-m", "started"]);
    f.ok(&["write", &issue, "--title", "fix it"]);
    f.ok(&["condition", "set", &issue, "--command", "exit 0"]);
    f.ok(&["condition", "unset", &issue]);
    let question = capture(&f, "spike", &["--title", "question"]);
    f.ok(&["convert", &question, "--kind", "group"]);
    let unlisted = capture(&f, "chore", &["--title", "dropped"]);
    f.ok(&["cancel", &unlisted, "-r", "not needed"]);
    let other = copy(&f);
    f.ok(&["complete", &issue]);
    other.ok(&["release", &issue, "-r", "remaining work"]);
    other.ok(&["note", "add", &issue, "-m", "released"]);
    merge_records(&other.0, &f.0);
    let listing = f.ok(&["resolve", &issue]);
    let completed = listing
        .lines()
        .find(|line| line.contains("Completed"))
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    f.ok(&["resolve", &issue, "--head", &completed, "-r", "finished"]);
    let labels =
        format!("# labels\n{group} feat\n\n{needed} docs\n{issue} bug\n{question}  spike\n");
    (f, labels, unlisted)
}

#[test]
fn converting_the_v1_form_of_a_store_rebuilds_its_v2_bytes() {
    let (f, labels, unlisted) = labelled_store();
    success(f.run(&["storage", "check"]));
    let v1 = Fixture::new();
    let v1_ids = downgrade(&f.0, &v1.0);
    let before = snapshot(&v1.0);
    let failure = failure(v1.run(&["storage", "check"]));
    assert!(failure.contains("needs conversion"), "{failure}");

    let out = Fixture::new();
    let output = out.0.join("converted");
    let summary = run(&v1.0, &Labels::parse(&labels).unwrap(), &output).unwrap();
    assert_eq!(snapshot(&v1.0), before, "the input store changed");
    assert!(!output.join(axon_label_conversion::UNCHECKED).exists());
    assert_eq!(summary.notes, 2);
    assert_eq!(summary.entities, 5);
    // The same bytes and IDs the binary wrote, header included.
    assert_eq!(snapshot(&output), snapshot(&f.0));
    let converted = convert(
        &read_files(&v1.0).unwrap(),
        &Labels::parse(&labels).unwrap(),
    )
    .unwrap();
    for (v2, v1) in &v1_ids {
        assert_eq!(
            converted.ids[&record::RecordId::try_from(v1.as_str()).unwrap()].as_ref(),
            v2
        );
    }
    let check = command(&output)
        .args(["storage", "check"])
        .output()
        .unwrap();
    assert!(check.status.success());
    assert!(String::from_utf8_lossy(&check.stderr).contains("consistent"));
    let list = success(command(&output).arg("list").output().unwrap());
    assert!(
        list.contains(&format!("{unlisted}  Issue  Cancelled  chore  dropped")),
        "{list}"
    );
    assert!(list.contains("  Completed  bug  fix it"), "{list}");
}

#[test]
fn conflicts_and_violations_are_converted_as_they_are() {
    let f = Fixture::new();
    f.init();
    let group = capture(
        &f,
        "chore",
        &["--kind", "group", "--accept", "--title", "g"],
    );
    let open = capture(&f, "feat", &["--accept", "--title", "open"]);
    let closed = capture(&f, "chore", &["--accept", "--title", "closed"]);
    let mixed = capture(&f, "bug", &["--accept", "--title", "mixed"]);
    let other = copy(&f);
    f.ok(&["cancel", &group]);
    other.ok(&[
        "capture", "--label", "feat", "--accept", "--title", "late", "--parent", &group,
    ]);
    f.ok(&["write", &open, "--title", "one side"]);
    other.ok(&["write", &open, "--title", "other side"]);
    f.ok(&["cancel", &closed]);
    other.ok(&["start", &closed]);
    other.ok(&["complete", &closed]);
    f.ok(&["start", &mixed]);
    f.ok(&["complete", &mixed]);
    other.ok(&["start", &mixed]);
    merge_records(&other.0, &f.0);
    let late = f
        .ok(&["list", "--search", "late"])
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    let report = failure(f.run(&["storage", "check"]));
    assert!(report.contains(&format!("Conflicted: {open}")), "{report}");
    assert!(
        report.contains(&format!("Conflicted: {closed}")),
        "{report}"
    );
    assert!(report.contains(&late), "{report}");

    let v1 = Fixture::new();
    downgrade(&f.0, &v1.0);
    // Every head of `closed` is terminal, so it needs no line; `open` has no terminal head and
    // `mixed` has one Completed and one InProgress head.
    let labels = Labels::parse(&format!("{late} feat\n{group} chore\n")).unwrap();
    let error = run(&v1.0, &labels, &v1.0.join("out")).unwrap_err().0;
    assert!(
        error.contains(&open) && error.contains(&mixed) && !error.contains(&closed),
        "{error}"
    );
    assert!(!v1.0.join("out").exists());
    // A conflicted Entity takes its one label on every head: `closed` gets `chore` on both.
    let labels = Labels::parse(&format!("{late} feat\n{open} feat\n{mixed} bug\n")).unwrap();
    let output = v1.0.join("out");
    run(&v1.0, &labels, &output).unwrap();
    assert_eq!(snapshot(&output), snapshot(&f.0));
    let converted = command(&output)
        .args(["storage", "check"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8(converted.stderr).unwrap(), report);
}

#[test]
fn an_entity_reopened_after_completion_needs_a_label() {
    let f = Fixture::new();
    f.init();
    let id = capture(&f, "chore", &["--accept", "--title", "again"]);
    f.ok(&["start", &id]);
    f.ok(&["complete", &id]);
    f.ok(&["reopen", &id, "-r", "not done"]);
    let v1 = Fixture::new();
    downgrade(&f.0, &v1.0);
    let error = refused(&v1, &Labels::default());
    assert!(
        error.contains("not terminal") && error.contains(&id),
        "{error}"
    );
    let labels = Labels::parse(&format!("{id} chore\nt-elsewhere feat")).unwrap();
    let summary = run(&v1.0, &labels, &v1.0.join("out")).unwrap();
    assert_eq!(
        summary.unused,
        vec![EntityId::try_from("t-elsewhere".to_string()).unwrap()]
    );
}

#[test]
fn stores_sharing_records_give_them_the_same_ids() {
    let (base, labels, _) = labelled_store();
    let labels = Labels::parse(&labels).unwrap();
    let issue = base
        .ok(&["list", "--search", "fix it"])
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    let left = copy(&base);
    let right = copy(&base);
    left.ok(&["reopen", &issue]);
    right.ok(&["note", "add", &issue, "-m", "only on the right"]);
    let (left_v1, right_v1) = (Fixture::new(), Fixture::new());
    downgrade(&left.0, &left_v1.0);
    downgrade(&right.0, &right_v1.0);
    let left_files = read_files(&left_v1.0).unwrap();
    let right_files = read_files(&right_v1.0).unwrap();
    let left_converted = convert(&left_files, &labels).unwrap();
    let right_converted = convert(&right_files, &labels).unwrap();
    let mut shared = 0;
    for (id, new) in &left_converted.ids {
        if let Some(other) = right_converted.ids.get(id) {
            assert_eq!(new, other);
            shared += 1;
        }
    }
    assert!(shared > 0);
    assert_eq!(left_converted.ids.len(), shared + 1);
    assert_eq!(right_converted.ids.len(), shared + 1);
    // Converting twice gives the same bytes.
    assert_eq!(convert(&left_files, &labels).unwrap(), left_converted);
}

#[test]
fn records_the_earlier_encoder_wrote_convert_to_the_current_encoding() {
    const EARLIER: [&str; 3] = [
        r#"{"entity":"i3","record":"created","parents":[],"at":"1970-01-01T00:00:03.500Z","recorder":{"actor":"setup","data":{"session_id":"session-1"}},"reason":null,"after":{"kind":"issue","lifecycle":"not-started","owner":null,"title":"task","description":"body\n日本語","condition":null,"parent":null,"needs":[]}}"#,
        r#"{"entity":"i3","record":"transition","operation":"start","parents":["2a21511cbd45b559ef550bd9595ef3a61babecfcefcd61caa1661b33d61c66ee"],"at":"1970-01-01T00:01:41.500Z","recorder":{"actor":"r0","data":{"session_id":"session-1"}},"reason":null,"after":{"kind":"issue","lifecycle":"in-progress","owner":"r0","title":"task","description":"body\n日本語","condition":null,"parent":null,"needs":[]}}"#,
        r#"{"entity":"i3","record":"edit","parents":["04349af49e27c28e63a4e7b2385be53a85eb07f057e2f97f83405af4d210a674"],"at":"1970-01-01T00:01:42.500Z","recorder":{"actor":"r0","data":{"session_id":"session-1"}},"reason":null,"after":{"kind":"issue","lifecycle":"in-progress","owner":"r0","title":"edited","description":"desc","condition":null,"parent":null,"needs":[]}}"#,
    ];
    const CURRENT: [&str; 3] = [
        r#"{"entity":"i3","record":"created","parents":[],"at":"1970-01-01T00:00:03.500Z","recorder":{"actor":"setup","data":{"session_id":"session-1"}},"reason":null,"after":{"kind":"issue","lifecycle":"not-started","owner":null,"title":"task","description":"body\n日本語","label":"feat","condition":null,"parent":null,"needs":[]}}"#,
        r#"{"entity":"i3","record":"transition","operation":"start","parents":["95e349ab47d8d1cd4f42275d434470347c48efdeb43d4085aeb13338580feec6"],"at":"1970-01-01T00:01:41.500Z","recorder":{"actor":"r0","data":{"session_id":"session-1"}},"reason":null,"after":{"kind":"issue","lifecycle":"in-progress","owner":"r0","title":"task","description":"body\n日本語","label":"feat","condition":null,"parent":null,"needs":[]}}"#,
        r#"{"entity":"i3","record":"edit","parents":["8b9bfb377789bac516817020a2ae94088a3717679873a39e490295cac2a64a6b"],"at":"1970-01-01T00:01:42.500Z","recorder":{"actor":"r0","data":{"session_id":"session-1"}},"reason":null,"after":{"kind":"issue","lifecycle":"in-progress","owner":"r0","title":"edited","description":"desc","label":"feat","condition":null,"parent":null,"needs":[]}}"#,
    ];
    let v1 = Fixture::new();
    fs::create_dir_all(v1.0.join(".axon/records")).unwrap();
    fs::write(
        v1.0.join(".axon/header.json"),
        // The earlier reader took a header with whitespace too.
        "{\"format\":\"axon-records/v1\", \"store\":\"store-1\",\"prefix\":\"i\"}\n",
    )
    .unwrap();
    for line in EARLIER {
        let bytes = format!("{line}\n");
        let id = blake3::hash(bytes.as_bytes()).to_hex().to_string();
        fs::create_dir_all(v1.0.join(".axon/records").join(&id[..2])).unwrap();
        fs::write(v1.0.join(".axon/records").join(&id[..2]).join(&id), bytes).unwrap();
    }
    let files = read_files(&v1.0).unwrap();
    // The InProgress Issue needs a line.
    let error = convert(&files, &Labels::default()).unwrap_err().0;
    assert!(
        error.contains("not terminal") && error.contains("i3  in-progress  edited"),
        "{error}"
    );
    let converted = convert(&files, &Labels::parse("i3 feat").unwrap()).unwrap();
    let written: BTreeSet<Vec<u8>> = converted.files.records.values().cloned().collect();
    let expected: BTreeSet<Vec<u8>> = CURRENT
        .iter()
        .map(|line| format!("{line}\n").into_bytes())
        .collect();
    assert_eq!(written, expected);
    assert_eq!(
        converted.files.header,
        b"{\"format\":\"axon-records/v2\",\"store\":\"store-1\",\"prefix\":\"i\"}\n"
    );
}

/// A small v1 store and its labels.
fn earlier_store() -> (Fixture, Fixture, Labels) {
    let f = Fixture::new();
    f.init();
    let id = capture(&f, "feat", &["--accept", "--title", "open"]);
    f.ok(&["start", &id]);
    f.ok(&["note", "add", &id, "-m", "a note"]);
    let v1 = Fixture::new();
    downgrade(&f.0, &v1.0);
    (f, v1, Labels::parse(&format!("{id} feat")).unwrap())
}
/// Runs the conversion expecting a refusal that leaves the input unchanged and writes nothing.
fn refused(v1: &Fixture, labels: &Labels) -> String {
    let before = snapshot(&v1.0);
    let output = v1.0.join("out");
    let error = run(&v1.0, labels, &output).unwrap_err().0;
    assert_eq!(snapshot(&v1.0), before);
    assert!(!output.exists());
    error
}
fn record_of(root: &Path, record: &str) -> PathBuf {
    record_files(root)
        .into_iter()
        .map(|relative| root.join(".axon/records").join(relative))
        .find(|path| fs::read_to_string(path).unwrap().contains(record))
        .unwrap()
}

#[test]
fn corrupt_stores_are_refused() {
    // Bytes whose hash is not the name.
    let (_, v1, labels) = earlier_store();
    let path = record_of(&v1.0, "\"record\":\"note\"");
    fs::write(&path, "{}\n").unwrap();
    let error = refused(&v1, &labels);
    assert!(
        error.contains("corrupt") && error.contains("hash"),
        "{error}"
    );

    // Bytes that are not canonical, under the name of their hash.
    let (_, v1, labels) = earlier_store();
    let path = record_of(&v1.0, "\"operation\":\"start\"");
    let spaced = fs::read_to_string(&path).unwrap().replacen(",", ", ", 1);
    fs::remove_file(&path).unwrap();
    let id = blake3::hash(spaced.as_bytes()).to_hex().to_string();
    let directory = v1.0.join(".axon/records").join(&id[..2]);
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join(&id), spaced).unwrap();
    let error = refused(&v1, &labels);
    assert!(error.contains("corrupt") && error.contains(&id), "{error}");

    // A record that already carries a label.
    let (f, v1, labels) = earlier_store();
    fs::remove_dir_all(v1.0.join(".axon/records")).unwrap();
    merge_records(&f.0, &v1.0);
    let error = refused(&v1, &labels);
    assert!(error.contains("carries a label"), "{error}");

    // A header that is not v1.
    let (f, _, labels) = earlier_store();
    let error = refused(&f, &labels);
    assert!(error.contains("needs no conversion"), "{error}");
}

#[test]
fn a_store_with_missing_records_is_refused() {
    let (_, v1, labels) = earlier_store();
    fs::remove_file(record_of(&v1.0, "\"record\":\"created\"")).unwrap();
    let error = refused(&v1, &labels);
    assert!(
        error.contains("missing") && error.contains("gap"),
        "{error}"
    );
}

#[test]
fn a_store_with_unmerged_paths_is_refused() {
    let (_, v1, labels) = earlier_store();
    git(&v1.0, &["init", "-q", "-b", "main"]);
    git(&v1.0, &["add", ".axon"]);
    git_commit(&v1.0, &["-qm", "store"]);
    git(&v1.0, &["checkout", "-qb", "side"]);
    let conflicting = v1.0.join(".axon/records/00");
    fs::create_dir_all(&conflicting).unwrap();
    fs::write(conflicting.join("x"), "side\n").unwrap();
    git(&v1.0, &["add", ".axon"]);
    git_commit(&v1.0, &["-qm", "side"]);
    git(&v1.0, &["checkout", "-q", "main"]);
    fs::create_dir_all(&conflicting).unwrap();
    fs::write(conflicting.join("x"), "main\n").unwrap();
    git(&v1.0, &["add", ".axon"]);
    git_commit(&v1.0, &["-qm", "main"]);
    assert!(
        !git_integration(&v1.0, &["merge", "-q", "side"])
            .status
            .success()
    );
    let error = refused(&v1, &labels);
    assert!(
        error.contains("unmerged") && error.contains(".axon/records/00/x"),
        "{error}"
    );
}

#[test]
fn the_output_must_be_a_new_store() {
    let (_, v1, labels) = earlier_store();
    fs::create_dir_all(v1.0.join("out/.axon")).unwrap();
    let error = run(&v1.0, &labels, &v1.0.join("out")).unwrap_err().0;
    assert!(error.contains("already exists"), "{error}");
    // A store left unchecked by an interrupted run is neither used nor removed.
    let leftover = v1.0.join("other").join(axon_label_conversion::UNCHECKED);
    fs::create_dir_all(&leftover).unwrap();
    let error = run(&v1.0, &labels, &v1.0.join("other")).unwrap_err().0;
    assert!(error.contains("unchecked store"), "{error}");
    assert!(leftover.exists() && !v1.0.join("other/.axon").exists());
}

/// `converted` with the record whose bytes contain `marker` replaced by `edit` of its bytes.
fn tampered(
    converted: &axon_label_conversion::Converted,
    marker: &str,
    edit: impl Fn(String) -> String,
) -> axon_label_conversion::Converted {
    let mut tampered = converted.clone();
    let (old, new) = converted
        .ids
        .iter()
        .find(|(_, new)| String::from_utf8_lossy(&converted.files.records[*new]).contains(marker))
        .unwrap();
    let bytes = edit(String::from_utf8(converted.files.records[new].clone()).unwrap()).into_bytes();
    let id = record::RecordId::of(&bytes);
    tampered.files.records.remove(new);
    tampered.files.records.insert(id.clone(), bytes);
    tampered.ids.insert(old.clone(), id);
    tampered
}

#[test]
fn verification_finds_a_record_that_changed_beyond_its_label() {
    let (_, v1, labels) = earlier_store();
    let input = read_files(&v1.0).unwrap();
    let converted = convert(&input, &labels).unwrap();
    verify(&input, &labels, &converted, &converted.files).unwrap();
    let created = converted
        .ids
        .values()
        .find(|id| String::from_utf8_lossy(&converted.files.records[*id]).contains("\"created\""))
        .unwrap()
        .to_string();
    let note = converted
        .ids
        .values()
        .find(|id| String::from_utf8_lossy(&converted.files.records[*id]).contains("\"note\""))
        .unwrap()
        .to_string();
    let start = "\"operation\":\"start\"";
    for (marker, edit, expected) in [
        (
            start,
            Box::new(|text: String| text.replace("\"title\":\"open\"", "\"title\":\"changed\""))
                as Box<dyn Fn(String) -> String>,
            "other than the label",
        ),
        (
            start,
            Box::new(|text: String| text.replace("\"label\":\"feat\"", "\"label\":\"bug\"")),
            "the label is not feat",
        ),
        (
            start,
            Box::new(|text: String| text.replace(&created, &note)),
            "parents do not map",
        ),
        (
            "\"note\"",
            Box::new(|text: String| text.replace("a note", "another note")),
            "a Note changed",
        ),
    ] {
        let tampered = tampered(&converted, marker, edit);
        let error = verify(&input, &labels, &tampered, &tampered.files)
            .unwrap_err()
            .0;
        assert!(error.contains(expected), "{expected}: {error}");
    }
    // The labels file is checked too, not the labels the conversion chose.
    let other = Labels::parse(&format!("{} bug", converted.labels.keys().next().unwrap())).unwrap();
    let error = verify(&input, &other, &converted, &converted.files)
        .unwrap_err()
        .0;
    assert!(error.contains("the label is not bug"), "{error}");
}

#[test]
fn notes_of_an_entity_without_records_are_a_gap() {
    let (_, v1, labels) = earlier_store();
    for path in record_files(&v1.0) {
        let path = v1.0.join(".axon/records").join(path);
        if !fs::read_to_string(&path).unwrap().contains("\"note\"") {
            fs::remove_file(path).unwrap();
        }
    }
    let error = refused(&v1, &labels);
    assert!(
        error.contains("gap") && error.contains("has Notes"),
        "{error}"
    );
}

#[test]
fn the_output_may_not_lie_in_the_source_store() {
    let (_, v1, labels) = earlier_store();
    for output in [
        v1.0.clone(),
        v1.0.join(".axon"),
        v1.0.join(".axon/records/zz"),
    ] {
        let before = snapshot(&v1.0);
        let error = run(&v1.0, &labels, &output).unwrap_err().0;
        assert!(error.contains("inside the source store"), "{error}");
        assert_eq!(snapshot(&v1.0), before);
    }
}

#[test]
fn a_symlinked_record_directory_is_refused() {
    let (_, v1, labels) = earlier_store();
    let elsewhere = Fixture::new();
    fs::rename(v1.0.join(".axon/records"), elsewhere.0.join("records")).unwrap();
    std::os::unix::fs::symlink(elsewhere.0.join("records"), v1.0.join(".axon/records")).unwrap();
    let error = refused(&v1, &labels);
    assert!(error.contains("not a directory"), "{error}");
}

#[test]
fn a_git_that_cannot_answer_stops_the_conversion() {
    let (_, v1, labels) = earlier_store();
    fs::write(v1.0.join(".git"), "gitdir: /nonexistent\n").unwrap();
    let error = refused(&v1, &labels);
    assert!(error.contains("cannot check the Git index"), "{error}");
}
