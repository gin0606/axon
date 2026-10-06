//! Seeds a project with thousands of records and times what the window derives from them.
//!
//! ```sh
//! cargo run --release -p axon-gui --example scale -- <data-dir> [top-level-groups]
//! AXON_GUI_DATA_DIR=<data-dir> cargo run --release -p axon-gui
//! ```
//!
//! `<data-dir>` is an absolute path, as for `AXON_GUI_DATA_DIR`. One that does not exist yet
//! is seeded first; one that exists has its seeded project measured again.
//! Remove the directory of a seeding that failed or was stopped before measuring again.
//! Each top-level Group holds 4 Groups of 10 Issues each, so
//! the default of 110 Groups and 50 top-level Issues makes 5,000 Entities. Half the Issues
//! carry a Note, every tenth five more, and Issues inside a Group depend on one another.

use axon::lifecycle::{
    Context, EntityId, Kind, Label, Lifecycle, Operation,
    record::{Current, Entry, Store, new_entity_id},
};
use axon::location::Location;
use axon_gui::board::{Board, Filter, Layout, State, listing::listing};
use axon_gui::project::AppData;
use chrono::{TimeDelta, Utc};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// The name of the project the example seeds and measures.
const PROJECT: &str = "大量データ";
const SUBGROUPS: usize = 4;
const ISSUES: usize = 10;
const LOOSE_ISSUES: usize = 50;

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(
        args.next()
            .expect("usage: scale <data-dir> [top-level-groups]"),
    );
    let groups: usize = args.next().map_or(110, |n| n.parse().expect("a count"));
    assert!(groups > 0, "at least one top-level Group");
    let seed = !dir.exists();
    let data = AppData::at(&dir).unwrap();
    if seed {
        let registry = data.create_project(PROJECT).unwrap();
        let project = registry.projects().last().unwrap().clone();
        let started = Instant::now();
        // Written through the core, as the CLI would; the window only reads.
        Location::standalone(&data.project_root(&project.id))
            .and_then(|location| location.open())
            .and_then(|mut store| {
                store.update(|header, records, _| {
                    let mut seed = Seed::new(records.clone(), &header.prefix);
                    seed.populate(groups);
                    Ok((seed.entries, ()))
                })
            })
            .unwrap();
        println!(
            "seeded {} in {:.1?}",
            data.project_root(&project.id).display(),
            started.elapsed()
        );
    }
    let registry = data.load_registry().unwrap();
    let project = registry
        .projects()
        .iter()
        .find(|project| project.name == PROJECT)
        .expect("a seeded project");
    let connection = data.connect(project);

    let load = time(5, || connection.load().unwrap());
    let (_, records, view) = connection.load().unwrap();
    // The window moves a read into its board; copies made beforehand keep cloning untimed.
    let mut reads = vec![(records.clone(), view.clone()); 5].into_iter();
    let board_new = time(5, || {
        let (records, view) = reads.next().unwrap();
        Board::new(records, view)
    });
    let board = Board::new(records.clone(), view);
    let notes = records.notes().count();
    println!(
        "{} Entities, {} Notes, {} entries",
        board.len(),
        notes,
        records.len()
    );
    println!("read the store: {}", median(load));
    println!("derive the board: {}", median(board_new));
    println!(
        "derive the view: {}",
        median(time(5, || records.view().unwrap()))
    );

    let default = Filter::default();
    let mut everything = Filter::default();
    everything.states.extend(State::ALL);
    let found = Filter {
        query: "検索語".into(),
        ..Filter::default()
    };
    let missing = Filter {
        query: "どこにもない語".into(),
        ..Filter::default()
    };
    for (name, filter) in [
        ("default filter", &default),
        ("every state", &everything),
        ("search, some match", &found),
        ("search, none match", &missing),
    ] {
        for layout in [Layout::Tree, Layout::Flat] {
            let runs = time(20, || listing(&board, filter, layout));
            let rows = listing(&board, filter, layout).rows.len();
            println!("list ({name}, {layout:?}, {rows} rows): {}", median(runs));
        }
    }

    let first = |kind: Kind, parent: bool| {
        board
            .items()
            .iter()
            .find(|item| item.kind == kind && item.parent.is_some() == parent)
            .unwrap()
            .id
            .clone()
    };
    // The Issue with a dependency that carries the most Notes, at any scale.
    let mut notes_of = std::collections::HashMap::new();
    for (_, note) in records.notes() {
        *notes_of.entry(&note.entity).or_insert(0) += 1;
    }
    let read = board.read();
    let noted = board
        .items()
        .iter()
        .filter(|item| {
            read.presented(&item.id)
                .is_some_and(|c| !c.needs.is_empty())
        })
        .max_by_key(|item| notes_of.get(&item.id).copied().unwrap_or(0))
        .expect("an Issue with a dependency")
        .id
        .clone();
    for (name, id) in [
        ("top-level Group", first(Kind::Group, false)),
        ("Issue with Notes and dependencies", noted),
        ("top-level Issue", first(Kind::Issue, false)),
    ] {
        let runs = time(10, || board.detail(&id).unwrap());
        println!("detail ({name}): {}", median(runs));
    }
    // The detail gathers the relations, Notes and history of the Entity, so its time depends
    // on the Entity: the slowest of the first ones shows the worst case.
    let slowest = board
        .items()
        .iter()
        .take(120)
        .map(|item| {
            // The median of a few runs, so one interrupted run is not taken for the slowest.
            let mut runs = time(3, || board.detail(&item.id).unwrap());
            runs.sort();
            (runs[1], item.kind, item.state)
        })
        .max_by_key(|(elapsed, ..)| *elapsed)
        .unwrap();
    println!(
        "detail (slowest of the first 120, a {:?} {:?}): {:.2?}",
        slowest.2, slowest.1, slowest.0
    );
}

