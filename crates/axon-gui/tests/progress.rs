//! Headless UI tests of the state menu: lifecycle transitions chosen in the detail pane of
//! records written to independent temporary projects through the library, the reasons the core
//! refuses one, and the inputs that must not make a second change.

use axon::lifecycle::{
    Context, EntityId, Kind, Label, Lifecycle, Operation, Refusal,
    record::{Current, Entry, RecordKind, Store, new_entity_id},
};
use axon_gui::{
    AxonApp, MIN_WINDOW_SIZE,
    app::{Found, OutcomeKind, StoreState, entity_element},
    board::{Change, Exclusion, Rejection, State},
    project::{AppData, FaultPoint, ProjectConnection, ProjectId, WriteOutcome},
};
use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AppContext, Bounds, ElementId, Entity, InputEvent, KeyDownEvent, KeyUpEvent, Keystroke,
    MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point, TestAppContext, WindowBounds,
    WindowHandle, WindowOptions, base::Root, px, size,
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

    fn create_as(
        &self,
        kind: Kind,
        lifecycle: Lifecycle,
        title: &str,
        parent: Option<&EntityId>,
    ) -> EntityId {
        let entry = self.write(|records, prefix| {
            let current = Current {
                kind,
                lifecycle,
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

    fn create(&self, kind: Kind, title: &str, parent: Option<&EntityId>) -> EntityId {
        self.create_as(kind, Lifecycle::NotStarted, title, parent)
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

    fn lifecycle(&self, id: &EntityId) -> Lifecycle {
        self.read(id).0.lifecycle
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

fn exists(handle: Window, id: impl Into<ElementId>, cx: &mut TestAppContext) -> bool {
    let id = id.into();
    let mut found = false;
    with_window(handle, cx, |window, _| {
        found = window.try_find(id).is_some()
    });
    found
}

fn open_entity(handle: Window, id: &EntityId, cx: &mut TestAppContext) {
    click_in(handle, "entity-list", entity_element(id), cx);
}

fn item(operation: Operation) -> ElementId {
    ElementId::Name(format!("transition-{operation:?}").into())
}

fn open_menu(handle: Window, cx: &mut TestAppContext) {
    click(handle, "change-state", cx);
    with_window(handle, cx, |window, _| {
        assert!(window.find("popup-menu").visible())
    });
}

/// Chooses `operation` from the state menu with the mouse.
fn choose(handle: Window, operation: Operation, cx: &mut TestAppContext) {
    open_menu(handle, cx);
    click_in(handle, "popup-menu", item(operation), cx);
}

fn menu_closed(handle: Window, app: &Entity<AxonApp>, cx: &mut TestAppContext) -> bool {
    !exists(handle, "popup-menu", cx) && !cx.read(|cx| app.read(cx).is_state_menu_open())
}

/// The operations the open detail offers, with whether the core accepts each.
fn offered(app: &Entity<AxonApp>, cx: &mut TestAppContext) -> Vec<(Operation, bool)> {
    cx.read(|cx| {
        let detail = app.read(cx).explorer().detail().unwrap().as_ref().unwrap();
        detail
            .progress
            .iter()
            .map(|step| (step.operation, step.check.is_ok()))
            .collect()
    })
}

fn rejection(app: &Entity<AxonApp>, cx: &mut TestAppContext) -> Option<Rejection> {
    cx.read(|cx| match app.read(cx).outcome().map(|o| &o.kind) {
        Some(OutcomeKind::Rejected(rejection)) => Some(rejection.clone()),
        _ => None,
    })
}

fn shown_state(app: &Entity<AxonApp>, cx: &mut TestAppContext) -> State {
    cx.read(|cx| {
        app.read(cx)
            .explorer()
            .detail()
            .unwrap()
            .as_ref()
            .unwrap()
            .state
    })
}

fn selected_title(app: &Entity<AxonApp>, cx: &mut TestAppContext) -> Option<String> {
    cx.read(|cx| {
        let detail = app.read(cx).explorer().detail()?;
        Some(detail.as_ref().unwrap().title.clone())
    })
}

fn center(handle: Window, id: ElementId, cx: &mut TestAppContext) -> Point<Pixels> {
    let mut at = Point::default();
    with_window(handle, cx, |window, _| {
        at = window.find(id).bounds().center()
    });
    at
}

/// The second press of a double click at `position`, arriving after the first has been handled.
fn second_click(handle: Window, position: Point<Pixels>, cx: &mut TestAppContext) {
    with_window(handle, cx, |window, cx| {
        for event in [
            MouseDownEvent {
                button: MouseButton::Left,
                position,
                modifiers: Default::default(),
                click_count: 2,
                first_mouse: false,
            }
            .to_platform_input(),
            MouseUpEvent {
                button: MouseButton::Left,
                position,
                modifiers: Default::default(),
                click_count: 2,
            }
            .to_platform_input(),
        ] {
            window.dispatch_event(event, cx);
            window.render_frame(cx);
        }
    });
}

fn key(handle: Window, key: &str, down: bool, held: bool, cx: &mut TestAppContext) {
    let keystroke = Keystroke::parse(key).unwrap();
    with_window(handle, cx, |window, cx| {
        let event = if down {
            KeyDownEvent {
                keystroke,
                is_held: held,
                prefer_character_input: false,
            }
            .to_platform_input()
        } else {
            KeyUpEvent { keystroke }.to_platform_input()
        };
        window.dispatch_event(event, cx);
        window.render_frame(cx);
    });
}

#[gpui_kit::test]
fn an_issue_goes_through_its_lifecycle_from_the_menu(cx: &mut TestAppContext) {
    use Operation::*;
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    let issue = seed.create_as(Kind::Issue, Lifecycle::Undecided, "会場を決める", None);
    let (handle, app) = open(&data, cx);
    open_entity(handle, &issue, cx);
    assert_eq!(offered(&app, cx), [(Accept, true), (Cancel, true)]);

    for (operation, after) in [
        (Accept, Lifecycle::NotStarted),
        (Start, Lifecycle::InProgress),
        (Release, Lifecycle::NotStarted),
        (Start, Lifecycle::InProgress),
        (Complete, Lifecycle::Completed),
        (Reopen, Lifecycle::NotStarted),
        (Cancel, Lifecycle::Cancelled),
        (Reconsider, Lifecycle::Undecided),
    ] {
        let (_, before) = seed.read(&issue);
        choose(handle, operation, cx);
        assert!(menu_closed(handle, &app, cx), "a choice closes the menu");
        let (now, count) = seed.read(&issue);
        assert_eq!((now.lifecycle, count), (after, before + 1), "{operation:?}");
        // The list and the detail show the saved value.
        assert_eq!(shown_state(&app, cx), State::of(Some(after)));
        cx.read(|cx| {
            let explorer = app.read(cx).explorer();
            let detail = explorer.detail().unwrap().as_ref().unwrap();
            assert_eq!(
                detail.history.last().unwrap().kind,
                RecordKind::Transition(operation)
            );
            let board = explorer.board().unwrap();
            assert_eq!(board.item(&issue).unwrap().state, State::of(Some(after)));
        });
        if after == Lifecycle::Completed {
            // Out of the default filter, the detail stays and says why it is not listed.
            cx.read(|cx| {
                let explorer = app.read(cx).explorer();
                assert_eq!(explorer.listing().matched, 0);
                assert_eq!(
                    explorer.selected_exclusions(),
                    [Exclusion::State(State::Completed)]
                );
            });
            assert!(exists(handle, "detail-filtered-out", cx));
        }
    }
}

#[gpui_kit::test]
fn a_group_completes_only_as_the_final_confirmation(cx: &mut TestAppContext) {
    use Operation::*;
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    let group = seed.create(Kind::Group, "秋の読書会", None);
    let child = seed.create(Kind::Issue, "会場を決める", Some(&group));
    let (handle, app) = open(&data, cx);
    open_entity(handle, &group, cx);

    // Never started or released directly; completing waits for every child to end.
    assert_eq!(
        offered(&app, cx),
        [(Complete, false), (Withdraw, true), (Cancel, false)]
    );
    assert!(exists(handle, "group-confirmation", cx));
    open_menu(handle, cx);
    with_window(handle, cx, |window, _| {
        let menu = window.within("popup-menu");
        assert!(menu.try_find(item(Start)).is_none());
        assert!(menu.try_find(item(Release)).is_none());
        assert!(menu.try_find(item(Complete)).is_some());
    });
    // Choosing it anyway writes nothing and names the child, which opens from the reason.
    let (_, before) = seed.read(&group);
    click_in(handle, "popup-menu", item(Complete), cx);
    assert_eq!(
        rejection(&app, cx),
        Some(Rejection::Refused(Refusal::ChildrenNotEnded(vec![
            child.clone()
        ])))
    );
    assert_eq!(seed.read(&group).1, before);
    click_in(handle, "progress-outcome", entity_element(&child), cx);
    assert_eq!(selected_title(&app, cx).as_deref(), Some("会場を決める"));

    // Working through its child, the Group is InProgress and is not withdrawn.
    choose(handle, Start, cx);
    open_entity(handle, &group, cx);
    assert_eq!(shown_state(&app, cx), State::InProgress);
    assert_eq!(
        offered(&app, cx),
        [(Complete, false), (Withdraw, false), (Cancel, false)]
    );

    open_entity(handle, &child, cx);
    choose(handle, Complete, cx);
    open_entity(handle, &group, cx);
    assert_eq!(
        offered(&app, cx),
        [(Complete, true), (Withdraw, false), (Cancel, true)]
    );
    choose(handle, Complete, cx);
    assert_eq!(seed.lifecycle(&group), Lifecycle::Completed);
    assert_eq!(shown_state(&app, cx), State::Completed);
}

#[gpui_kit::test]
fn refusals_agree_with_the_core_and_name_what_to_resolve(cx: &mut TestAppContext) {
    use Operation::*;
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    let plan = seed.create_as(Kind::Group, Lifecycle::Undecided, "構想中の会", None);
    let idea = seed.create(Kind::Issue, "案を出す", Some(&plan));
    let venue = seed.create(Kind::Issue, "会場を決める", None);
    let invite = seed.create(Kind::Issue, "案内を送る", None);
    seed.needs(&invite, &venue);
    let closed = seed.create(Kind::Group, "終わった会", None);
    let leftover = seed.create(Kind::Issue, "残りの仕事", Some(&closed));
    seed.perform(&leftover, Cancel);
    seed.perform(&closed, Cancel);
    let (handle, app) = open(&data, cx);
    click(handle, "state-Cancelled", cx);

    for (entity, operation, refusal, related) in [
        (
            &idea,
            Start,
            Refusal::AncestorsNotAdopted(vec![plan.clone()]),
            &plan,
        ),
        (
            &invite,
            Start,
            Refusal::DependenciesNotCompleted(vec![venue.clone()]),
            &venue,
        ),
        (
            &leftover,
            Reconsider,
            Refusal::ParentClosed(closed.clone()),
            &closed,
        ),
    ] {
        open_entity(handle, entity, cx);
        let (_, before) = seed.read(entity);
        // The menu says why before anything is tried.
        open_menu(handle, cx);
        assert!(exists(handle, "transition-reason", cx));
        click_in(handle, "popup-menu", item(operation), cx);
        assert_eq!(
            rejection(&app, cx),
            Some(Rejection::Refused(refusal.clone()))
        );
        // The core refuses the same on the store itself, and nothing was written.
        let written = seed
            .0
            .read(|_, records, _| records.perform(entity, operation, None, now()).err())
            .unwrap()
            .and_then(|error| error.refusal().cloned());
        assert_eq!(written, Some(refusal));
        assert_eq!(seed.read(entity).1, before);
        click_in(handle, "progress-outcome", entity_element(related), cx);
        cx.read(|cx| assert_eq!(app.read(cx).explorer().selected(), Some(related)));
    }
}

#[gpui_kit::test]
fn the_records_under_the_lock_decide_and_the_screen_reads_them_again(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    let issue = seed.create(Kind::Issue, "案内を送る", None);
    let venue = seed.create(Kind::Issue, "会場を決める", None);
    let (handle, app) = open(&data, cx);
    open_entity(handle, &issue, cx);
    // A dependency appears after the screen was read, as from the CLI.
    seed.needs(&issue, &venue);

    // Chosen from the keyboard, so the focus stays on the menu button through the read.
    for _ in 0..60 {
        if trigger_focused(handle, cx) {
            break;
        }
        with_window(handle, cx, |window, cx| window.press("tab", cx));
    }
    with_window(handle, cx, |window, cx| window.press("enter", cx));
    with_window(handle, cx, |window, cx| window.press("down", cx));
    with_window(handle, cx, |window, cx| window.press("enter", cx));
    assert!(trigger_focused(handle, cx));
    assert_eq!(
        rejection(&app, cx),
        Some(Rejection::Refused(Refusal::DependenciesNotCompleted(vec![
            venue.clone()
        ])))
    );
    assert_eq!(seed.lifecycle(&issue), Lifecycle::NotStarted);
    cx.read(|cx| {
        let detail = app.read(cx).explorer().detail().unwrap().as_ref().unwrap();
        assert_eq!(detail.dependencies[0].id, venue, "the store was read again");
        assert_eq!(detail.state, State::NotStarted);
    });
}

#[gpui_kit::test]
fn a_double_click_on_a_choice_makes_one_change(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    let issue = seed.create(Kind::Issue, "会場を決める", None);
    let other = seed.create(Kind::Issue, "案内を送る", None);
    seed.needs(&issue, &other);
    seed.perform(&issue, Operation::Withdraw);
    let (handle, app) = open(&data, cx);
    open_entity(handle, &issue, cx);
    let (_, before) = seed.read(&issue);

    // The second press lands where the menu was, before and after the first change is saved.
    open_menu(handle, cx);
    let at = center(handle, item(Operation::Accept), cx);
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window
            .within("popup-menu")
            .click(item(Operation::Accept), cx);
    })
    .unwrap();
    // Still saving.
    cx.read(|cx| assert!(app.read(cx).is_saving()));
    second_click(handle, at, cx);
    assert_eq!(seed.read(&issue).1, before + 1);
    assert_eq!(seed.lifecycle(&issue), Lifecycle::NotStarted);
    assert!(menu_closed(handle, &app, cx));

    // Saved, the second press of a double click on a control behind does nothing either.
    for id in [
        ElementId::from("convert-entity"),
        ElementId::Name(format!("remove-dependency-{other}").into()),
    ] {
        let at = center(handle, id, cx);
        second_click(handle, at, cx);
    }
    let (now, after) = seed.read(&issue);
    assert_eq!(after, before + 1);
    assert_eq!(now.kind, Kind::Issue);
    assert_eq!(now.needs, BTreeSet::from([other.clone()]));
    // An ordinary click still acts.
    click(handle, "convert-entity", cx);
    assert_eq!(seed.read(&issue).0.kind, Kind::Group);
}

fn trigger_focused(handle: Window, cx: &mut TestAppContext) -> bool {
    let mut focused = false;
    with_window(handle, cx, |window, _| {
        focused = window.find("change-state").focused() == Some(true)
    });
    focused
}

/// Holds Enter down on the open menu with an item selected, lets it repeat and releases it.
fn hold_enter(handle: Window, cx: &mut TestAppContext) {
    key(handle, "enter", true, false, cx);
    for _ in 0..5 {
        key(handle, "enter", true, true, cx);
    }
    key(handle, "enter", false, false, cx);
}

#[gpui_kit::test]
fn a_held_key_makes_one_change_and_the_next_is_chosen_anew(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    let issue = seed.create(Kind::Issue, "会場を決める", None);
    seed.create(Kind::Issue, "案内を送る", None);
    let (handle, app) = open(&data, cx);
    open_entity(handle, &issue, cx);
    let (_, before) = seed.read(&issue);

    // From the keyboard alone: Tab to the menu button and open it.
    for _ in 0..60 {
        if trigger_focused(handle, cx) {
            break;
        }
        with_window(handle, cx, |window, cx| window.press("tab", cx));
    }
    assert!(trigger_focused(handle, cx));
    with_window(handle, cx, |window, cx| window.press("enter", cx));
    assert!(exists(handle, "popup-menu", cx));
    // The first item is Start; Enter chooses it on its press and the key stays down.
    with_window(handle, cx, |window, cx| window.press("down", cx));
    hold_enter(handle, cx);
    assert_eq!(seed.read(&issue).1, before + 1);
    assert_eq!(seed.lifecycle(&issue), Lifecycle::InProgress);
    assert!(
        menu_closed(handle, &app, cx),
        "the held key did not reopen it"
    );
    // The focus is back on the menu button, and the next change is chosen anew.
    assert!(trigger_focused(handle, cx));
    with_window(handle, cx, |window, cx| window.press("enter", cx));
    assert!(exists(handle, "popup-menu", cx));
    with_window(handle, cx, |window, cx| window.press("enter", cx));
    assert_eq!(seed.read(&issue).1, before + 1, "nothing is chosen yet");
    with_window(handle, cx, |window, cx| window.press("escape", cx));
    assert!(menu_closed(handle, &app, cx));

    // Opened with the mouse from the list, the focus goes back to the list.
    open_entity(handle, &issue, cx);
    open_menu(handle, cx);
    with_window(handle, cx, |window, cx| window.press("down", cx));
    hold_enter(handle, cx);
    assert_eq!(seed.read(&issue).1, before + 2);
    assert!(menu_closed(handle, &app, cx));
    with_window(handle, cx, |window, cx| window.press("down", cx));
    assert_eq!(selected_title(&app, cx).as_deref(), Some("案内を送る"));
}

#[gpui_kit::test]
fn a_press_behind_the_open_menu_only_closes_it(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    let issue = seed.create(Kind::Issue, "案内を送る", None);
    let venue = seed.create(Kind::Issue, "会場を決める", None);
    seed.needs(&issue, &venue);
    let (handle, app) = open(&data, cx);
    open_entity(handle, &issue, cx);
    let (_, before) = seed.read(&issue);

    open_menu(handle, cx);
    click(
        handle,
        ElementId::Name(format!("remove-dependency-{venue}").into()),
        cx,
    );
    assert!(menu_closed(handle, &app, cx));
    let (now, after) = seed.read(&issue);
    assert_eq!(after, before);
    assert_eq!(now.needs, BTreeSet::from([venue.clone()]));
}

#[gpui_kit::test]
fn nothing_is_chosen_while_a_change_is_saved(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (project, seed) = Seed::new(&data, "読書会");
    let issue = seed.create(Kind::Issue, "仕事", None);
    let (handle, app) = open(&data, cx);
    open_entity(handle, &issue, cx);
    let (_, before) = seed.read(&issue);
    let start = Change::Transition {
        entity: issue.clone(),
        operation: Operation::Start,
        to: Lifecycle::InProgress,
    };
    let cancel = Change::Transition {
        entity: issue.clone(),
        operation: Operation::Cancel,
        to: Lifecycle::Cancelled,
    };
    cx.update(|cx| {
        app.update(cx, |app, cx| {
            app.choose_transition(&project, start, false, cx);
            assert!(app.is_saving());
            app.choose_transition(&project, cancel, false, cx);
        })
    });
    with_window(handle, cx, |window, _| {
        // Saving is said beside the menu.
        assert!(window.find("progress-saving").visible());
    });
    cx.run_until_parked();
    assert_eq!(seed.read(&issue).1, before + 1);
    assert_eq!(seed.lifecycle(&issue), Lifecycle::InProgress);
}

#[gpui_kit::test]
fn a_failed_save_keeps_the_state_and_can_be_tried_again(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    let issue = seed.create(Kind::Issue, "仕事", None);
    let (handle, app) = open(&data, cx);
    open_entity(handle, &issue, cx);
    let root = cx.read(|cx| {
        let app = app.read(cx);
        app.data().project_root(&app.selected().unwrap().id)
    });
    let records = root.join(".axon/records");
    let aside = root.join(".axon/records-aside");
    std::fs::rename(&records, &aside).unwrap();

    choose(handle, Operation::Start, cx);
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(matches!(
            app.outcome().map(|o| &o.kind),
            Some(OutcomeKind::NotApplied(_))
        ));
        assert!(matches!(app.store(), StoreState::Loaded(_)));
    });
    // Not shown as made.
    assert_eq!(shown_state(&app, cx), State::NotStarted);
    assert!(exists(handle, "progress-outcome", cx));

    std::fs::rename(&aside, &records).unwrap();
    choose(handle, Operation::Start, cx);
    assert_eq!(seed.lifecycle(&issue), Lifecycle::InProgress);
    assert_eq!(shown_state(&app, cx), State::InProgress);
    cx.read(|cx| assert!(app.read(cx).outcome().is_none()));
}

#[gpui_kit::test]
fn an_unknown_publication_is_read_again_and_compared(cx: &mut TestAppContext) {
    use std::sync::{Arc, atomic::AtomicBool, atomic::Ordering};
    for (point, made, expected) in [
        (
            FaultPoint::BeforePublish,
            Lifecycle::NotStarted,
            Found::NotMade,
        ),
        (FaultPoint::AfterPublish, Lifecycle::InProgress, Found::Made),
    ] {
        let (_dir, data) = data();
        let armed = Arc::new(AtomicBool::new(false));
        let fault = armed.clone();
        let data = data.with_fault(move |at| {
            if at == point && fault.load(Ordering::SeqCst) {
                return Err(std::io::Error::other("injected"));
            }
            Ok(())
        });
        let (_, seed) = Seed::new(&data, "読書会");
        let issue = seed.create(Kind::Issue, "仕事", None);
        let (handle, app) = open(&data, cx);
        open_entity(handle, &issue, cx);
        let (_, before) = seed.read(&issue);
        armed.store(true, Ordering::SeqCst);
        choose(handle, Operation::Start, cx);
        armed.store(false, Ordering::SeqCst);

        let found = cx.read(|cx| match app.read(cx).outcome().map(|o| &o.kind) {
            Some(OutcomeKind::Unknown { found, .. }) => Some(*found),
            _ => None,
        });
        assert_eq!(found, Some(expected), "{point:?}");
        assert!(exists(handle, "progress-outcome", cx));
        let (now, after) = seed.read(&issue);
        assert_eq!(now.lifecycle, made);
        assert_eq!(shown_state(&app, cx), State::of(Some(made)));
        // Never sent again by itself.
        assert_eq!(after, before + usize::from(made == Lifecycle::InProgress));
    }
}

#[gpui_kit::test]
fn the_smallest_window_keeps_the_state_menu_usable(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    let long =
        "とても長いタイトルの Group でも状態の変更と編集のボタンが収まることを確かめる".repeat(2);
    let group = seed.create(Kind::Group, &long, None);
    seed.create(Kind::Issue, "子", Some(&group));
    let (handle, _app) = open_sized(&data, MIN_WINDOW_SIZE.0, MIN_WINDOW_SIZE.1, cx);
    open_entity(handle, &group, cx);
    open_menu(handle, cx);
    with_window(handle, cx, |window, _| {
        let viewport = window.viewport_size();
        let ids = [ElementId::from("change-state"), "edit-entity".into()];
        for id in ids {
            let bounds = window.find(id.clone()).bounds();
            assert!(
                bounds.size.width >= px(20.) && bounds.size.height >= px(14.),
                "{id:?} is too small to use: {bounds:?}"
            );
            assert!(
                bounds.bottom_right().x <= viewport.width,
                "{id:?} overflows the window: {bounds:?}"
            );
        }
        let trigger = window.find("change-state").bounds();
        let edit = window.find("edit-entity").bounds();
        assert!(
            trigger.right() <= edit.left(),
            "the state menu is left of edit"
        );
        let title = window.find("detail-title").bounds();
        assert!(
            trigger.top() >= title.bottom(),
            "the actions are under the title"
        );
        let menu = window.within("popup-menu");
        for operation in [Operation::Complete, Operation::Withdraw, Operation::Cancel] {
            let bounds = menu.find(item(operation)).bounds();
            assert!(
                bounds.bottom_right().x <= viewport.width
                    && bounds.bottom_right().y <= viewport.height,
                "{operation:?} overflows the window: {bounds:?}"
            );
        }
    });
}
