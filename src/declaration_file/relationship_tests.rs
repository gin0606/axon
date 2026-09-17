use crate::declaration::*;
use crate::lifecycle::{Context, Current, EntityId, Kind, Lifecycle};
use chrono::{TimeZone, Utc};
use std::collections::BTreeSet;

fn id(s: &str) -> EntityId {
    s.to_owned().try_into().unwrap()
}
fn context() -> Context {
    Context {
        at: Utc.timestamp_opt(1000, 0).unwrap(),
        recorder: None,
    }
}
fn current(title: &str) -> Current {
    Current {
        title: title.into(),
        description: String::new(),
        lifecycle: Lifecycle::NotStarted,
        condition: None,
        parent: None,
        dependencies: BTreeSet::new(),
    }
}

fn reorder_records(text: &str, order: usize) -> String {
    let mut result = String::new();
    let mut blocks: Vec<String> = Vec::new();
    let flush = |blocks: &mut Vec<String>, result: &mut String| {
        if order == 1 {
            blocks.reverse();
        } else if order == 2 && !blocks.is_empty() {
            blocks.rotate_left(1);
        }
        for block in blocks.drain(..) {
            result.push_str(&block);
        }
    };
    for line in text.split_inclusive('\n') {
        if !line.starts_with(' ') {
            flush(&mut blocks, &mut result);
            result.push_str(line);
        } else if line.starts_with("  - ") {
            blocks.push(line.into());
        } else {
            blocks.last_mut().unwrap().push_str(line);
        }
    }
    flush(&mut blocks, &mut result);
    result
}

#[test]
fn relationship_changes_are_order_independent_on_both_backends() {
    use crate::lifecycle::Operation;
    for file_backend in [false, true] {
        for case in [
            "cancelled-parent",
            "cancelled-dependencies",
            "new-parent",
            "invert-tree",
            "active-subtree",
        ] {
            let root = std::env::temp_dir()
                .join(format!("axon-relations-{:032x}", rand::random::<u128>()));
            std::fs::create_dir(&root).unwrap();
            let location = crate::location::Location::discover(&root, true).unwrap();
            location.init_backend("demo", file_backend).unwrap();
            let mut store = location.open().unwrap();
            let mut before = store.read().unwrap().1;
            for (name, kind) in [
                ("g", Kind::Group),
                ("h", Kind::Group),
                ("j", Kind::Group),
                ("a", Kind::Issue),
                ("b", Kind::Issue),
            ] {
                before
                    .create(id(name), kind, current(name), context())
                    .unwrap();
            }
            match case {
                "cancelled-parent" => {
                    before
                        .perform(&id("g"), Operation::Cancel, None, context())
                        .unwrap();
                }
                "cancelled-dependencies" => {
                    before.add_dependency(&id("a"), &id("b")).unwrap();
                    before
                        .perform(&id("a"), Operation::Cancel, None, context())
                        .unwrap();
                }
                "invert-tree" => before.set_parent(&id("h"), Some(id("g"))).unwrap(),
                "active-subtree" => {
                    before.set_parent(&id("j"), Some(id("g"))).unwrap();
                    before.set_parent(&id("a"), Some(id("j"))).unwrap();
                    for name in ["g", "h", "j", "a"] {
                        before
                            .perform(&id(name), Operation::Start, None, context())
                            .unwrap();
                    }
                }
                _ => {}
            }
            let selectors = [id("g"), id("h"), id("j"), id("a"), id("b")];
            let mut d = export(&before, &selectors).unwrap();
            match case {
                "cancelled-parent" => d.issues[0].parent = Some(Reference::id("g")),
                "cancelled-dependencies" => d.issues[0].needs = vec![Reference::id("h")],
                "new-parent" => {
                    d.groups.push(example().groups.remove(0));
                    d.issues[0].parent = Some(Reference::key("plan"));
                }
                "invert-tree" => {
                    d.groups[0].parent = Some(Reference::id("h"));
                    d.groups[1].parent = None;
                }
                "active-subtree" => d.groups[2].parent = Some(Reference::id("h")),
                _ => unreachable!(),
            }
            d.prepare(&before, "demo").unwrap();
            let text = d.serialize(&before).unwrap();
            let mut canonical_result = None;
            for order in 0..3 {
                store
                    .update(|_, snapshot| {
                        *snapshot = before.clone();
                        Ok(())
                    })
                    .unwrap();
                let input = reorder_records(&text, order);
                let mut parsed = parse(&input).unwrap();
                if order == 1 {
                    assert_eq!(parsed.groups.first(), d.groups.last());
                    assert_eq!(parsed.issues.first(), d.issues.last());
                }
                let mut candidate = before.clone();
                let operations = parsed.apply_operations(&mut candidate, context());
                parsed.prepare(&before, "demo").unwrap();
                let input = parsed.serialize(&before).unwrap();
                let path = root.join("plan.yaml");
                std::fs::write(&path, &input).unwrap();
                let result = crate::declaration_file::apply(&mut store, &path, context());
                let after = store.read().unwrap().1;
                if case == "cancelled-parent" {
                    assert!(operations.unwrap_err().to_string().contains("parent"));
                    let error = result.unwrap_err().to_string();
                    assert!(error.contains("parent"), "{error}");
                    assert_eq!(after, before);
                    assert_eq!(std::fs::read_to_string(&path).unwrap(), input);
                    continue;
                }
                operations.unwrap();
                assert!(result.unwrap().changed, "{case}");
                for entity in candidate.entities() {
                    assert_eq!(after.entity(&entity.id).unwrap().current, entity.current);
                }
                let state = |name: &str| &after.entity(&id(name)).unwrap().current;
                match case {
                    "cancelled-dependencies" => {
                        assert_eq!(state("a").lifecycle, Lifecycle::Cancelled);
                        assert_eq!(state("a").dependencies, BTreeSet::from([id("h")]));
                    }
                    "new-parent" => assert_eq!(
                        state("a").parent,
                        Some(id(d.groups.last().unwrap().id.as_ref().unwrap()))
                    ),
                    "invert-tree" => {
                        assert_eq!(state("g").parent, Some(id("h")));
                        assert_eq!(state("h").parent, None);
                    }
                    "active-subtree" => {
                        assert_eq!(state("j").parent, Some(id("h")));
                        assert_eq!(state("a").parent, Some(id("j")));
                        for name in ["g", "h", "j", "a"] {
                            assert_eq!(state(name).lifecycle, Lifecycle::InProgress);
                        }
                    }
                    _ => unreachable!(),
                }
                let output = std::fs::read_to_string(&path).unwrap();
                if let Some(expected) = &canonical_result {
                    assert_eq!(&output, expected, "{case}, order {order}");
                } else {
                    canonical_result = Some(output);
                }
                assert!(
                    !crate::declaration_file::apply(&mut store, &path, context())
                        .unwrap()
                        .changed
                );
                assert_eq!(store.read().unwrap().1, after);
            }
            drop(store);
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}
