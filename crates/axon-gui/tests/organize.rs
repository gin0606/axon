//! Headless UI tests of rearranging the structure from the detail pane: moving under a Group
//! and out of one, adding and removing dependencies and converting the kind, on records written
//! to independent temporary projects through the library.

use axon::lifecycle::{
    Context, EntityId, Kind, Label, Lifecycle, Operation, Refusal,
    record::{Current, Entry, RecordKind, Store, ViolationKind, new_entity_id},
};
use axon_gui::{
    AxonApp, MIN_WINDOW_SIZE,
    app::{Found, OutcomeKind, StoreState, entity_element},
    board::{Change, Purpose, Rejection, State},
    project::{AppData, FaultPoint, ProjectConnection, ProjectId, WriteOutcome},
};
use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AppContext, Bounds, ElementId, Entity, Point, TestAppContext, WindowBounds, WindowHandle,
    WindowOptions, base::Root, px, size,
};
use std::collections::BTreeSet;

type Window = WindowHandle<Root>;

fn data() -> (tempfile::TempDir, AppData) {
    let dir = tempfile::tempdir().unwrap();
    let data = AppData::at(dir.path().join("data")).unwrap();
    (dir, data)
}

/// Writes records to one project as the CLI would, one update each.
struct Seed(ProjectConnection);

impl Seed {
    fn new(data: &AppData, name: &str) -> (ProjectId, Self) {
        let registry = data.create_project(name).unwrap();
        let project = registry.projects().last().unwrap().clone();
        (project.id.clone(), Self(data.connect(&project)))
    }

    fn write(&self, entry: impl FnOnce(&Store, &str) -> Entry) -> Entry {
        match self.0.update(|header, records, _| {
            let entry = entry(records, &header.prefix);
            Ok((vec![entry.clone()], entry))
        }) {
            WriteOutcome::Applied(entry) => entry,
            other => panic!("{other:?}"),
        }
    }

    fn create(&self, kind: Kind, title: &str, parent: Option<&EntityId>) -> EntityId {
        let entry = self.write(|records, prefix| {
            let current = Current {
                kind,
                lifecycle: Lifecycle::NotStarted,
                owner: None,
                title: title.into(),
                description: String::new(),
                label: Label::Feat,
                condition: None,
                parent: parent.cloned(),
                needs: BTreeSet::new(),
            };
            let id = new_entity_id(prefix).unwrap();
            Entry::Record(records.create(id, current, now()).unwrap())
        });
        entry.entity().clone()
    }

    fn perform(&self, id: &EntityId, operation: Operation) {
        self.write(|records, _| {
            Entry::Record(records.perform(id, operation, None, now()).unwrap())
        });
    }

    fn needs(&self, id: &EntityId, target: &EntityId) {
        self.write(|records, _| {
            Entry::Record(
                records
                    .add_dependency(id, target, None, now())
                    .unwrap()
                    .unwrap(),
            )
        });
    }

    /// The current value of `id` and the number of records, as the store holds them now.
    fn read(&self, id: &EntityId) -> (Current, usize) {
        self.0
            .read(|_, records, view| (view.current(id).unwrap().clone(), records.records().count()))
            .unwrap()
    }
}

fn now() -> Context {
    Context {
        at: chrono::Utc::now(),
        recorder: None,
    }
}

fn open_sized(
    data: &AppData,
    width: f32,
    height: f32,
    cx: &mut TestAppContext,
) -> (Window, Entity<AxonApp>) {
    cx.update(axon_gui::init);
    let data = data.clone();
    let (window, app) = cx.update(|cx| {
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: Point::default(),
                size: size(px(width), px(height)),
            })),
            ..axon_gui::main_window_options(cx)
        };
        gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| AxonApp::new(data, window, cx))
        })
        .expect("open the main window")
    });
    cx.run_until_parked();
    (window.downcast::<Root>().expect("Base Root"), app)
}

fn open(data: &AppData, cx: &mut TestAppContext) -> (Window, Entity<AxonApp>) {
    open_sized(data, 1200., 900., cx)
}

fn with_window(
    handle: Window,
    cx: &mut TestAppContext,
    f: impl FnOnce(&mut gpui_kit::Window, &mut gpui_kit::App),
) {
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        f(window, cx)
    })
    .unwrap();
    cx.run_until_parked();
}

fn click(handle: Window, id: impl Into<ElementId>, cx: &mut TestAppContext) {
    let id = id.into();
    with_window(handle, cx, |window, cx| window.click(id, cx));
}

