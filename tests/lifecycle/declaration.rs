use super::*;
use axon::declaration::{self, Reference};
fn created(text: &str) -> String {
    text.split_whitespace().next().unwrap().into()
}
fn snapshot(f: &Fixture) -> Store {
    f.records()
}
fn view_of(records: &Store) -> record::View {
    records.view().unwrap()
}
#[test]
fn declaration_export_selectors_and_references_are_read_only() {
    let f = Fixture::new();
    f.ok(&["init", "demo"]);
    let outer = created(&f.ok(&[
        "capture", "--label", "chore", "--kind", "group", "--accept", "--title", "Outer",
    ]));
    let group = created(&f.ok(&[
        "capture", "--label", "chore", "--kind", "group", "--accept", "--title", "Plan",
        "--parent", &outer,
    ]));
    let external = f.accepted("Outside");
    let done = created(&f.ok(&[
        "capture", "--label", "chore", "--accept", "--title", "Finished", "--parent", &group,
    ]));
    let cancelled = created(&f.ok(&[
        "capture",
        "--label",
        "chore",
        "--accept",
        "--title",
        "Cancelled",
        "--parent",
        &group,
    ]));
    let nested = created(&f.ok(&[
        "capture", "--label", "chore", "--kind", "group", "--accept", "--title", "Nested",
        "--parent", &group,
    ]));
    let child = created(&f.ok(&[
        "capture",
        "--label",
        "chore",
        "--accept",
        "--title",
        "Body",
        "-m",
        "\n indented\r\nwith control\u{1}\nlast\n\n",
        "--parent",
        &nested,
        "--needs",
        &external,
        "--command",
        "touch executed",
    ]));
    f.ok(&["start", &done]);
    f.ok(&["complete", &done]);
    f.ok(&["cancel", &cancelled]);
    let before = snapshot(&f);
    let file_before = f.record_files();
    let output = f.run(&["export", group.rsplit('-').next().unwrap()]);
    assert!(output.stderr.is_empty());
    let yaml = success(output);
    let d = declaration::parse(&yaml).unwrap();
    assert_eq!(d.serialize(&view_of(&before)).unwrap(), yaml);
    assert_eq!(d.groups.len(), 2);
    assert_eq!(d.issues.len(), 3);
    // Groups export their saved lifecycle, not the derived InProgress.
    assert!(d.groups.iter().all(|r| r.lifecycle == "not-started"));
    assert!(
        f.ok(&["list", "--lifecycle", "in-progress"])
            .contains(&group)
    );
    assert_eq!(
        d.issues
            .iter()
            .find(|r| r.id.as_ref() == Some(&done))
            .unwrap()
            .lifecycle,
        "completed"
    );
    assert_eq!(
        d.issues
            .iter()
            .find(|r| r.id.as_ref() == Some(&cancelled))
            .unwrap()
            .lifecycle,
        "cancelled"
    );
    assert_eq!(
        d.references
            .iter()
            .map(|r| r.id.clone())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([outer.clone(), external.clone()])
    );
    assert!(d.records().all(|r| r.key.is_none() && r.base.is_some()));
    let single = declaration::parse(&f.ok(&["export", &child])).unwrap();
    assert!(single.groups.is_empty());
    assert_eq!(single.issues.len(), 1);
    assert_eq!(single.issues[0].parent, Some(Reference::id(&nested)));
    assert_eq!(single.issues[0].title, "Body");
    assert_eq!(
        single.issues[0].description,
        "\n indented\r\nwith control\u{1}\nlast\n\n"
    );
    assert_eq!(
        single
            .references
            .iter()
            .map(|r| r.id.clone())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([nested.clone(), external.clone()])
    );
    let union = declaration::parse(&f.ok(&["export", &child, &external, &group, &group])).unwrap();
    assert_eq!(union.groups.len(), 2);
    assert_eq!(union.issues.len(), 4);
    assert_eq!(
        union.references.iter().map(|r| &r.id).collect::<Vec<_>>(),
        vec![&outer]
    );
    assert!(failure(f.run(&["export", &group, "missing"])).contains("missing"));
    failure(f.run(&["export"]));
    assert_eq!(snapshot(&f), before);
    assert_eq!(f.record_files(), file_before);
    assert!(!f.0.join("executed").exists());
}
#[test]
fn declaration_docs_and_template_work_with_broken_management_root() {
    let f = Fixture::new();
    fs::write(f.0.join(".git"), "broken marker").unwrap();
    fs::create_dir(f.0.join(".axon")).unwrap();
    fs::write(f.0.join(".axon/header.json"), "broken storage").unwrap();
    let docs = f
        .ok(&["docs", "declaration"])
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    for phrase in [
        "The strict YAML schema is axon-declaration/v2.",
        "Every record has id, key, base, lifecycle, title, description, label, parent and needs.",
        "Existing records have a full id and a read-only base fingerprint and lifecycle.",
        "New records have id: null, base: null, a unique key, and lifecycle: undecided or not-started.",
        "References use { id: FULL-ID } or { key: local-key }.",
        "The label is required and one of bug, feat, chore, docs, test, refactor or spike",
        "axon import prepare plan.yaml axon import check plan.yaml axon import apply plan.yaml",
        "Entities not listed in groups or issues are untouched.",
        "the file does not delete or cancel it, detach it from its parent, or remove its dependencies.",
        "To detach an Entity from a Group, write parent: null in its record.",
        "Use needs: [] for no outgoing dependencies.",
    ] {
        assert!(docs.contains(phrase), "{phrase}");
    }
    let output = f.run(&["docs", "declaration", "--example"]);
    assert!(output.stderr.is_empty());
    let yaml = success(output);
    assert!(yaml.starts_with("schema: axon-declaration/v2\n"), "{yaml}");
    assert_eq!(yaml.matches("\n    label: feat\n").count(), 3, "{yaml}");
    let d = declaration::parse(&yaml).unwrap();
    assert_eq!(d, declaration::example());
    assert_eq!(d.serialize(&view_of(&Store::new())).unwrap(), yaml);
    assert!(f.ok(&["docs"]).contains("docs declaration"));
    assert!(f.ok(&["import", "--help"]).contains("prepare"));
    assert!(f.ok(&["import", "check", "--help"]).contains("<FILE>"));
    assert!(f.ok(&["export", "--help"]).contains("<ID>..."));
    assert!(
        f.ok(&["docs", "declaration", "--help"])
            .contains("--example")
    );
}

