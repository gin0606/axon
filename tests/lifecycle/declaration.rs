use super::*;
use axon::declaration::{self, Reference};
fn created(text: &str) -> String {
    text.split_whitespace().next().unwrap().into()
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
#[test]
fn declaration_export_selectors_and_references_are_read_only_on_both_backends() {
    for backend in ["sqlite", "file"] {
        let f = Fixture::new();
        f.ok(&["init", "demo", "--backend", backend]);
        let outer = created(&f.ok(&["capture", "--kind", "group", "--accept", "--title", "Outer"]));
        let group = created(&f.ok(&[
            "capture", "--kind", "group", "--accept", "--title", "Plan", "--parent", &outer,
        ]));
        let external = f.accepted("Outside");
        let done = created(&f.ok(&[
            "capture", "--accept", "--title", "Finished", "--parent", &group,
        ]));
        let cancelled = created(&f.ok(&[
            "capture",
            "--accept",
            "--title",
            "Cancelled",
            "--parent",
            &group,
        ]));
        let nested = created(&f.ok(&[
            "capture", "--kind", "group", "--accept", "--title", "Nested", "--parent", &group,
        ]));
        let child = created(&f.ok(&[
            "capture",
            "--accept",
            "--title",
            "Body\r\nwith control\u{1}",
            "-m",
            "\n indented\nlast\n\n",
            "--parent",
            &nested,
            "--needs",
            &external,
            "--command",
            "touch executed",
        ]));
        f.ok(&["start", &outer]);
        f.ok(&["start", &group]);
        f.ok(&["start", &done]);
        f.ok(&["complete", &done]);
        f.ok(&["cancel", &cancelled]);
        let before = snapshot(&f);
        let file_before =
            (backend == "file").then(|| fs::read(f.0.join(".axon/state.jsonl")).unwrap());
        let output = f.run(&["export", group.rsplit('-').next().unwrap()]);
        assert!(output.stderr.is_empty());
        let yaml = success(output);
        let d = declaration::parse(&yaml).unwrap();
        assert_eq!(d.serialize(&before).unwrap(), yaml);
        assert_eq!(d.groups.len(), 2);
        assert_eq!(d.issues.len(), 3);
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
        assert_eq!(single.issues[0].title, "Body\r\nwith control\u{1}");
        assert_eq!(single.issues[0].description, "\n indented\nlast\n\n");
        assert_eq!(
            single
                .references
                .iter()
                .map(|r| r.id.clone())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([nested.clone(), external.clone()])
        );
        let union =
            declaration::parse(&f.ok(&["export", &child, &external, &group, &group])).unwrap();
        assert_eq!(union.groups.len(), 2);
        assert_eq!(union.issues.len(), 4);
        assert_eq!(
            union.references.iter().map(|r| &r.id).collect::<Vec<_>>(),
            vec![&outer]
        );
        assert!(failure(f.run(&["export", &group, "missing"])).contains("missing"));
        failure(f.run(&["export"]));
        assert_eq!(snapshot(&f), before);
        if let Some(bytes) = file_before {
            assert_eq!(fs::read(f.0.join(".axon/state.jsonl")).unwrap(), bytes);
        }
        assert!(!f.0.join("executed").exists());
    }
}
#[test]
fn declaration_docs_and_template_work_with_broken_management_root() {
    let f = Fixture::new();
    fs::write(f.0.join(".git"), "broken marker").unwrap();
    fs::create_dir(f.0.join(".axon")).unwrap();
    fs::write(f.0.join(".axon/axon.db"), "broken storage").unwrap();
    let docs = f.ok(&["docs", "declaration"]);
    for phrase in [
        "id",
        "key",
        "base",
        "lifecycle",
        "title",
        "description",
        "parent",
        "needs",
        "prepare",
        "check",
        "apply",
        "parent: null",
        "does not delete or cancel",
        "untouched",
    ] {
        assert!(docs.contains(phrase), "{phrase}");
    }
    let output = f.run(&["docs", "declaration", "--example"]);
    assert!(output.stderr.is_empty());
    let yaml = success(output);
    let d = declaration::parse(&yaml).unwrap();
    assert_eq!(d, declaration::example());
    assert_eq!(d.serialize(&Snapshot::empty()).unwrap(), yaml);
    assert!(f.ok(&["docs"]).contains("docs declaration"));
    let help = f.ok(&["--help"]);
    for (heading, name) in [
        ("Candidates & inspection:", "export"),
        ("Text & relationships:", "import"),
    ] {
        let section = help.split(heading).nth(1).unwrap();
        let section = section.trim_start().split("\n\n").next().unwrap();
        assert!(
            section
                .lines()
                .any(|line| line.split_whitespace().next() == Some(name))
        );
    }
    assert!(f.ok(&["import", "--help"]).contains("prepare"));
    assert!(f.ok(&["import", "check", "--help"]).contains("<FILE>"));
    assert!(f.ok(&["export", "--help"]).contains("<ID>..."));
    assert!(
        f.ok(&["docs", "declaration", "--help"])
            .contains("--example")
    );
}

#[test]
fn declaration_prepare_check_new_plan_and_existing_changes_on_both_backends() {
    for backend in ["sqlite", "file"] {
        let f = Fixture::new();
        f.ok(&["init", "demo", "--backend", backend]);
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
        assert!(text.contains("needs: +"));
        assert!(text.contains("Situation after: Blocked"));
        assert_eq!(before, snapshot(&f));
        assert_eq!(bytes, fs::read(&path).unwrap());
        let existing = created(&f.ok(&[
            "capture",
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
}

#[test]
fn declaration_check_rejections_preserve_both_backends_and_input() {
    for backend in ["sqlite", "file"] {
        let f = Fixture::new();
        f.ok(&["init", "demo", "--backend", backend]);
        let item = f.accepted("Original");
        let external = f.accepted("External");
        let exported = f.ok(&["export", &item]);
        let path = f.0.join("plan.yaml");
        let before = snapshot(&f);
        let mutations = [
            (
                exported.replace("axon-declaration/v1", "wrong/v1"),
                "schema:",
            ),
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
}

#[test]
fn declaration_check_rejects_local_and_core_guards_on_both_backends() {
    for backend in ["sqlite", "file"] {
        let f = Fixture::new();
        f.ok(&["init", "demo", "--backend", backend]);
        let group = created(&f.ok(&["capture", "--kind", "group", "--accept", "--title", "Group"]));
        let other = created(&f.ok(&["capture", "--kind", "group", "--accept", "--title", "Other"]));
        let a = f.accepted("A");
        let b = f.accepted("B");
        let exported = f.ok(&["export", &a]);
        let path = f.0.join("plan.yaml");
        let reject = |input: String, category: &str| {
            let before = snapshot(&f);
            fs::write(&path, &input).unwrap();
            let message = failure(f.run(&["import", "check", path.to_str().unwrap()]));
            let apply_message = failure(f.run(&["import", "apply", path.to_str().unwrap()]));
            assert!(apply_message.contains("Not applied:"), "{apply_message}");
            assert!(message.contains(category), "{category}: {message}");
            assert_eq!(snapshot(&f), before);
            assert_eq!(fs::read_to_string(&path).unwrap(), input);
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
        let fresh_yaml = fresh.serialize(&snapshot(&f)).unwrap();
        reject(
            fresh_yaml.replace("    key: first\n", "    key: null\n"),
            "identity/reference:",
        );
        reject(
            fresh_yaml.replace("lifecycle: not-started", "lifecycle: completed"),
            "identity/reference:",
        );
        let mut d = declaration::parse(&exported).unwrap();
        d.issues.push(d.issues[0].clone());
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
        reject(d.serialize(&snapshot(&f)).unwrap(), "read-only:");
        let mut d = declaration::parse(&exported).unwrap();
        d.issues[0].base = Some(format!("blake3:{}", "0".repeat(64)));
        d.issues[0].title = "Edit".into();
        reject(d.serialize(&snapshot(&f)).unwrap(), "conflict:");
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
        reject(d.serialize(&snapshot(&f)).unwrap(), "core rejection:");
        let mut d = declaration::parse(&f.ok(&["export", &group, &other])).unwrap();
        d.groups[0].parent = Some(Reference::id(&other));
        d.groups[1].parent = Some(Reference::id(&group));
        reject(d.serialize(&snapshot(&f)).unwrap(), "core rejection:");
        f.ok(&["start", &a]);
        let mut d = declaration::parse(&f.ok(&["export", &a])).unwrap();
        d.issues[0].parent = Some(Reference::id(&group));
        d.refresh_references(&snapshot(&f)).unwrap();
        reject(d.serialize(&snapshot(&f)).unwrap(), "core rejection:");
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
                reject(d.serialize(&snapshot(&f)).unwrap(), "core rejection:");
            }
        }
        let mut d = declaration::parse(&f.ok(&["export", &a])).unwrap();
        d.issues[0].needs.push(Reference::id(&b));
        d.refresh_references(&snapshot(&f)).unwrap();
        reject(d.serialize(&snapshot(&f)).unwrap(), "core rejection:");
        f.ok(&["parent", "set", &a, "--parent", &group]);
        f.ok(&["start", &group]);
        f.ok(&["complete", &group]);
        let mut d = declaration::parse(&f.ok(&["export", &a])).unwrap();
        d.issues[0].parent = None;
        d.refresh_references(&snapshot(&f)).unwrap();
        reject(d.serialize(&snapshot(&f)).unwrap(), "core rejection:");
        let mut d = declaration::parse(&f.ok(&["export", &b])).unwrap();
        d.issues[0].parent = Some(Reference::id(&group));
        d.refresh_references(&snapshot(&f)).unwrap();
        reject(d.serialize(&snapshot(&f)).unwrap(), "core rejection:");
    }
}

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
fn declaration_prepare_reports_applied_file_when_output_fails() {
    use std::os::fd::{FromRawFd, OwnedFd};
    for backend in ["sqlite", "file"] {
        let f = Fixture::new();
        f.ok(&["init", "demo", "--backend", backend]);
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
}

#[test]
fn declaration_apply_registers_edits_and_retries_on_both_backends() {
    let mut prepared = declaration::example();
    prepared.prepare(&Snapshot::empty(), "demo").unwrap();
    let input = prepared.serialize(&Snapshot::empty()).unwrap();
    let mut results = Vec::new();
    for backend in ["sqlite", "file"] {
        let f = Fixture::new();
        f.ok(&["init", "demo", "--backend", backend]);
        let path = f.0.join("plan.yaml");
        let apply = ["import", "apply", path.to_str().unwrap()];
        fs::write(&path, &input).unwrap();
        let applied_output = f.ok(&apply);
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
        // Simulate a successful storage save whose declaration rewrite never happened.
        fs::write(&path, &input).unwrap();
        assert!(f.ok(&apply).contains("no-op"));
        assert_eq!(snapshot(&f), saved);
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
        fs::write(&path, edit.serialize(&before).unwrap()).unwrap();
        f.ok(&["write", group, "--title", "Concurrent"]);
        let concurrent = snapshot(&f);
        assert!(failure(f.run(&apply)).contains("conflict:"));
        assert_eq!(snapshot(&f), concurrent);
        f.ok(&["write", group, "--title", &d.groups[0].title]);
        f.ok(&apply);
        let after = snapshot(&f);
        let first_id = first.clone().try_into().unwrap();
        assert_eq!(
            after.entity(&first_id).unwrap().current.condition,
            before.entity(&first_id).unwrap().current.condition
        );
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
        results.push(
            after
                .entities()
                .map(|e| (e.id.clone(), (e.kind, e.current.clone())))
                .collect::<std::collections::BTreeMap<_, _>>(),
        );
        fs::write(&path, &input).unwrap();
        let before_retry = snapshot(&f);
        assert!(failure(f.run(&apply)).contains("conflict:"));
        assert_eq!(snapshot(&f), before_retry);
    }
    assert_eq!(results[0], results[1]);
}

#[test]
fn declaration_title_changes_escape_each_side_on_both_backends() {
    for backend in ["sqlite", "file"] {
        let f = Fixture::new();
        f.ok(&["init", "demo", "--backend", backend]);
        let id = f.accepted("Before\nline\r\t\u{1b}");
        let mut d = declaration::parse(&f.ok(&["export", &id])).unwrap();
        d.issues[0].title = "After\nline\r\t\u{7f}".into();
        d.issues[0].description = "Private full description".into();
        let path = f.0.join("plan.yaml");
        fs::write(&path, d.serialize(&snapshot(&f)).unwrap()).unwrap();
        let text = f.ok(&["import", "check", path.to_str().unwrap()]);
        assert!(
            text.lines()
                .any(|line| line == "  title: Before\\nline\\r\\t\\x1b -> After\\nline\\r\\t\\x7f"),
            "{text}"
        );
        assert!(text.contains("description: changed"));
        assert!(!text.contains("Private full description"));
        d.issues[0].title = "Before\nline\r\t\u{1b}".into();
        fs::write(&path, d.serialize(&snapshot(&f)).unwrap()).unwrap();
        assert!(
            f.ok(&["import", "check", path.to_str().unwrap()])
                .contains("title: unchanged")
        );
    }
}

#[test]
fn declaration_help_gives_examples_and_next_commands_without_opening_either_backend() {
    for backend in ["sqlite", "file"] {
        let f = Fixture::new();
        f.ok(&["init", "demo", "--backend", backend]);
        let storage = f.0.join(if backend == "sqlite" {
            ".axon/axon.db"
        } else {
            ".axon/state.jsonl"
        });
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
}

#[test]
fn declaration_new_id_mappings_stay_on_one_line_on_both_backends() {
    for backend in ["sqlite", "file"] {
        let f = Fixture::new();
        f.ok(&["init", "demo", "--backend", backend]);
        let mut d = declaration::example();
        d.groups[0].id = Some("demo-multi\nline\r\t\u{1b}".into());
        let path = f.0.join("plan.yaml");
        fs::write(&path, d.serialize(&snapshot(&f)).unwrap()).unwrap();
        for command in ["prepare", "apply"] {
            let output = f.ok(&["import", command, path.to_str().unwrap()]);
            assert!(
                output
                    .lines()
                    .any(|line| line == "plan -> demo-multi\\nline\\r\\t\\x1b"),
                "{output}"
            );
            assert_eq!(output.lines().count(), 4, "{output}");
        }
    }
}

#[test]
fn declaration_rejects_external_kind_changes_before_apply_and_on_retry() {
    for backend in ["sqlite", "file"] {
        for external_kind in ["issue", "group"] {
            let f = Fixture::new();
            f.ok(&["init", "demo", "--backend", backend]);
            let external = if external_kind == "group" {
                created(&f.ok(&[
                    "capture", "--kind", "group", "--accept", "--title", "External",
                ]))
            } else {
                f.accepted("External")
            };
            let selected = created(&f.ok(&[
                "capture", "--accept", "--title", "Original", "--needs", &external,
            ]));
            let original = snapshot(&f);
            let mut d = declaration::parse(&f.ok(&["export", &selected])).unwrap();
            d.issues[0].title = "Edited".into();
            let valid = d.serialize(&original).unwrap();
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
                let input = d.serialize(&before).unwrap();
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
                fs::write(&path, d.serialize(&before).unwrap()).unwrap();
                f.ok(&["import", "check", path.to_str().unwrap()]);
            }
        }
    }
}