fn click_in(handle: Window, scope: &'static str, id: ElementId, cx: &mut TestAppContext) {
    with_window(handle, cx, |window, cx| window.within(scope).click(id, cx));
}

fn exists(handle: Window, id: &'static str, cx: &mut TestAppContext) -> bool {
    let mut found = false;
    with_window(handle, cx, |window, _| {
        found = window.try_find(id).is_some()
    });
    found
}

fn open_entity(handle: Window, id: &EntityId, cx: &mut TestAppContext) {
    click_in(handle, "entity-list", entity_element(id), cx);
}

/// The listed rows as (ID, depth).
fn tree(app: &Entity<AxonApp>, cx: &mut TestAppContext) -> Vec<(EntityId, usize)> {
    cx.read(|cx| {
        app.read(cx)
            .explorer()
            .listing()
            .rows
            .iter()
            .map(|row| (row.id.clone(), row.depth))
            .collect()
    })
}

fn rejection(app: &Entity<AxonApp>, cx: &mut TestAppContext) -> Option<Rejection> {
    cx.read(|cx| match app.read(cx).outcome().map(|o| &o.kind) {
        Some(OutcomeKind::Rejected(rejection)) => Some(rejection.clone()),
        _ => None,
    })
}

fn selected_title(app: &Entity<AxonApp>, cx: &mut TestAppContext) -> Option<String> {
    cx.read(|cx| {
        let detail = app.read(cx).explorer().detail()?;
        Some(detail.as_ref().unwrap().title.clone())
    })
}

#[gpui_kit::test]
fn a_group_moves_with_its_subtree_and_the_move_survives_a_restart(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    let prep = seed.create(Kind::Group, "準備", None);
    let venue = seed.create(Kind::Issue, "会場を決める", Some(&prep));
    let invite = seed.create(Kind::Issue, "案内を送る", Some(&prep));
    seed.needs(&invite, &venue);
    seed.perform(&venue, Operation::Start);
    let event = seed.create(Kind::Group, "秋の読書会", None);
    let (handle, app) = open(&data, cx);

    open_entity(handle, &prep, cx);
    click(handle, "move-entity", cx);
    cx.read(|cx| {
        let picker = app.read(cx).picker().unwrap();
        assert_eq!(picker.purpose, Purpose::Parent);
    });
    // Search, then choose the Group.
    with_window(handle, cx, |window, cx| window.input("秋", cx));
    click_in(handle, "picker", entity_element(&event), cx);

    assert_eq!(
        tree(&app, cx),
        [
            (event.clone(), 0),
            (prep.clone(), 1),
            (venue.clone(), 2),
            (invite.clone(), 2)
        ]
    );
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(app.picker().is_none(), "a made change closes the picker");
        assert!(app.outcome().is_none());
        let explorer = app.explorer();
        let detail = explorer.detail().unwrap().as_ref().unwrap();
        assert_eq!(detail.title, "準備");
        assert_eq!(detail.parent.as_ref().unwrap().id, event);
        // The started child keeps its state and makes the new parent line working.
        let board = explorer.board().unwrap();
        assert_eq!(board.item(&venue).unwrap().state, State::InProgress);
        assert_eq!(board.item(&event).unwrap().state, State::InProgress);
    });
    // The list has the focus back, so the arrow keys move through it again.
    with_window(handle, cx, |window, cx| window.press("down", cx));
    assert_eq!(selected_title(&app, cx).as_deref(), Some("会場を決める"));
    open_entity(handle, &prep, cx);
    let (invite_now, _) = seed.read(&invite);
    assert_eq!(invite_now.parent, Some(prep.clone()));
    assert_eq!(invite_now.needs, BTreeSet::from([venue.clone()]));

    // Taking it out again.
    click(handle, "detach-entity", cx);
    assert_eq!(seed.read(&prep).0.parent, None);

    // A new window on the same data, as after a restart, shows what was saved.
    let (_, again) = open(&data, cx);
    cx.read(|cx| {
        let board = again.read(cx).explorer().board().unwrap();
        assert_eq!(board.item(&prep).unwrap().parent, None);
        assert_eq!(board.item(&venue).unwrap().parent, Some(prep.clone()));
        assert_eq!(board.item(&event).unwrap().state, State::NotStarted);
    });
}