#[test]
fn declaration_prepare_check_new_plan_and_existing_changes() {
    let f = Fixture::new();
    f.ok(&["init", "demo"]);
    let before = snapshot(&f);
    let path = f.0.join("plan.yaml");
    fs::write(&path, f.ok(&["docs", "declaration", "--example"])).unwrap();
    let args = ["import", "prepare", path.to_str().unwrap()];
    let prepared_output = f.ok(&args);
    let bytes = fs::read(&path).unwrap();
    let d = declaration::parse(std::str::from_utf8(&bytes).unwrap()).unwrap();
    assert!(
        d.records()
            .all(|r| r.id.is_some() && r.base.is_none() && r.key.is_some())
    );
    for record in d.records() {
        let mapping = format!(
            "{} -> {}",
            record.key.as_ref().unwrap(),
            record.id.as_ref().unwrap()
        );
        assert!(prepared_output.lines().any(|line| line == mapping));
    }
    assert_eq!(f.ok(&args), prepared_output);
    assert_eq!(bytes, fs::read(&path).unwrap());
    let text = f.ok(&["import", "check", path.to_str().unwrap()]);
    assert_eq!(text.matches("Create ").count(), 3);
    assert_eq!(text.matches("\n  label: feat\n").count(), 3, "{text}");
    assert!(text.contains("needs: +"));
    assert!(text.contains("Situation after: Blocked"));
    assert_eq!(before, snapshot(&f));
    assert_eq!(bytes, fs::read(&path).unwrap());
    let existing = created(&f.ok(&[
        "capture",
        "--label",
        "chore",
        "--accept",
        "--title",
        "Original",
        "--command",
        "touch executed",
    ]));
    let original = f.ok(&["export", &existing]);
    fs::write(&path, &original).unwrap();
    assert!(
        f.ok(&["import", "check", path.to_str().unwrap()])
            .contains("No changes")
    );
    fs::write(&path, original.replace("title: Original", "title: Updated")).unwrap();
    assert!(
        f.ok(&["import", "check", path.to_str().unwrap()])
            .contains("title: Original -> Updated")
    );
    assert!(!f.0.join("executed").exists());
    f.ok(&["write", &existing, "--title", "Concurrent"]);
    let before = snapshot(&f);
    let bytes = fs::read(&path).unwrap();
    assert!(failure(f.run(&["import", "check", path.to_str().unwrap()])).contains("conflict:"));
    assert_eq!(before, snapshot(&f));
    assert_eq!(bytes, fs::read(&path).unwrap());
}

