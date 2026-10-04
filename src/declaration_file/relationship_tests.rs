use crate::declaration::*;
use crate::lifecycle::EntityId;
use crate::lifecycle::record::{Context, Current, Entry, Kind, Lifecycle, Operation, Store};
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
fn current(kind: Kind, title: &str) -> Current {
    Current {
        kind,
        lifecycle: Lifecycle::NotStarted,
        owner: None,
        title: title.into(),
        description: String::new(),
        label: crate::lifecycle::Label::Chore,
        condition: None,
        parent: None,
        needs: BTreeSet::new(),
    }
}
/// The records of one scenario, built in memory and published as one batch per fixture.
struct Scenario {
    store: Store,
    clock: std::cell::Cell<i64>,
}
impl Scenario {
    fn new() -> Self {
        Self {
            store: Store::new(),
            clock: std::cell::Cell::new(2000),
        }
    }
    fn tick(&self) -> Context {
        self.clock.set(self.clock.get() + 1);
        Context {
            at: Utc.timestamp_opt(self.clock.get(), 0).unwrap(),
            recorder: None,
        }
    }
    fn create(&mut self, name: &str, kind: Kind) {
        let record = self
            .store
            .create(id(name), current(kind, name), self.tick())
            .unwrap();
        self.store.insert(Entry::Record(record)).unwrap();
    }
    fn perform(&mut self, name: &str, operation: Operation) {
        let record = self
            .store
            .perform(&id(name), operation, None, self.tick())
            .unwrap();
        self.store.insert(Entry::Record(record)).unwrap();
    }
    fn set_parent(&mut self, name: &str, parent: &str) {
        let record = self
            .store
            .set_parent(&id(name), Some(id(parent)), None, self.tick())
            .unwrap()
            .unwrap();
        self.store.insert(Entry::Record(record)).unwrap();
    }
    fn add_dependency(&mut self, name: &str, target: &str) {
        let record = self
            .store
            .add_dependency(&id(name), &id(target), None, self.tick())
            .unwrap()
            .unwrap();
        self.store.insert(Entry::Record(record)).unwrap();
    }
}

fn reorder_records(text: &str, group_order: usize, issue_order: usize) -> String {
    let mut result = String::new();
    let mut blocks: Vec<String> = Vec::new();
    let mut order = 0;
    let flush = |blocks: &mut Vec<String>, result: &mut String, order| {
        let mut remaining = std::mem::take(blocks);
        let mut order = order;
        while !remaining.is_empty() {
            let index = order % remaining.len();
            order /= remaining.len();
            result.push_str(&remaining.remove(index));
        }
    };
    for line in text.split_inclusive('\n') {
        if !line.starts_with(' ') {
            flush(&mut blocks, &mut result, order);
            result.push_str(line);
            order = match line {
                "groups:\n" => group_order,
                "issues:\n" => issue_order,
                _ => 0,
            };
        } else if line.starts_with("  - ") {
            blocks.push(line.into());
        } else {
            blocks.last_mut().unwrap().push_str(line);
        }
    }
    flush(&mut blocks, &mut result, order);
    result
}

#[test]
fn relationship_changes_are_order_independent() {
    for case in [
        "cancelled-parent",
        "cancelled-dependencies",
        "new-parent",
        "invert-tree",
        "active-subtree",
    ] {
        let mut scenario = Scenario::new();
        for (name, kind) in [
            ("g", Kind::Group),
            ("h", Kind::Group),
            ("j", Kind::Group),
            ("a", Kind::Issue),
            ("b", Kind::Issue),
        ] {
            scenario.create(name, kind);
        }
        match case {
            "cancelled-parent" => scenario.perform("g", Operation::Cancel),
            "cancelled-dependencies" => {
                scenario.add_dependency("a", "b");
                scenario.perform("a", Operation::Cancel);
            }
            "invert-tree" => scenario.set_parent("h", "g"),
            "active-subtree" => {
                scenario.set_parent("j", "g");
                scenario.set_parent("a", "j");
                scenario.perform("a", Operation::Start);
            }
            _ => {}
        }
        let before = scenario.store;
        let before_view = before.view().unwrap();
        let selectors = [id("g"), id("h"), id("j"), id("a"), id("b")];
        let mut d = export(&before, &before_view, &selectors).unwrap();
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
        d.groups[1].title = "Renamed".into();
        d.prepare(&before, "demo").unwrap();
        let text = d.serialize(&before_view).unwrap();
        let mut canonical_result = None;
        let group_orders = (1..=d.groups.len()).product::<usize>();
        let issue_orders = (1..=d.issues.len()).product::<usize>();
        for (group_order, issue_order) in
            (0..group_orders).flat_map(|group| (0..issue_orders).map(move |issue| (group, issue)))
        {
            let root = std::env::temp_dir()
                .join(format!("axon-relations-{:032x}", rand::random::<u128>()));
            std::fs::create_dir(&root).unwrap();
            let location = crate::location::Location::discover(&root, true).unwrap();
            location.init("demo").unwrap();
            let mut store = location.open().unwrap();
            store
                .update(|_, _, _| Ok((before.entries().map(|(_, e)| e.clone()).collect(), ())))
                .unwrap();
            assert_eq!(store.read().unwrap().1, before);
            let input = reorder_records(&text, group_order, issue_order);
            let mut parsed = parse(&input).unwrap();
            if group_order == d.groups.len() - 1 {
                assert_eq!(parsed.groups.first(), d.groups.last());
            }
            if issue_order == 1 {
                assert_eq!(parsed.issues.first(), d.issues.last());
            }
            let mut candidate = before.clone();
            let operations = parsed.apply_operations(&mut candidate, context());
            parsed.prepare(&before, "demo").unwrap();
            let input = parsed.serialize(&before_view).unwrap();
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
                drop(store);
                std::fs::remove_dir_all(root).unwrap();
                continue;
            }
            operations.unwrap();
            assert!(result.unwrap().changed, "{case}");
            let candidate_view = candidate.view().unwrap();
            let after_view = after.view().unwrap();
            for (entity, settled) in candidate_view.settled() {
                assert_eq!(after_view.current(entity).unwrap(), &settled.current);
            }
            let state = |name: &str| after_view.current(&id(name)).unwrap();
            match case {
                "cancelled-dependencies" => {
                    assert_eq!(state("a").lifecycle, Lifecycle::Cancelled);
                    assert_eq!(state("a").needs, BTreeSet::from([id("h")]));
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
                    assert_eq!(state("a").lifecycle, Lifecycle::InProgress);
                    for name in ["g", "h", "j"] {
                        assert_eq!(state(name).lifecycle, Lifecycle::NotStarted);
                    }
                    assert_eq!(
                        after_view.effective_lifecycle(&id("h")),
                        Some(Lifecycle::InProgress)
                    );
                }
                _ => unreachable!(),
            }
            let output = std::fs::read_to_string(&path).unwrap();
            if let Some(expected) = &canonical_result {
                assert_eq!(
                    &output, expected,
                    "{case}, group order {group_order}, issue order {issue_order}"
                );
            } else {
                canonical_result = Some(output);
            }
            assert!(
                !crate::declaration_file::apply(&mut store, &path, context())
                    .unwrap()
                    .changed
            );
            assert_eq!(store.read().unwrap().1, after);
            drop(store);
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}