#[gpui_kit::test]
fn dependencies_are_added_and_removed_and_both_sides_show_it(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    let venue = seed.create(Kind::Issue, "会場を決める", None);
    let invite = seed.create(Kind::Issue, "案内を送る", None);
    let (handle, app) = open(&data, cx);

    open_entity(handle, &invite, cx);
    click(handle, "add-dependency", cx);
    click_in(handle, "picker", entity_element(&venue), cx);
    cx.read(|cx| {
        let explorer = app.read(cx).explorer();
        let detail = explorer.detail().unwrap().as_ref().unwrap();
        assert_eq!(detail.dependencies[0].id, venue);
        assert_eq!(detail.status, axon::read::Status::Blocked);
        let board = explorer.board().unwrap();
        assert_eq!(
            board.item(&invite).unwrap().status,
            axon::read::Status::Blocked
        );
    });
    click_in(handle, "detail-dependencies", entity_element(&venue), cx);
    cx.read(|cx| {
        let detail = app.read(cx).explorer().detail().unwrap().as_ref().unwrap();
        assert_eq!(detail.dependents[0].id, invite);
    });

    open_entity(handle, &invite, cx);
    click(
        handle,
        ElementId::Name(format!("remove-dependency-{venue}").into()),
        cx,
    );
    assert!(seed.read(&invite).0.needs.is_empty());
    cx.read(|cx| {
        let explorer = app.read(cx).explorer();
        let detail = explorer.detail().unwrap().as_ref().unwrap();
        assert!(detail.dependencies.is_empty());
        assert_eq!(detail.status, axon::read::Status::Ready);
    });
}

#[gpui_kit::test]
fn refused_changes_say_why_and_name_what_is_involved(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    let outer = seed.create(Kind::Group, "外", None);
    let inner = seed.create(Kind::Group, "内", Some(&outer));
    let first = seed.create(Kind::Issue, "一", Some(&inner));
    let lone = seed.create(Kind::Issue, "単独", None);
    seed.needs(&first, &lone);
    let (handle, app) = open(&data, cx);
    let (_, before) = seed.read(&outer);

    // A Group into its own child: a containment cycle, named with the destination.
    open_entity(handle, &outer, cx);
    click(handle, "move-entity", cx);
    click_in(handle, "picker", entity_element(&inner), cx);
    assert_eq!(
        rejection(&app, cx),
        Some(Rejection::Refused(Refusal::ContainmentCycle {
            destination: inner.clone()
        }))
    );
    click_in(handle, "structure-outcome", entity_element(&inner), cx);
    assert_eq!(selected_title(&app, cx).as_deref(), Some("内"));

    // A cycle through containment and dependencies: the Group completes after `first`, which
    // waits for `lone`, which would wait for the Group.
    open_entity(handle, &lone, cx);
    click(handle, "add-dependency", cx);
    click_in(handle, "picker", entity_element(&outer), cx);
    let Some(Rejection::Refused(Refusal::NewViolations(added))) = rejection(&app, cx) else {
        panic!("{:?}", rejection(&app, cx));
    };
    let (on_cycle, _) = added
        .iter()
        .find(|(_, kind)| *kind == ViolationKind::CompletionCycle)
        .unwrap_or_else(|| panic!("{added:?}"))
        .clone();
    // The Entity on the cycle is named and opens from the reason.
    click_in(handle, "structure-outcome", entity_element(&on_cycle), cx);
    cx.read(|cx| {
        assert_eq!(app.read(cx).explorer().selected(), Some(&on_cycle));
    });
    // Choosing an ancestor as a dependency.
    open_entity(handle, &first, cx);
    click(handle, "add-dependency", cx);
    click_in(handle, "picker", entity_element(&outer), cx);
    assert_eq!(
        rejection(&app, cx),
        Some(Rejection::Refused(Refusal::DependencyOnAncestor {
            entity: first.clone(),
            target: outer.clone()
        }))
    );
    // Nothing was written.
    assert_eq!(seed.read(&outer).1, before);

    // A Group with children is not converted, and the detail says so before anything is tried.
    open_entity(handle, &inner, cx);
    cx.read(|cx| {
        let detail = app.read(cx).explorer().detail().unwrap().as_ref().unwrap();
        assert_eq!(
            detail.structure.convert.1,
            Err(Rejection::Refused(Refusal::GroupWithChildren(vec![
                first.clone()
            ])))
        );
    });
    assert!(exists(handle, "convert-blocked", cx));
    click_in(handle, "convert-blocked", entity_element(&first), cx);
    assert_eq!(selected_title(&app, cx).as_deref(), Some("一"));
}