#[test]
fn declaration_check_rejections_preserve_storage_and_input() {
    let f = Fixture::new();
    f.ok(&["init", "demo"]);
    let item = f.accepted("Original");
    let external = f.accepted("External");
    let exported = f.ok(&["export", &item]);
    let path = f.0.join("plan.yaml");
    let before = snapshot(&f);
    let mutations = [
        (
            exported.replace("axon-declaration/v2", "wrong/v1"),
            "schema:",
        ),
        (
            exported.replace("axon-declaration/v2", "axon-declaration/v1"),
            "run axon export again",
        ),
        (exported.replace("label: chore", "label: fix"), "schema:"),
        (exported.replace("label: chore", "label: null"), "schema:"),
        (exported.replace("    label: chore\n", ""), "schema:"),
        (
            exported.replace("title: Original", "title: Original\n    unknown: x"),
            "schema:",
        ),
        (
            exported.replace("lifecycle: not-started", "lifecycle: in-progress"),
            "read-only:",
        ),
        (
            exported.replace("needs: []", "needs:\n      - { key: missing }"),
            "identity/reference:",
        ),
        (
            exported.replace("needs: []", &format!("needs:\n      - {{ id: {item} }}")),
            "identity/reference:",
        ),
        (
            exported.replace("needs: []", "needs:\n      - { id: demo-absent }"),
            "identity/reference:",
        ),
        (
            exported.replace(
                "needs: []",
                &format!("needs:\n      - {{ id: {external} }}"),
            ),
            "references",
        ),
        (format!("# noncanonical\n{exported}"), "prepare"),
    ];
    for (input, diagnostic) in mutations {
        fs::write(&path, &input).unwrap();
        let message = failure(f.run(&["import", "check", path.to_str().unwrap()]));
        let apply_message = failure(f.run(&["import", "apply", path.to_str().unwrap()]));
        assert!(apply_message.contains("Not applied:"), "{apply_message}");
        assert!(message.contains(diagnostic), "{diagnostic}: {message}");
        assert_eq!(fs::read_to_string(&path).unwrap(), input);
        assert_eq!(snapshot(&f), before);
    }
}

