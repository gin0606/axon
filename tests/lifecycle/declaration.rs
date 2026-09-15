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
        let outer = created(&f.ok(&["group", "plan", "--title", "Outer"]));
        let group = created(&f.ok(&["group", "plan", "--title", "Plan", "--parent", &outer]));
        let external = f.plan("Outside");
        let done = created(&f.ok(&["plan", "--title", "Finished", "--parent", &group]));
        let cancelled = created(&f.ok(&["plan", "--title", "Cancelled", "--parent", &group]));
        let nested = created(&f.ok(&["group", "plan", "--title", "Nested", "--parent", &group]));
        let child = created(&f.ok(&[
            "plan",
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
        f.ok(&["done", &done]);
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
    assert_eq!(d.serialize(&axon::sqlite::empty()).unwrap(), yaml);
    assert!(f.ok(&["docs"]).contains("docs declaration"));
    let help = f.ok(&["--help"]);
    let section = help
        .split("Plan management:")
        .nth(1)
        .unwrap()
        .split("Setup & utilities:")
        .next()
        .unwrap();
    assert!(section.contains("export"));
    assert!(f.ok(&["export", "--help"]).contains("<ID>..."));
    assert!(
        f.ok(&["docs", "declaration", "--help"])
            .contains("--example")
    );
}