#[gpui_kit::test]
fn nothing_moves_out_of_or_into_a_finished_group(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    let done = seed.create(Kind::Group, "終わった会", None);
    let inside = seed.create(Kind::Issue, "中の仕事", Some(&done));
    seed.perform(&inside, Operation::Cancel);
    seed.perform(&done, Operation::Cancel);
    let lone = seed.create(Kind::Issue, "単独", None);
    let (handle, app) = open(&data, cx);
    // Show the finished Entities too.
    click(handle, "state-Cancelled", cx);

    open_entity(handle, &inside, cx);
    cx.read(|cx| {
        let detail = app.read(cx).explorer().detail().unwrap().as_ref().unwrap();
        assert_eq!(
            detail.structure.detach,
            Some(Err(Rejection::Refused(Refusal::ParentClosed(done.clone()))))
        );
    });
    assert!(exists(handle, "detach-blocked", cx));

    open_entity(handle, &lone, cx);
    click(handle, "move-entity", cx);
    click_in(handle, "picker", entity_element(&done), cx);
    assert_eq!(
        rejection(&app, cx),
        Some(Rejection::Refused(Refusal::DestinationNotOpenGroup(
            done.clone()
        )))
    );
    assert_eq!(seed.read(&lone).0.parent, None);
}

#[gpui_kit::test]
fn kinds_convert_where_the_core_allows_it(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    let issue = seed.create(Kind::Issue, "大きくなった仕事", None);
    let working = seed.create(Kind::Issue, "進行中", None);
    seed.perform(&working, Operation::Start);
    let (handle, app) = open(&data, cx);

    open_entity(handle, &issue, cx);
    click(handle, "convert-entity", cx);
    assert_eq!(seed.read(&issue).0.kind, Kind::Group);
    cx.read(|cx| {
        let explorer = app.read(cx).explorer();
        let detail = explorer.detail().unwrap().as_ref().unwrap();
        assert_eq!(detail.kind, Kind::Group);
        assert_eq!(detail.structure.convert, (Kind::Issue, Ok(())));
        assert_eq!(
            detail.history.last().unwrap().kind,
            RecordKind::Convert,
            "the history shows the conversion"
        );
    });
    // Back again.
    click(handle, "convert-entity", cx);
    assert_eq!(seed.read(&issue).0.kind, Kind::Issue);

    open_entity(handle, &working, cx);
    cx.read(|cx| {
        let detail = app.read(cx).explorer().detail().unwrap().as_ref().unwrap();
        assert_eq!(
            detail.structure.convert.1,
            Err(Rejection::Refused(Refusal::ConvertInProgress))
        );
    });
    assert!(exists(handle, "convert-blocked", cx));
}

#[gpui_kit::test]
fn another_project_s_entities_cannot_be_named(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    let issue = seed.create(Kind::Issue, "会場を決める", None);
    let (_, other) = Seed::new(&data, "家計簿");
    let foreign = other.create(Kind::Group, "家計簿の Group", None);
    let (handle, app) = open(&data, cx);
    open_entity(handle, &issue, cx);

    click(handle, "move-entity", cx);
    with_window(handle, cx, |window, _| {
        assert!(
            window
                .within("picker")
                .try_find(entity_element(&foreign))
                .is_none()
        );
    });
    let (_, before) = seed.read(&issue);
    let (_, foreign_before) = other.read(&foreign);
    for change in [
        Change::Move {
            entity: issue.clone(),
            parent: Some(foreign.clone()),
        },
        Change::AddDependency {
            entity: issue.clone(),
            target: foreign.clone(),
        },
    ] {
        cx.update(|cx| app.update(cx, |app, cx| app.apply(change, cx)));
        cx.run_until_parked();
        assert!(rejection(&app, cx).is_some());
    }
    assert_eq!(seed.read(&issue).1, before);
    assert_eq!(other.read(&foreign).1, foreign_before);
}

#[gpui_kit::test]
fn the_records_under_the_lock_decide_and_the_screen_reads_them_again(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    let group = seed.create(Kind::Group, "会", None);
    let (handle, app) = open(&data, cx);
    open_entity(handle, &group, cx);
    // A child appears after the screen was read, as from the CLI.
    let child = seed.create(Kind::Issue, "後から足した子", Some(&group));

    click(handle, "convert-entity", cx);
    assert_eq!(
        rejection(&app, cx),
        Some(Rejection::Refused(Refusal::GroupWithChildren(vec![
            child.clone()
        ])))
    );
    assert_eq!(seed.read(&group).0.kind, Kind::Group);
    cx.read(|cx| {
        let detail = app.read(cx).explorer().detail().unwrap().as_ref().unwrap();
        assert_eq!(detail.children[0].id, child, "the store was read again");
    });
}