#[test]
fn declaration_check_rejects_local_and_core_guards() {
    let f = Fixture::new();
    f.ok(&["init", "demo"]);
    let group = created(&f.ok(&[
        "capture", "--label", "chore", "--kind", "group", "--accept", "--title", "Group",
    ]));
    let other = created(&f.ok(&[
        "capture", "--label", "chore", "--kind", "group", "--accept", "--title", "Other",
    ]));
    let a = f.accepted("A");
    let b = f.accepted("B");
    let exported = f.ok(&["export", &a]);
    let path = f.0.join("plan.yaml");
    let reject = |input: String, category: &str| {
        let before = snapshot(&f);
        fs::write(&path, &input).unwrap();
        let message = failure(f.run(&["import", "check", path.to_str().unwrap()]));
        let apply_message = failure(f.run(&["import", "apply", path.to_str().unwrap()]));
        assert!(
            apply_message.contains("Not applied:"),
            "input:\n{input}\n{apply_message}"
        );
        assert!(
            message.contains(category),
            "input:\n{input}\n{category}: {message}"
        );
        assert_eq!(snapshot(&f), before, "input:\n{input}");
        assert_eq!(fs::read_to_string(&path).unwrap(), input, "input:\n{input}");
    };
    for input in [
        exported.replace("title: A", "title: A\n    title: duplicate"),
        exported.replace("title: A", "title: &anchor A"),
        exported.replace("title: A", "title: *alias"),
        exported.replace("title: A", "title: !!str A"),
        exported.replace("title: A", "title: A\n    <<: {}"),
        exported.replace(
            "parent: null",
            &format!("parent: {{ id: {group}, key: alias }}"),
        ),
    ] {
        reject(input, "schema:");
    }
    reject(
        exported.replace(&format!("id: {a}"), "id: null"),
        "identity/reference:",
    );
    let mut fresh = declaration::example();
    fresh.prepare(&snapshot(&f), "demo").unwrap();
    let fresh_yaml = fresh.serialize(&view_of(&snapshot(&f))).unwrap();
    reject(
        fresh_yaml.replace("    key: first\n", "    key: null\n"),
        "identity/reference:",
    );
    reject(
        fresh_yaml.replace("lifecycle: not-started", "lifecycle: completed"),
        "identity/reference:",
    );
    let duplicated = exported.replace(
        "references: []",
        &format!(
            "{}references: []",
            exported
                .split("issues:\n")
                .nth(1)
                .unwrap()
                .split("references:")
                .next()
                .unwrap()
        ),
    );
    reject(duplicated, "identity/reference:");
    reject(
        exported.replace(
            "needs: []",
            &format!("needs:\n      - {{ id: {b} }}\n      - {{ id: {b} }}"),
        ),
        "identity/reference:",
    );
    let mut d = declaration::parse(&exported).unwrap();
    d.groups = std::mem::take(&mut d.issues);
    reject(d.serialize(&view_of(&snapshot(&f))).unwrap(), "read-only:");
    let mut d = declaration::parse(&exported).unwrap();
    d.issues[0].base = Some(format!("blake3:{}", "0".repeat(64)));
    d.issues[0].title = "Edit".into();
    reject(d.serialize(&view_of(&snapshot(&f))).unwrap(), "conflict:");
    let mut d = declaration::parse(&exported).unwrap();
    d.issues[0].parent = Some(Reference::id(&b));
    // Leave references empty to test the storage-side parent guard after regeneration.
    assert!(
        d.prepare(&snapshot(&f), "demo")
            .unwrap_err()
            .to_string()
            .contains("Group")
    );
    let mut d = declaration::parse(&f.ok(&["export", &a, &b])).unwrap();
    d.issues[0].needs.push(Reference::id(&b));
    d.issues[1].needs.push(Reference::id(&a));
    reject(
        d.serialize(&view_of(&snapshot(&f))).unwrap(),
        "core rejection:",
    );
    let mut d = declaration::parse(&f.ok(&["export", &group, &other])).unwrap();
    d.groups[0].parent = Some(Reference::id(&other));
    d.groups[1].parent = Some(Reference::id(&group));
    reject(
        d.serialize(&view_of(&snapshot(&f))).unwrap(),
        "core rejection:",
    );
    f.ok(&["start", &a]);
    f.ok(&["withdraw", &group]);
    let mut d = declaration::parse(&f.ok(&["export", &a])).unwrap();
    d.issues[0].parent = Some(Reference::id(&group));
    d.refresh_references(&view_of(&snapshot(&f))).unwrap();
    reject(
        d.serialize(&view_of(&snapshot(&f))).unwrap(),
        "core rejection:",
    );
    f.ok(&["accept", &group]);
    f.ok(&["complete", &a]);
    f.ok(&["cancel", &b]);
    for item in [&a, &b] {
        for field in ["title", "description"] {
            let mut d = declaration::parse(&f.ok(&["export", item])).unwrap();
            if field == "title" {
                d.issues[0].title = "Edit".into();
            } else {
                d.issues[0].description = "Edit".into();
            }
            reject(
                d.serialize(&view_of(&snapshot(&f))).unwrap(),
                "core rejection:",
            );
        }
    }
    let mut d = declaration::parse(&f.ok(&["export", &a])).unwrap();
    d.issues[0].needs.push(Reference::id(&b));
    d.refresh_references(&view_of(&snapshot(&f))).unwrap();
    reject(
        d.serialize(&view_of(&snapshot(&f))).unwrap(),
        "core rejection:",
    );
    f.ok(&["parent", "set", &a, "--parent", &group]);
    f.ok(&["complete", &group]);
    let mut d = declaration::parse(&f.ok(&["export", &a])).unwrap();
    d.issues[0].parent = None;
    d.refresh_references(&view_of(&snapshot(&f))).unwrap();
    reject(
        d.serialize(&view_of(&snapshot(&f))).unwrap(),
        "core rejection:",
    );
    let mut d = declaration::parse(&f.ok(&["export", &b])).unwrap();
    d.issues[0].parent = Some(Reference::id(&group));
    d.refresh_references(&view_of(&snapshot(&f))).unwrap();
    reject(
        d.serialize(&view_of(&snapshot(&f))).unwrap(),
        "core rejection:",
    );
}

#[cfg(not(unix))]
#[test]
fn declaration_prepare_escapes_control_characters_in_output_paths() {
    let f = Fixture::new();
    f.ok(&["init", "demo"]);
    let path = f.0.join("plan\u{1b}.yaml");
    fs::write(&path, f.ok(&["docs", "declaration", "--example"])).unwrap();
    let text = f.ok(&["import", "prepare", path.to_str().unwrap()]);
    assert!(!text.contains('\u{1b}'));
    assert!(text.contains("\\x1b"));
}

#[cfg(unix)]
#[test]
fn declaration_import_keeps_newlines_in_paths_out_of_success_headers() {
    let f = Fixture::new();
    f.ok(&["init", "demo"]);
    let path = f.0.join("plan\u{1b}\n.yaml");
    fs::write(&path, f.ok(&["docs", "declaration", "--example"])).unwrap();
    let before = snapshot(&f);
    let displayed_path = path
        .to_str()
        .unwrap()
        .replace('\u{1b}', "\\x1b")
        .replace('\n', "\\n");

    let prepared = f.ok(&["import", "prepare", path.to_str().unwrap()]);
    assert!(!prepared.contains('\u{1b}'));
    assert_eq!(
        prepared.lines().next().unwrap(),
        format!("Prepared {displayed_path}. Storage unchanged.")
    );
    let declaration = declaration::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    for record in declaration.records() {
        let mapping = format!(
            "{} -> {}",
            record.key.as_ref().unwrap(),
            record.id.as_ref().unwrap()
        );
        assert!(prepared.lines().any(|line| line == mapping));
    }
    assert_eq!(snapshot(&f), before);

    let applied = f.ok(&["import", "apply", path.to_str().unwrap()]);
    assert!(!applied.contains('\u{1b}'));
    assert_eq!(
        applied.lines().next().unwrap(),
        format!("Applied: storage applied; declaration updated: {displayed_path}")
    );
    for record in declaration.records() {
        let mapping = format!(
            "{} -> {}",
            record.key.as_ref().unwrap(),
            record.id.as_ref().unwrap()
        );
        assert!(applied.lines().any(|line| line == mapping));
    }
    assert_eq!(f.record_files().len(), declaration.records().count());
}