fn time<T>(runs: usize, mut f: impl FnMut() -> T) -> Vec<Duration> {
    (0..runs)
        .map(|_| {
            let started = Instant::now();
            std::hint::black_box(f());
            started.elapsed()
        })
        .collect()
}

fn median(mut runs: Vec<Duration>) -> String {
    runs.sort();
    format!(
        "median {:.2?} (min {:.2?}, max {:.2?}, {} runs)",
        runs[runs.len() / 2],
        runs[0],
        runs[runs.len() - 1],
        runs.len()
    )
}

/// Builds entries on a copy of the store, so each one is checked against the earlier ones.
struct Seed {
    records: Store,
    prefix: String,
    entries: Vec<Entry>,
    clock: chrono::DateTime<Utc>,
    serial: usize,
}

impl Seed {
    fn new(records: Store, prefix: &str) -> Self {
        Self {
            records,
            prefix: prefix.to_owned(),
            entries: Vec::new(),
            clock: Utc::now(),
            serial: 0,
        }
    }

    fn context(&mut self) -> Context {
        self.clock += TimeDelta::milliseconds(1);
        Context {
            at: self.clock,
            recorder: None,
        }
    }

    fn push(&mut self, entry: Entry) {
        self.records.insert(entry.clone()).unwrap();
        self.entries.push(entry);
    }

    fn populate(&mut self, groups: usize) {
        for g in 0..groups {
            let top = self.create(Kind::Group, Lifecycle::NotStarted, None);
            for _ in 0..SUBGROUPS {
                let group = self.create(Kind::Group, Lifecycle::NotStarted, Some(&top));
                let mut issues = Vec::new();
                for i in 0..ISSUES {
                    let lifecycle = if i >= 8 {
                        Lifecycle::Undecided
                    } else {
                        Lifecycle::NotStarted
                    };
                    let issue = self.create(Kind::Issue, lifecycle, Some(&group));
                    match i {
                        0..=2 => {
                            self.perform(&issue, Operation::Start);
                            self.perform(&issue, Operation::Complete);
                        }
                        3 => self.perform(&issue, Operation::Start),
                        5..=7 => self.needs(&issue, &issues[i - 1]),
                        _ => {}
                    }
                    if i.is_multiple_of(2) {
                        let notes = if (g + i).is_multiple_of(10) { 6 } else { 1 };
                        for n in 0..notes {
                            self.note(&issue, n);
                        }
                    }
                    issues.push(issue);
                }
            }
        }
        for _ in 0..LOOSE_ISSUES {
            self.create(Kind::Issue, Lifecycle::Undecided, None);
        }
    }

    fn create(&mut self, kind: Kind, lifecycle: Lifecycle, parent: Option<&EntityId>) -> EntityId {
        self.serial += 1;
        let id = new_entity_id(&self.prefix).unwrap();
        let mut description = format!("作業 {} の背景と完了条件。", self.serial).repeat(12);
        if self.serial.is_multiple_of(7) {
            description.push_str("検索語を含む。");
        }
        let current = Current {
            kind,
            lifecycle,
            owner: None,
            title: format!("大量データの確認用の仕事 {}", self.serial),
            description,
            label: Label::ALL[self.serial % Label::ALL.len()],
            condition: None,
            parent: parent.cloned(),
            needs: BTreeSet::new(),
        };
        let context = self.context();
        let record = self.records.create(id.clone(), current, context).unwrap();
        self.push(Entry::Record(record));
        id
    }

    fn perform(&mut self, id: &EntityId, operation: Operation) {
        let context = self.context();
        let record = self.records.perform(id, operation, None, context).unwrap();
        self.push(Entry::Record(record));
    }

    fn needs(&mut self, id: &EntityId, target: &EntityId) {
        let context = self.context();
        let record = self
            .records
            .add_dependency(id, target, None, context)
            .unwrap()
            .unwrap();
        self.push(Entry::Record(record));
    }

    fn note(&mut self, id: &EntityId, n: usize) {
        let context = self.context();
        let body = format!("補足 {n}: 調べた結果と次に確かめること。").repeat(6);
        let note = self.records.add_note(id, body, None, context).unwrap();
        self.push(Entry::Note(note));
    }
}