#[gpui_kit::test]
fn one_change_is_written_at_a_time(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    let group = seed.create(Kind::Group, "会", None);
    let issue = seed.create(Kind::Issue, "仕事", None);
    let (handle, app) = open(&data, cx);
    open_entity(handle, &issue, cx);
    let (_, before) = seed.read(&issue);

    // A second change while the first is written does nothing, so a repeated click cannot
    // make another.
    cx.update(|cx| {
        app.update(cx, |app, cx| {
            app.apply(
                Change::Convert {
                    entity: issue.clone(),
                    kind: Kind::Group,
                },
                cx,
            );
            assert!(app.is_saving());
            app.apply(
                Change::Move {
                    entity: issue.clone(),
                    parent: Some(group.clone()),
                },
                cx,
            );
        })
    });
    cx.run_until_parked();
    let (now, after) = seed.read(&issue);
    assert_eq!(now.kind, Kind::Group);
    assert_eq!(now.parent, None);
    assert_eq!(after, before + 1);
    cx.read(|cx| assert!(!app.read(cx).is_saving()));
}

#[gpui_kit::test]
fn a_change_finishing_after_a_switch_stays_with_its_project(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (first, seed) = Seed::new(&data, "読書会");
    let issue = seed.create(Kind::Issue, "仕事", None);
    let (second, other) = Seed::new(&data, "家計簿");
    other.create(Kind::Issue, "家計簿をつける", None);
    let (handle, app) = open(&data, cx);
    cx.update(|cx| app.update(cx, |app, cx| app.select(first.clone(), cx)));
    cx.run_until_parked();
    open_entity(handle, &issue, cx);

    cx.update(|cx| {
        app.update(cx, |app, cx| {
            app.apply(
                Change::Convert {
                    entity: issue.clone(),
                    kind: Kind::Group,
                },
                cx,
            );
            app.select(second.clone(), cx);
        })
    });
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(matches!(app.store(), StoreState::Loaded(_)));
        assert_eq!(app.explorer().project(), Some(&second));
        assert!(app.explorer().selected().is_none());
        assert!(app.outcome().is_none());
        assert!(app.picker().is_none());
    });
    assert_eq!(seed.read(&issue).0.kind, Kind::Group);
    cx.update(|cx| app.update(cx, |app, cx| app.select(first.clone(), cx)));
    cx.run_until_parked();
    cx.read(|cx| {
        let board = app.read(cx).explorer().board().unwrap();
        assert_eq!(board.item(&issue).unwrap().kind, Kind::Group);
    });
}

#[gpui_kit::test]
fn the_smallest_window_keeps_the_structure_controls_usable(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    let long = "とても長いタイトルの Group が移動先の候補に並んでも収まることを確かめる".repeat(2);
    let group = seed.create(Kind::Group, "会", None);
    let far = seed.create(Kind::Group, &long, None);
    let issue = seed.create(Kind::Issue, "仕事", Some(&group));
    let target = seed.create(Kind::Issue, "依存先", None);
    seed.needs(&issue, &target);
    let (handle, _app) = open_sized(&data, MIN_WINDOW_SIZE.0, MIN_WINDOW_SIZE.1, cx);
    open_entity(handle, &issue, cx);
    click(handle, "move-entity", cx);
    with_window(handle, cx, |window, _| {
        let viewport = window.viewport_size();
        let remove: ElementId = ElementId::Name(format!("remove-dependency-{target}").into());
        for id in [
            ElementId::from("move-entity"),
            "detach-entity".into(),
            "convert-entity".into(),
            "picker-query".into(),
            "picker-cancel".into(),
        ]
        .into_iter()
        .chain([remove])
        {
            let element = window.find(id.clone());
            let bounds = element.bounds();
            assert!(
                bounds.size.width >= px(20.) && bounds.size.height >= px(14.),
                "{id:?} is too small to use: {bounds:?}"
            );
            assert!(
                bounds.bottom_right().x <= viewport.width,
                "{id:?} overflows the window: {bounds:?}"
            );
        }
        let candidate = window.within("picker").find(entity_element(&far));
        assert!(candidate.bounds().bottom_right().x <= viewport.width);
    });
}