#[cfg(unix)]
#[test]
fn declaration_prepare_reports_applied_file_when_output_fails() {
    use std::os::fd::{FromRawFd, OwnedFd};
    let f = Fixture::new();
    f.ok(&["init", "demo"]);
    let path = f.0.join("plan.yaml");
    fs::write(&path, f.ok(&["docs", "declaration", "--example"])).unwrap();
    let before = snapshot(&f);
    let mut sockets = [0; 2];
    assert_eq!(
        unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_DGRAM, 0, sockets.as_mut_ptr()) },
        0
    );
    assert_eq!(unsafe { libc::close(sockets[0]) }, 0);
    let writer = unsafe { OwnedFd::from_raw_fd(sockets[1]) };
    let out = f
        .command()
        .args(["import", "prepare", path.to_str().unwrap()])
        .stdout(Stdio::from(writer))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let error = String::from_utf8_lossy(&out.stderr);
    assert!(
        error.contains("Applied: declaration updated; storage unchanged; output failed"),
        "{error}"
    );
    let d = declaration::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    assert!(d.records().all(|r| r.id.is_some()));
    assert_eq!(snapshot(&f), before);
}

#[test]
fn declaration_rejects_reusing_an_assigned_id_with_a_different_stored_value() {
    for applied_before in [true, false] {
        let f = Fixture::new();
        f.ok(&["init", "demo"]);
        let path = f.0.join("plan.yaml");
        fs::write(&path, f.ok(&["docs", "declaration", "--example"])).unwrap();
        f.ok(&["import", "prepare", path.to_str().unwrap()]);
        let mut d = declaration::parse(&fs::read_to_string(&path).unwrap()).unwrap();
        let conflicting_id;
        if applied_before {
            let original = fs::read(&path).unwrap();
            f.ok(&["import", "apply", path.to_str().unwrap()]);
            fs::write(&path, &original).unwrap();
            f.ok(&["import", "prepare", path.to_str().unwrap()]);
            assert_eq!(fs::read(&path).unwrap(), original);
            assert!(
                f.ok(&["import", "apply", path.to_str().unwrap()])
                    .contains("no-op")
            );
            conflicting_id = d.issues[0].id.clone().unwrap();
            f.ok(&["write", &conflicting_id, "--title", "Edited after apply"]);
        } else {
            conflicting_id = f.accepted("Different Entity");
            d.groups[0].id = Some(conflicting_id.clone());
        }
        let input = d.serialize(&view_of(&snapshot(&f))).unwrap();
        fs::write(&path, &input).unwrap();
        let before = snapshot(&f);
        let files_before = f.record_files();
        for command in ["prepare", "check", "apply"] {
            let message = failure(f.run(&["import", command, path.to_str().unwrap()]));
            for expected in [
                &format!("conflict: {conflicting_id}: new id already exists"),
                "axon export",
                "separate file",
                "transfer your edits",
                "id: null",
            ] {
                assert!(message.contains(expected), "{command}: {message}");
            }
            assert_eq!(fs::read(&path).unwrap(), input.as_bytes());
            assert_eq!(snapshot(&f), before);
            assert_eq!(f.record_files(), files_before);
        }
        // Explicitly clearing IDs requests a new plan; key references follow its new IDs.
        let old_ids: Vec<_> = d.records().map(|r| r.id.clone().unwrap()).collect();
        for r in d.groups.iter_mut().chain(&mut d.issues) {
            r.id = None;
        }
        fs::write(&path, d.serialize(&view_of(&before)).unwrap()).unwrap();
        f.ok(&["import", "prepare", path.to_str().unwrap()]);
        let new = declaration::parse(&fs::read_to_string(&path).unwrap()).unwrap();
        for r in new.records() {
            let assigned = r.id.as_ref().unwrap();
            assert!(!old_ids.contains(assigned));
            assert!(!view_of(&before).is_known(&assigned.clone().try_into().unwrap()));
        }
        f.ok(&["import", "check", path.to_str().unwrap()]);
        f.ok(&["import", "apply", path.to_str().unwrap()]);
        assert_eq!(f.record_files().len(), files_before.len() + 3);
    }
}

#[test]
fn declaration_apply_registers_edits_and_retries() {
    let mut prepared = declaration::example();
    prepared.prepare(&Store::new(), "demo").unwrap();
    let input = prepared.serialize(&view_of(&Store::new())).unwrap();
    let f = Fixture::new();
    f.ok(&["init", "demo"]);
    let path = f.0.join("plan.yaml");
    let apply = ["import", "apply", path.to_str().unwrap()];
    fs::write(&path, &input).unwrap();
    let applied_output = f.ok(&apply);
    // One record per registered Entity, with its final value.
    assert_eq!(f.record_files().len(), 3);
    for record in prepared.records() {
        let mapping = format!(
            "{} -> {}",
            record.key.as_ref().unwrap(),
            record.id.as_ref().unwrap()
        );
        assert!(applied_output.lines().any(|line| line == mapping));
    }
    assert!(!f.ok(&apply).contains(" -> "));
    let saved = snapshot(&f);
    let canonical = fs::read_to_string(&path).unwrap();
    let d = declaration::parse(&canonical).unwrap();
    assert!(d.records().all(|r| r.base.is_some() && r.key.is_some()));
    assert!(
        f.ok(&["import", "check", path.to_str().unwrap()])
            .contains("No changes")
    );
    // Simulate a successful storage save whose declaration rewrite never happened: the
    // retry with the same content adds no record.
    fs::write(&path, &input).unwrap();
    assert!(f.ok(&apply).contains("no-op"));
    assert_eq!(snapshot(&f), saved);
    assert_eq!(f.record_files().len(), 3);
    assert_eq!(fs::read_to_string(&path).unwrap(), canonical);
    let group = d.groups[0].id.as_ref().unwrap();
    let first = d
        .issues
        .iter()
        .find(|r| r.key.as_deref() == Some("first"))
        .unwrap()
        .id
        .as_ref()
        .unwrap();
    let second = d
        .issues
        .iter()
        .find(|r| r.key.as_deref() == Some("second"))
        .unwrap()
        .id
        .as_ref()
        .unwrap();
    f.ok(&["condition", "set", first, "--command", "touch executed"]);
    f.ok(&["note", "add", first, "-m", "Keep this note"]);
    let before = snapshot(&f);
    let mut edit = declaration::parse(&f.ok(&["export", group])).unwrap();
    edit.groups[0].title = "Revised plan".into();
    edit.issues
        .iter_mut()
        .find(|r| r.id.as_ref() == Some(first))
        .unwrap()
        .needs = vec![Reference::id(second)];
    let item = edit
        .issues
        .iter_mut()
        .find(|r| r.id.as_ref() == Some(second))
        .unwrap();
    item.needs.clear();
    item.parent = None;
    item.description = "Updated body".into();
    let mut new = prepared.issues[0].clone();
    new.id = Some("demo-extra".into());
    new.key = Some("extra".into());
    new.parent = Some(Reference::id(group));
    new.needs = vec![Reference::id(first)];
    edit.issues.push(new);
    edit.prepare(&before, "demo").unwrap();
    fs::write(&path, edit.serialize(&view_of(&before)).unwrap()).unwrap();
    f.ok(&["write", group, "--title", "Concurrent"]);
    let concurrent = snapshot(&f);
    assert!(failure(f.run(&apply)).contains("conflict:"));
    assert_eq!(snapshot(&f), concurrent);
    f.ok(&["write", group, "--title", &d.groups[0].title]);
    let files = f.record_files().len();
    f.ok(&apply);
    // One record per changed Entity: the three edited ones and the new one.
    assert_eq!(f.record_files().len(), files + 4);
    let after = snapshot(&f);
    let first_id: EntityId = first.clone().try_into().unwrap();
    assert_eq!(
        view_of(&after).current(&first_id).unwrap().condition,
        view_of(&before).current(&first_id).unwrap().condition
    );
    let log = f.ok(&["log", first]);
    assert!(log.contains("Declaration applied: needs"), "{log}");
    assert_eq!(
        f.ok(&["note", "list", first])
            .matches("Keep this note")
            .count(),
        1
    );
    assert!(!f.0.join("executed").exists());
    assert!(
        f.ok(&["import", "check", path.to_str().unwrap()])
            .contains("No changes")
    );
    fs::write(&path, &input).unwrap();
    let before_retry = snapshot(&f);
    assert!(failure(f.run(&apply)).contains("conflict:"));
    assert_eq!(snapshot(&f), before_retry);
}