#[gpui_kit::test]
fn a_failed_save_keeps_the_screen_and_can_be_tried_again(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    let issue = seed.create(Kind::Issue, "仕事", None);
    let other = seed.create(Kind::Issue, "別の仕事", None);
    let (handle, app) = open(&data, cx);
    open_entity(handle, &issue, cx);
    let root = cx.read(|cx| {
        let app = app.read(cx);
        app.data().project_root(&app.selected().unwrap().id)
    });
    let records = root.join(".axon/records");
    let aside = root.join(".axon/records-aside");
    std::fs::rename(&records, &aside).unwrap();

    click(handle, "convert-entity", cx);
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(
            matches!(
                app.outcome().map(|o| &o.kind),
                Some(OutcomeKind::NotApplied(_))
            ),
            "{:?}",
            app.outcome()
        );
        assert!(!app.is_saving());
        // Nothing is read again, so the screen and the selection stay.
        assert!(matches!(app.store(), StoreState::Loaded(_)));
        assert_eq!(app.explorer().selected(), Some(&issue));
    });
    assert!(exists(handle, "structure-outcome", cx));

    // Once the store is back the same change can be made.
    std::fs::rename(&aside, &records).unwrap();
    click(handle, "convert-entity", cx);
    assert_eq!(seed.read(&issue).0.kind, Kind::Group);
    cx.read(|cx| assert!(app.read(cx).outcome().is_none()));

    // The outcome of a change does not follow to another Entity.
    std::fs::rename(&records, &aside).unwrap();
    click(handle, "convert-entity", cx);
    cx.read(|cx| assert!(app.read(cx).outcome().is_some()));
    open_entity(handle, &other, cx);
    cx.read(|cx| assert!(app.read(cx).outcome().is_none()));
    std::fs::rename(&aside, &records).unwrap();
}

#[gpui_kit::test]
fn a_refusal_after_a_switch_is_shown_back_in_its_project(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (first, seed) = Seed::new(&data, "読書会");
    let group = seed.create(Kind::Group, "会", None);
    let (second, other) = Seed::new(&data, "家計簿");
    let elsewhere = other.create(Kind::Issue, "家計簿をつける", None);
    let (handle, app) = open(&data, cx);
    cx.update(|cx| app.update(cx, |app, cx| app.select(first.clone(), cx)));
    cx.run_until_parked();
    open_entity(handle, &group, cx);
    // A child appears after the screen was read, so the lock refuses the conversion.
    seed.create(Kind::Issue, "後から足した子", Some(&group));

    cx.update(|cx| {
        app.update(cx, |app, cx| {
            app.apply(
                Change::Convert {
                    entity: group.clone(),
                    kind: Kind::Issue,
                },
                cx,
            );
            app.select(second.clone(), cx);
        })
    });
    cx.run_until_parked();
    open_entity(handle, &elsewhere, cx);
    // Nothing of the other project's change shows here.
    assert!(!exists(handle, "structure-outcome", cx));
    assert!(!exists(handle, "structure-saving", cx));
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(!app.is_saving());
        assert!(app.outcome().is_none());
        assert_eq!(app.outcomes_of(&first).len(), 1);
    });
    // Making a change here leaves the other project's outcome alone.
    click(handle, "move-entity", cx);
    cx.read(|cx| assert_eq!(app.read(cx).outcomes_of(&first).len(), 1));

    // Back in the project it is shown above the list, and opens its Entity.
    cx.update(|cx| app.update(cx, |app, cx| app.select(first.clone(), cx)));
    cx.run_until_parked();
    assert!(exists(handle, "other-outcomes", cx));
    click_in(handle, "other-outcomes", entity_element(&group), cx);
    assert_eq!(selected_title(&app, cx).as_deref(), Some("会"));
    assert!(matches!(
        rejection(&app, cx),
        Some(Rejection::Refused(Refusal::GroupWithChildren(_)))
    ));
    assert!(exists(handle, "structure-outcome", cx));
    assert!(!exists(handle, "other-outcomes", cx));

    // Once seen there, leaving the project closes the detail and forgets it.
    cx.update(|cx| app.update(cx, |app, cx| app.select(second.clone(), cx)));
    cx.run_until_parked();
    cx.read(|cx| assert!(app.read(cx).outcomes_of(&first).is_empty()));
    cx.update(|cx| app.update(cx, |app, cx| app.select(first.clone(), cx)));
    cx.run_until_parked();
    assert!(!exists(handle, "other-outcomes", cx));
}