#[test]
fn declaration_label_changes_show_each_side_and_apply_as_one_record() {
    let f = Fixture::new();
    f.ok(&["init", "demo"]);
    let id = created(&f.ok(&[
        "capture", "--label", "chore", "--accept", "--title", "Labelled",
    ]));
    let exported = f.ok(&["export", &id]);
    assert!(exported.contains("\n    description: \"\"\n    label: chore\n    parent: null\n"));
    let path = f.0.join("plan.yaml");
    let edited = exported.replace("label: chore", "label: bug");
    fs::write(&path, &edited).unwrap();
    let before = snapshot(&f);
    let text = f.ok(&["import", "check", path.to_str().unwrap()]);
    assert!(
        text.lines().any(|line| line == "  label: chore -> bug"),
        "{text}"
    );
    assert!(
        text.lines().any(|line| line == "  title: unchanged"),
        "{text}"
    );
    assert!(!text.contains("No changes"), "{text}");
    assert_eq!(snapshot(&f), before);
    f.ok(&["import", "apply", path.to_str().unwrap()]);
    let after = snapshot(&f);
    assert_eq!(after.len(), before.len() + 1);
    let log = f.ok(&["log", &id]);
    assert!(log.ends_with("  Declaration applied: label\n"), "{log}");
    assert!(f.ok(&["list", "--label", "bug"]).contains(&id));
    let rewritten = fs::read_to_string(&path).unwrap();
    assert!(rewritten.contains("label: bug"));
    assert_eq!(rewritten, f.ok(&["export", &id]));
    let check = f.ok(&["import", "check", path.to_str().unwrap()]);
    assert!(check.contains("No changes"), "{check}");
}

#[test]
fn declaration_title_changes_show_each_side_and_reject_control_characters() {
    let f = Fixture::new();
    f.ok(&["init", "demo"]);
    let id = f.accepted("Before title");
    let mut d = declaration::parse(&f.ok(&["export", &id])).unwrap();
    d.issues[0].title = "After title".into();
    d.issues[0].description = "Private full description".into();
    let path = f.0.join("plan.yaml");
    fs::write(&path, d.serialize(&view_of(&snapshot(&f))).unwrap()).unwrap();
    let text = f.ok(&["import", "check", path.to_str().unwrap()]);
    assert!(
        text.lines()
            .any(|line| line == "  title: Before title -> After title"),
        "{text}"
    );
    assert!(text.contains("description: changed"));
    assert!(
        text.lines().any(|line| line == "  label: unchanged"),
        "{text}"
    );
    assert!(!text.contains("Private full description"));
    let before = snapshot(&f);
    for title in ["After\nline", "After\u{1b}[2J", &"a".repeat(201)] {
        d.issues[0].title = title.into();
        let input = d.serialize(&view_of(&before)).unwrap();
        fs::write(&path, &input).unwrap();
        for command in ["check", "apply"] {
            let error = failure(f.run(&["import", command, path.to_str().unwrap()]));
            assert!(error.contains("title"), "{command}: {error}");
            assert!(!error.contains('\u{1b}'), "{error}");
        }
        assert_eq!(fs::read_to_string(&path).unwrap(), input);
        assert_eq!(snapshot(&f), before);
    }
}

#[test]
fn declaration_help_gives_examples_and_next_commands_without_opening_the_store() {
    let f = Fixture::new();
    f.ok(&["init", "demo"]);
    let storage = f.0.join(".axon/header.json");
    fs::write(&storage, "broken storage").unwrap();
    for (args, example, next) in [
        (
            vec!["export", "--help"],
            "axon export ID... > plan.yaml",
            "axon import prepare plan.yaml",
        ),
        (
            vec!["import", "prepare", "--help"],
            "axon import prepare plan.yaml",
            "axon import check plan.yaml",
        ),
        (
            vec!["import", "check", "--help"],
            "axon import check plan.yaml",
            "axon import apply plan.yaml",
        ),
        (
            vec!["import", "apply", "--help"],
            "axon import apply plan.yaml",
            "axon import check plan.yaml",
        ),
    ] {
        let output = f.run(&args);
        assert!(output.stderr.is_empty());
        let text = success(output);
        assert!(text.contains(example), "{text}");
        assert!(text.contains(next), "{text}");
        assert!(text.contains("Next"), "{text}");
    }
    assert_eq!(fs::read_to_string(storage).unwrap(), "broken storage");
}