#[gpui_kit::test]
fn the_outcome_for_another_entity_shows_in_the_open_detail(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    let group = seed.create(Kind::Group, "会", None);
    let other = seed.create(Kind::Issue, "別の仕事", None);
    let (handle, app) = open(&data, cx);
    open_entity(handle, &group, cx);
    seed.create(Kind::Issue, "後から足した子", Some(&group));
    cx.update(|cx| {
        app.update(cx, |app, cx| {
            app.apply(
                Change::Convert {
                    entity: group.clone(),
                    kind: Kind::Issue,
                },
                cx,
            );
            app.open_entity(other.clone(), cx);
        })
    });
    cx.run_until_parked();
    assert_eq!(selected_title(&app, cx).as_deref(), Some("別の仕事"));
    assert!(exists(handle, "other-outcomes", cx));
    click_in(handle, "other-outcomes", entity_element(&group), cx);
    assert_eq!(selected_title(&app, cx).as_deref(), Some("会"));
    assert!(exists(handle, "structure-outcome", cx));
}

/// What the read after a publication of unknown outcome in the project on screen found.
fn found(app: &Entity<AxonApp>, cx: &mut TestAppContext) -> Option<Found> {
    cx.read(|cx| {
        let app = app.read(cx);
        let project = app.explorer().project()?;
        app.outcomes_of(project).iter().find_map(|o| match &o.kind {
            OutcomeKind::Unknown { found, .. } => Some(*found),
            _ => None,
        })
    })
}

/// Data whose writes fail at `point` while `armed` holds, after running `also`.
fn failing_writes(
    point: FaultPoint,
    also: impl Fn() + Send + Sync + 'static,
) -> (
    tempfile::TempDir,
    AppData,
    std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    use std::sync::{Arc, atomic::AtomicBool, atomic::Ordering};
    let (dir, data) = data();
    let armed = Arc::new(AtomicBool::new(false));
    let fault = armed.clone();
    let data = data.with_fault(move |at| {
        if at == point && fault.load(Ordering::SeqCst) {
            also();
            return Err(std::io::Error::other("injected"));
        }
        Ok(())
    });
    (dir, data, armed)
}

#[gpui_kit::test]
fn an_unknown_publication_is_read_again_and_compared(cx: &mut TestAppContext) {
    use std::sync::atomic::Ordering;
    for (point, made, expected) in [
        (FaultPoint::BeforePublish, Kind::Issue, Found::NotMade),
        (FaultPoint::AfterPublish, Kind::Group, Found::Made),
    ] {
        let (_dir, data, armed) = failing_writes(point, || {});
        let (_, seed) = Seed::new(&data, "読書会");
        let issue = seed.create(Kind::Issue, "仕事", None);
        let (handle, app) = open(&data, cx);
        open_entity(handle, &issue, cx);
        let (_, before) = seed.read(&issue);
        armed.store(true, Ordering::SeqCst);
        click(handle, "convert-entity", cx);
        armed.store(false, Ordering::SeqCst);

        assert_eq!(found(&app, cx), Some(expected), "{point:?}");
        assert!(exists(handle, "structure-outcome", cx));
        let (now, after) = seed.read(&issue);
        assert_eq!(now.kind, made);
        // Never sent again by itself.
        assert_eq!(after, before + usize::from(made == Kind::Group));
    }
}

#[gpui_kit::test]
fn an_unknown_publication_waits_for_a_read_that_succeeds(cx: &mut TestAppContext) {
    use std::sync::{Arc, Mutex, atomic::Ordering};
    // Where the store's records go aside, so the read after the publication fails.
    let records: Arc<Mutex<Option<std::path::PathBuf>>> = Arc::default();
    let aside = records.clone();
    let (_dir, data, armed) = failing_writes(FaultPoint::AfterPublish, move || {
        if let Some(records) = aside.lock().unwrap().as_ref() {
            std::fs::rename(records, records.with_extension("aside")).unwrap();
        }
    });
    let (_, seed) = Seed::new(&data, "読書会");
    let issue = seed.create(Kind::Issue, "仕事", None);
    let (handle, app) = open(&data, cx);
    open_entity(handle, &issue, cx);
    let root = cx.read(|cx| {
        let app = app.read(cx);
        app.data().project_root(&app.selected().unwrap().id)
    });
    let path = root.join(".axon/records");
    *records.lock().unwrap() = Some(path.clone());
    armed.store(true, Ordering::SeqCst);
    click(handle, "convert-entity", cx);
    armed.store(false, Ordering::SeqCst);
    cx.read(|cx| assert!(matches!(app.read(cx).store(), StoreState::Failed(_))));
    assert_eq!(found(&app, cx), Some(Found::Pending));

    // The reload the message asks for keeps the outcome and tells what the change left.
    std::fs::rename(path.with_extension("aside"), &path).unwrap();
    click(handle, "reload", cx);
    assert_eq!(found(&app, cx), Some(Found::Made));
    // The failed read closed the detail, so the result shows above the list.
    assert!(exists(handle, "other-outcomes", cx));
    click_in(handle, "other-outcomes", entity_element(&issue), cx);
    assert_eq!(selected_title(&app, cx).as_deref(), Some("仕事"));
    assert!(exists(handle, "structure-outcome", cx));
}