#[test]
fn declaration_keeps_a_valid_written_id_and_rejects_ids_outside_the_character_rule() {
    let f = Fixture::new();
    f.ok(&["init", "demo"]);
    let path = f.0.join("plan.yaml");
    let before = snapshot(&f);
    for written in [
        "demo-multi\nline",
        "Demo-upper",
        "demo with space",
        "demo_x",
    ] {
        let mut d = declaration::example();
        d.groups[0].id = Some(written.into());
        let input = d.serialize(&view_of(&before)).unwrap();
        fs::write(&path, &input).unwrap();
        for command in ["prepare", "check", "apply"] {
            let error = failure(f.run(&["import", command, path.to_str().unwrap()]));
            assert!(error.contains("invalid EntityId"), "{command}: {error}");
            assert_eq!(error.trim_end().lines().count(), 1, "{error}");
        }
        assert_eq!(fs::read_to_string(&path).unwrap(), input);
        assert_eq!(snapshot(&f), before);
    }
    let mut d = declaration::example();
    d.groups[0].id = Some("demo-custom".into());
    fs::write(&path, d.serialize(&view_of(&before)).unwrap()).unwrap();
    for command in ["prepare", "apply"] {
        let output = f.ok(&["import", command, path.to_str().unwrap()]);
        assert!(
            output.lines().any(|line| line == "plan -> demo-custom"),
            "{output}"
        );
        assert_eq!(output.lines().count(), 4, "{output}");
    }
}

#[test]
fn declaration_rejects_external_kind_changes_before_apply_and_on_retry() {
    let title = "Revised title";
    for external_kind in ["issue", "group"] {
        let f = Fixture::new();
        f.ok(&["init", "demo"]);
        let external = if external_kind == "group" {
            created(&f.ok(&[
                "capture", "--label", "chore", "--kind", "group", "--accept", "--title", "External",
            ]))
        } else {
            f.accepted("External")
        };
        let selected = created(&f.ok(&[
            "capture", "--label", "chore", "--accept", "--title", "Original", "--needs", &external,
        ]));
        let original = snapshot(&f);
        let mut d = declaration::parse(&f.ok(&["export", &selected])).unwrap();
        d.issues[0].title = title.into();
        let valid = d.serialize(&view_of(&original)).unwrap();
        let path = f.0.join("plan.yaml");
        for retry in [false, true] {
            if retry {
                fs::write(&path, &valid).unwrap();
                f.ok(&["import", "apply", path.to_str().unwrap()]);
            }
            let before = snapshot(&f);
            d.references[0].kind = if external_kind == "group" {
                "issue"
            } else {
                "group"
            }
            .into();
            let input = d.serialize(&view_of(&before)).unwrap();
            fs::write(&path, &input).unwrap();
            for operation in ["check", "apply"] {
                let error = failure(f.run(&["import", operation, path.to_str().unwrap()]));
                assert!(
                    error.contains("read-only:") && error.contains("kind is fixed"),
                    "{error}"
                );
                assert_eq!(snapshot(&f), before);
                assert_eq!(fs::read_to_string(&path).unwrap(), input);
            }
            d.references[0].kind = external_kind.into();
            d.references[0].title = "Stale context".into();
            d.references[0].lifecycle = "cancelled".into();
            fs::write(&path, d.serialize(&view_of(&before)).unwrap()).unwrap();
            f.ok(&["import", "check", path.to_str().unwrap()]);
        }
    }
}

#[cfg(unix)]
#[test]
fn declaration_cli_handles_every_control_in_output_paths() {
    let title = "Revised plan";
    for control in [10, 27, 28, 29, 30, 31] {
        let f = Fixture::new();
        f.ok(&["init", "demo"]);
        let before = snapshot(&f);
        let path = f.0.join(format!("plan{}.yaml", char::from(control)));
        let mut plan = declaration::example();
        plan.groups[0].title = title.into();
        let input = plan.serialize(&view_of(&before)).unwrap();
        fs::write(&path, &input).unwrap();
        let output = f.ok(&["import", "prepare", path.to_str().unwrap()]);
        let escaped = if control == 10 {
            "\\n".into()
        } else {
            format!("\\x{control:02x}")
        };
        assert_eq!(
            output.lines().next().unwrap(),
            format!(
                "Prepared {}. Storage unchanged.",
                path.display()
                    .to_string()
                    .replace(char::from(control), &escaped)
            )
        );
        assert_eq!(snapshot(&f), before);
        let prepared = fs::read_to_string(&path).unwrap();
        let checked = f.ok(&["import", "check", path.to_str().unwrap()]);
        assert_eq!(
            checked
                .lines()
                .filter(|line| *line == "  Create group")
                .count(),
            1,
            "{checked}"
        );
        assert_eq!(
            checked
                .lines()
                .filter(|line| *line == "  Create issue")
                .count(),
            2,
            "{checked}"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), prepared);
        assert_eq!(snapshot(&f), before);
        let applied = f.ok(&["import", "apply", path.to_str().unwrap()]);
        let saved = snapshot(&f);
        assert_eq!(view_of(&saved).known().count(), 3);
        let result = declaration::parse(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(result.groups[0].title, title);
        assert!(result.records().all(|r| r.id.is_some() && r.base.is_some()));
        assert!(applied.contains("plan -> "));
    }
}