#[gpui_kit::test]
fn an_outcome_for_an_entity_gone_from_the_records_is_still_shown(cx: &mut TestAppContext) {
    use std::sync::{Arc, Mutex, atomic::Ordering};
    // Its records go away outside the application while the write stops midway.
    let records: Arc<Mutex<Option<std::path::PathBuf>>> = Arc::default();
    let gone = records.clone();
    let (_dir, data, armed) = failing_writes(FaultPoint::BeforePublish, move || {
        if let Some(records) = gone.lock().unwrap().as_ref() {
            for entry in std::fs::read_dir(records).unwrap() {
                std::fs::remove_dir_all(entry.unwrap().path()).unwrap();
            }
        }
    });
    let (_, seed) = Seed::new(&data, "読書会");
    let issue = seed.create(Kind::Issue, "仕事", None);
    let (handle, app) = open(&data, cx);
    open_entity(handle, &issue, cx);
    let root = cx.read(|cx| {
        let app = app.read(cx);
        app.data().project_root(&app.selected().unwrap().id)
    });
    *records.lock().unwrap() = Some(root.join(".axon/records"));
    armed.store(true, Ordering::SeqCst);
    click(handle, "convert-entity", cx);
    armed.store(false, Ordering::SeqCst);
    assert_eq!(found(&app, cx), Some(Found::Undetermined));
    cx.read(|cx| assert!(app.read(cx).explorer().selected().is_none()));
    assert!(exists(handle, "other-outcomes", cx));
    click(
        handle,
        ElementId::Name(format!("dismiss-outcome-{issue}").into()),
        cx,
    );
    assert!(!exists(handle, "other-outcomes", cx));
}

#[gpui_kit::test]
fn an_outcome_never_drawn_is_not_forgotten(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    let group = seed.create(Kind::Group, "会", None);
    seed.create(Kind::Issue, "子", Some(&group));
    let other = seed.create(Kind::Issue, "別の仕事", None);
    let (handle, app) = open(&data, cx);
    open_entity(handle, &group, cx);
    // Refused and then left before any frame drew the reason.
    cx.update(|cx| {
        app.update(cx, |app, cx| {
            app.apply(
                Change::Convert {
                    entity: group.clone(),
                    kind: Kind::Issue,
                },
                cx,
            );
            app.open_entity(other.clone(), cx);
        })
    });
    cx.run_until_parked();
    assert!(exists(handle, "other-outcomes", cx));
    // Drawn in its own detail and left, it is forgotten.
    click_in(handle, "other-outcomes", entity_element(&group), cx);
    assert!(exists(handle, "structure-outcome", cx));
    open_entity(handle, &other, cx);
    assert!(!exists(handle, "other-outcomes", cx));
}

#[gpui_kit::test]
fn a_move_found_made_after_an_unknown_publication_closes_its_picker(cx: &mut TestAppContext) {
    use std::sync::atomic::Ordering;
    let (_dir, data, armed) = failing_writes(FaultPoint::AfterPublish, || {});
    let (_, seed) = Seed::new(&data, "読書会");
    let issue = seed.create(Kind::Issue, "仕事", None);
    let group = seed.create(Kind::Group, "会", None);
    let (handle, app) = open(&data, cx);
    open_entity(handle, &issue, cx);
    click(handle, "move-entity", cx);
    armed.store(true, Ordering::SeqCst);
    click_in(handle, "picker", entity_element(&group), cx);
    armed.store(false, Ordering::SeqCst);
    assert_eq!(found(&app, cx), Some(Found::Made));
    assert_eq!(seed.read(&issue).0.parent, Some(group.clone()));
    cx.read(|cx| assert!(app.read(cx).picker().is_none()));
}

#[gpui_kit::test]
fn closing_the_detail_with_a_picker_open_gives_the_list_the_keys(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    let first = seed.create(Kind::Issue, "一", None);
    seed.create(Kind::Issue, "二", None);
    let (handle, app) = open(&data, cx);
    open_entity(handle, &first, cx);
    click(handle, "add-dependency", cx);
    click(handle, "close-detail", cx);
    with_window(handle, cx, |window, cx| window.press("down", cx));
    assert_eq!(selected_title(&app, cx).as_deref(), Some("一"));
}
