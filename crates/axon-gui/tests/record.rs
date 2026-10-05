//! Headless UI tests of recording work: creating Issues and Groups from the workbench, editing
//! the text and label of an Entity, and adding Notes, on independent temporary projects written
//! through the library. They cover refused input, writes refused under the lock, publications
//! of unknown outcome, repeated submissions and keeping drafts while the screen changes. Text
//! is typed through the test platform, not an OS input method.

use axon::lifecycle::{
    Context, EntityId, Kind, Label, Lifecycle, Line, LineProblem, Operation, Refusal, TITLE_LIMIT,
    record::{Current, Entry, RecordKind, Store, new_entity_id},
};
use axon_gui::{
    AxonApp, MIN_WINDOW_SIZE,
    app::{Found, OutcomeKind, entity_element},
    board::Rejection,
    project::{AppData, FaultPoint, ProjectConnection, ProjectId, WriteOutcome},
};
use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AppContext, Bounds, ElementId, Entity, Point, TestAppContext, VisualTestContext, WindowBounds,
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

    fn create(&self, kind: Kind, title: &str, parent: Option<&EntityId>) -> EntityId {
        let entry = self.write(|records, prefix| {
            let current = Current {
                kind,
                lifecycle: Lifecycle::NotStarted,
                owner: None,
                title: title.into(),
                description: "前の本文".into(),
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

    fn move_under(&self, id: &EntityId, parent: &EntityId) {
        self.write(|records, _| {
            Entry::Record(
                records
                    .set_parent(id, Some(parent.clone()), None, now())
                    .unwrap()
                    .unwrap(),
            )
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

    fn current(&self, id: &EntityId) -> Current {
        self.0
            .read(|_, _, view| view.current(id).unwrap().clone())
            .unwrap()
    }

    /// Every Entity with its current value, in no particular order.
    fn all(&self) -> Vec<(EntityId, Current)> {
        self.0
            .read(|_, records, view| {
                records
                    .records()
                    .map(|(_, record)| record.entity.clone())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .map(|id| {
                        let current = view.current(&id).unwrap().clone();
                        (id, current)
                    })
                    .collect()
            })
            .unwrap()
    }

    /// The number of records and Notes.
    fn entries(&self) -> usize {
        self.0.read(|_, records, _| records.len()).unwrap()
    }

    fn notes(&self, id: &EntityId) -> Vec<String> {
        self.0
            .read(|_, records, _| {
                axon::read::notes(records, id)
                    .into_iter()
                    .map(|(_, note)| note.body.clone())
                    .collect()
            })
            .unwrap()
    }

    fn kinds(&self, id: &EntityId) -> Vec<RecordKind> {
        self.0
            .read(|_, records, _| {
                records
                    .history(id)
                    .unwrap()
                    .into_iter()
                    .map(|(_, record)| record.kind.clone())
                    .collect()
            })
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

fn exists(handle: Window, id: impl Into<ElementId>, cx: &mut TestAppContext) -> bool {
    let id = id.into();
    let mut found = false;
    with_window(handle, cx, |window, _| {
        found = window.try_find(id).is_some()
    });
    found
}

fn open_entity(handle: Window, id: &EntityId, cx: &mut TestAppContext) {
    with_window(handle, cx, |window, cx| {
        window.within("entity-list").click(entity_element(id), cx)
    });
}

/// Types `text` where the focus is, a line break for each `\n`.
fn type_text(handle: Window, text: &str, cx: &mut TestAppContext) {
    for (ix, line) in text.split('\n').enumerate() {
        with_window(handle, cx, |window, cx| {
            if ix > 0 {
                window.press("enter", cx);
            }
            if !line.is_empty() {
                window.input(line, cx);
            }
        });
    }
}

/// The element of a multi-line editor.
fn editor(state: gpui_kit::EntityId) -> ElementId {
    ElementId::from(("input", state))
}

fn workbench_body(app: &Entity<AxonApp>, cx: &mut TestAppContext) -> ElementId {
    cx.read(|cx| {
        let workbench = app.read(cx).workbench().read(cx);
        editor(workbench.body().entity_id())
    })
}

/// The title and body the workbench holds.
fn workbench_text(app: &Entity<AxonApp>, cx: &mut TestAppContext) -> (String, String) {
    cx.read(|cx| {
        let workbench = app.read(cx).workbench().read(cx);
        (
            workbench.title().read(cx).value().to_string(),
            workbench.body().read(cx).value().to_string(),
        )
    })
}

fn fill_workbench(
    handle: Window,
    app: &Entity<AxonApp>,
    title: &str,
    body: &str,
    cx: &mut TestAppContext,
) {
    click(handle, "title", cx);
    type_text(handle, title, cx);
    let body_id = workbench_body(app, cx);
    click(handle, body_id, cx);
    type_text(handle, body, cx);
}

fn selected(app: &Entity<AxonApp>, cx: &mut TestAppContext) -> Option<EntityId> {
    cx.read(|cx| app.read(cx).explorer().selected().cloned())
}

fn outcome_of(app: &Entity<AxonApp>, cx: &mut TestAppContext) -> Option<OutcomeKind> {
    cx.read(|cx| {
        let app = app.read(cx);
        app.outcome()
            .or_else(|| app.create_outcome())
            .map(|o| o.kind.clone())
    })
}

fn refusal(app: &Entity<AxonApp>, cx: &mut TestAppContext) -> Refusal {
    match outcome_of(app, cx) {
        Some(OutcomeKind::Rejected(Rejection::Refused(refusal))) => refusal,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

/// Replaces what the title field of the edit form holds.
fn set_edit_title(handle: Window, app: &Entity<AxonApp>, title: &str, cx: &mut TestAppContext) {
    let input = cx.read(|cx| app.read(cx).edit_draft().unwrap().title.clone());
    let title = title.to_owned();
    with_window(handle, cx, |window, cx| {
        input.update(cx, |input, cx| input.set_value(title, window, cx))
    });
}

fn edit_body(app: &Entity<AxonApp>, cx: &mut TestAppContext) -> ElementId {
    cx.read(|cx| editor(app.read(cx).edit_draft().unwrap().body.entity_id()))
}

fn note_body(app: &Entity<AxonApp>, cx: &mut TestAppContext) -> ElementId {
    cx.read(|cx| editor(app.read(cx).note_draft().unwrap().entity_id()))
}

fn note_text(app: &Entity<AxonApp>, cx: &mut TestAppContext) -> Option<String> {
    cx.read(|cx| {
        app.read(cx)
            .note_draft()
            .map(|draft| draft.read(cx).value().to_string())
    })
}

fn edit_text(app: &Entity<AxonApp>, cx: &mut TestAppContext) -> Option<(String, String, Label)> {
    cx.read(|cx| {
        app.read(cx).edit_draft().map(|draft| {
            (
                draft.title.read(cx).value().to_string(),
                draft.body.read(cx).value().to_string(),
                draft.label,
            )
        })
    })
}

#[gpui_kit::test]
fn an_undecided_issue_is_created_with_feat_and_read_again_after_restart(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (project, seed) = Seed::new(&data, "読書会");
    let (handle, app) = open(&data, cx);
    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(
            (app.create_kind(), app.create_lifecycle()),
            (Kind::Issue, Lifecycle::Undecided)
        );
        assert_eq!(app.workbench().read(cx).label(), Label::Feat);
    });

    let body = "候補を三つ挙げる\n費用と広さを比べる\n".to_owned() + &"長い本文。".repeat(200);
    fill_workbench(handle, &app, "会場を決める", &body, cx);
    click(handle, "create-entity", cx);

    let all = seed.all();
    assert_eq!(all.len(), 1);
    let (id, current) = &all[0];
    assert_eq!(
        (
            current.kind,
            current.lifecycle,
            current.title.as_str(),
            current.description.as_str(),
            current.label,
            &current.parent
        ),
        (
            Kind::Issue,
            Lifecycle::Undecided,
            "会場を決める",
            body.as_str(),
            Label::Feat,
            &None
        )
    );
    assert!(id.as_ref().starts_with("axon-"));
    // The list and the detail show it, the workbench is emptied for the next one.
    assert_eq!(selected(&app, cx).as_ref(), Some(id));
    assert!(exists(handle, entity_element(id), cx));
    cx.read(|cx| {
        let explorer = app.read(cx).explorer();
        let detail = explorer.detail().unwrap().as_ref().unwrap();
        assert_eq!(detail.title, "会場を決める");
        assert_eq!(detail.history.len(), 1);
        assert_eq!(detail.history[0].kind, RecordKind::Created);
    });
    assert_eq!(workbench_text(&app, cx), (String::new(), String::new()));

    // Another window on the same data reads it from the disk.
    let (_, reopened) = open(&data, cx);
    cx.read(|cx| {
        let explorer = reopened.read(cx).explorer();
        assert_eq!(explorer.project(), Some(&project));
        assert_eq!(
            explorer.board().unwrap().item(id).unwrap().title,
            "会場を決める"
        );
    });
}

#[gpui_kit::test]
fn a_group_is_created_adopted_and_work_is_created_inside_it(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    let closed = seed.create(Kind::Group, "終わった会", None);
    seed.perform(&closed, Operation::Cancel);
    let (handle, app) = open(&data, cx);

    click(handle, "create-kind-group", cx);
    click(handle, "create-not-started", cx);
    fill_workbench(handle, &app, "秋の読書会", "", cx);
    click(handle, "create-entity", cx);
    let group = selected(&app, cx).expect("the new Group is open");
    let current = seed.current(&group);
    assert_eq!(
        (current.kind, current.lifecycle),
        (Kind::Group, Lifecycle::NotStarted)
    );

    click(handle, "create-inside", cx);
    assert_eq!(selected(&app, cx), None, "the workbench is shown");
    cx.read(|cx| assert_eq!(app.read(cx).create_parent(), Some(&group)));
    assert!(exists(handle, "create-parent", cx));
    click(handle, "create-kind-issue", cx);
    click(handle, "create-undecided", cx);
    // The title has the focus.
    type_text(handle, "会場を決める", cx);
    click(handle, "create-entity", cx);
    let child = selected(&app, cx).expect("the new Issue is open");
    let current = seed.current(&child);
    assert_eq!(
        (current.kind, current.lifecycle, current.parent),
        (Kind::Issue, Lifecycle::Undecided, Some(group.clone()))
    );
    cx.read(|cx| {
        let explorer = app.read(cx).explorer();
        let detail = explorer.detail().unwrap().as_ref().unwrap();
        assert_eq!(detail.parent.as_ref().map(|link| &link.id), Some(&group));
    });

    // Nothing is created inside a finished Group; the button says why.
    click(handle, "reset-filter", cx);
    with_window(handle, cx, |window, cx| {
        window.click("state-Cancelled", cx);
    });
    open_entity(handle, &closed, cx);
    assert!(exists(handle, "create-inside-blocked", cx));
    click(handle, "create-inside", cx);
    assert_eq!(selected(&app, cx), Some(closed));
}

#[gpui_kit::test]
fn a_refused_title_writes_nothing_and_keeps_what_was_typed(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    let (handle, app) = open(&data, cx);
    let body_id = workbench_body(&app, cx);
    click(handle, body_id, cx);
    type_text(handle, "タイトルを付け忘れた本文", cx);
    click(handle, "create-entity", cx);
    assert_eq!(
        refusal(&app, cx),
        Refusal::InvalidLine {
            field: Line::Title,
            problem: LineProblem::Empty
        }
    );
    assert!(exists(handle, "create-outcome", cx));
    assert_eq!(seed.entries(), 0);
    assert_eq!(
        workbench_text(&app, cx),
        (String::new(), "タイトルを付け忘れた本文".into())
    );

    let long = "長".repeat(TITLE_LIMIT + 1);
    click(handle, "title", cx);
    type_text(handle, &long, cx);
    click(handle, "create-entity", cx);
    assert_eq!(
        refusal(&app, cx),
        Refusal::InvalidLine {
            field: Line::Title,
            problem: LineProblem::TooLong {
                length: TITLE_LIMIT + 1,
                limit: TITLE_LIMIT
            }
        }
    );
    assert_eq!(seed.entries(), 0);
    assert_eq!(workbench_text(&app, cx).0, long);
    // Typing again puts the reason away.
    type_text(handle, "x", cx);
    assert_eq!(outcome_of(&app, cx), None);
}

#[gpui_kit::test]
fn an_edit_saves_title_body_and_label_in_one_record_and_keeps_relations(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    let group = seed.create(Kind::Group, "秋の読書会", None);
    let other = seed.create(Kind::Issue, "予算を決める", None);
    let issue = seed.create(Kind::Issue, "会場", None);
    let (handle, app) = open(&data, cx);
    open_entity(handle, &issue, cx);
    click(handle, "edit-entity", cx);
    assert_eq!(
        edit_text(&app, cx),
        Some(("会場".into(), "前の本文".into(), Label::Feat))
    );
    // Changed elsewhere after the screen read them: kept by the edit.
    seed.move_under(&issue, &group);
    seed.needs(&issue, &other);

    set_edit_title(handle, &app, "会場を決める", cx);
    let body = edit_body(&app, cx);
    click(handle, body, cx);
    type_text(handle, "\n追記した日本語の行", cx);
    click(handle, "edit-label", cx);
    with_window(handle, cx, |window, cx| {
        let index = Label::ALL.iter().position(|l| *l == Label::Bug).unwrap();
        window.within("popup-menu").click(index, cx);
    });
    let before = seed.entries();
    click(handle, "save-edit", cx);

    assert_eq!(seed.entries(), before + 1, "one record");
    assert_eq!(seed.kinds(&issue).last(), Some(&RecordKind::Import));
    let current = seed.current(&issue);
    assert_eq!(
        (
            current.title.as_str(),
            current.description.as_str(),
            current.label,
            current.parent.as_ref(),
            current.needs.contains(&other)
        ),
        (
            "会場を決める",
            "前の本文\n追記した日本語の行",
            Label::Bug,
            Some(&group),
            true
        )
    );
    // The form closes; the detail, the list and the history show the saved value.
    assert_eq!(edit_text(&app, cx), None);
    cx.read(|cx| {
        let explorer = app.read(cx).explorer();
        let detail = explorer.detail().unwrap().as_ref().unwrap();
        assert_eq!(
            (detail.title.as_str(), detail.label),
            ("会場を決める", Label::Bug)
        );
        assert_eq!(detail.history.last().unwrap().kind, RecordKind::Import);
        assert_eq!(
            explorer.board().unwrap().item(&issue).unwrap().title,
            "会場を決める"
        );
    });

    // The title alone is the CLI's text edit; an unchanged form writes nothing.
    click(handle, "edit-entity", cx);
    set_edit_title(handle, &app, "会場を予約する", cx);
    click(handle, "save-edit", cx);
    assert_eq!(seed.kinds(&issue).last(), Some(&RecordKind::Edit));
    let before = seed.entries();
    click(handle, "edit-entity", cx);
    click(handle, "save-edit", cx);
    assert_eq!(seed.entries(), before);
    assert_eq!(edit_text(&app, cx), None);
}

#[gpui_kit::test]
fn an_invalid_edit_and_a_refusal_under_the_lock_keep_the_draft(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    let issue = seed.create(Kind::Issue, "会場", None);
    let (handle, app) = open(&data, cx);
    open_entity(handle, &issue, cx);
    click(handle, "edit-entity", cx);
    set_edit_title(handle, &app, "  ", cx);
    let body = edit_body(&app, cx);
    click(handle, body, cx);
    type_text(handle, "書きかけ", cx);
    let before = seed.entries();
    click(handle, "save-edit", cx);
    assert_eq!(
        refusal(&app, cx),
        Refusal::InvalidLine {
            field: Line::Title,
            problem: LineProblem::Empty
        }
    );
    assert!(exists(handle, "edit-outcome", cx));
    assert_eq!(seed.entries(), before);
    assert_eq!(
        edit_text(&app, cx),
        Some(("  ".into(), "前の本文書きかけ".into(), Label::Feat))
    );

    // Ended elsewhere after the screen read it: refused under the lock, nothing written.
    set_edit_title(handle, &app, "会場を決める", cx);
    seed.perform(&issue, Operation::Cancel);
    let before = seed.entries();
    click(handle, "save-edit", cx);
    assert_eq!(refusal(&app, cx), Refusal::TerminalTextFixed);
    assert_eq!(seed.entries(), before);
    assert_eq!(seed.current(&issue).title, "会場");
    assert_eq!(
        edit_text(&app, cx),
        Some((
            "会場を決める".into(),
            "前の本文書きかけ".into(),
            Label::Feat
        ))
    );
    // The read after it shows the Entity ended; it is not edited again from the screen.
    cx.read(|cx| {
        let explorer = app.read(cx).explorer();
        let detail = explorer.detail().unwrap().as_ref().unwrap();
        assert_eq!(detail.stored, Some(Lifecycle::Cancelled));
        assert!(detail.editable.is_err());
    });
    click(handle, "cancel-edit", cx);
    assert_eq!(edit_text(&app, cx), None);
    assert!(exists(handle, "edit-blocked", cx));
    click(handle, "edit-entity", cx);
    assert_eq!(edit_text(&app, cx), None, "a terminal Entity is not edited");
}

#[gpui_kit::test]
fn notes_are_added_in_any_state_and_an_empty_one_is_refused(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    let issue = seed.create(Kind::Issue, "会場", None);
    seed.perform(&issue, Operation::Start);
    seed.perform(&issue, Operation::Complete);
    let (handle, app) = open(&data, cx);
    click(handle, "state-Completed", cx);
    open_entity(handle, &issue, cx);
    click(handle, "edit-entity", cx);
    assert_eq!(edit_text(&app, cx), None, "a terminal Entity is not edited");

    click(handle, "write-note", cx);
    type_text(handle, "  ", cx);
    click(handle, "add-note", cx);
    assert_eq!(refusal(&app, cx), Refusal::EmptyNote);
    assert!(seed.notes(&issue).is_empty());

    let body = note_body(&app, cx);
    click(handle, body, cx);
    type_text(handle, "\n会場は駅前に決まった\n費用は予算内", cx);
    click(handle, "add-note", cx);
    assert_eq!(
        seed.notes(&issue),
        ["  \n会場は駅前に決まった\n費用は予算内"]
    );
    assert_eq!(note_text(&app, cx), None, "the draft is saved");
    cx.read(|cx| {
        let explorer = app.read(cx).explorer();
        let detail = explorer.detail().unwrap().as_ref().unwrap();
        assert_eq!(detail.notes.len(), 1);
    });
    // No control edits or deletes a Note.
    assert!(!exists(handle, "edit-note", cx));
    assert!(!exists(handle, "delete-note", cx));
}

#[gpui_kit::test]
fn repeated_submissions_while_saving_write_once(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    let (handle, app) = open(&data, cx);
    fill_workbench(handle, &app, "会場を決める", "", cx);
    // Both clicks arrive before the write finishes.
    with_window(handle, cx, |window, cx| {
        window.click("create-entity", cx);
        window.render_frame(cx);
        window.click("create-entity", cx);
        window.render_frame(cx);
    });
    assert_eq!(seed.all().len(), 1);
    let issue = selected(&app, cx).unwrap();

    click(handle, "write-note", cx);
    type_text(handle, "一度だけ", cx);
    with_window(handle, cx, |window, cx| {
        window.click("add-note", cx);
        window.render_frame(cx);
        window.click("add-note", cx);
    });
    assert_eq!(seed.notes(&issue), ["一度だけ"]);
}

#[gpui_kit::test]
fn an_unknown_creation_is_read_again_and_never_sent_again(cx: &mut TestAppContext) {
    use std::sync::{Arc, atomic::AtomicBool, atomic::Ordering};
    for (point, made) in [
        (FaultPoint::BeforePublish, false),
        (FaultPoint::AfterPublish, true),
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
        let (handle, app) = open(&data, cx);
        fill_workbench(handle, &app, "会場を決める", "本文", cx);
        armed.store(true, Ordering::SeqCst);
        click(handle, "create-entity", cx);
        armed.store(false, Ordering::SeqCst);

        let expected = if made { Found::Made } else { Found::NotMade };
        let outcome = outcome_of(&app, cx);
        assert!(
            matches!(outcome, Some(OutcomeKind::Unknown { found, .. }) if found == expected),
            "{point:?}: {outcome:?}"
        );
        assert!(exists(handle, "create-outcome", cx), "{point:?}");
        assert_eq!(seed.all().len(), usize::from(made), "{point:?}");
        if made {
            // Found made, it is opened and the workbench emptied as a saved one is.
            assert_eq!(selected(&app, cx), Some(seed.all()[0].0.clone()));
            assert_eq!(workbench_text(&app, cx), (String::new(), String::new()));
        } else {
            assert_eq!(selected(&app, cx), None);
            assert_eq!(
                workbench_text(&app, cx),
                ("会場を決める".into(), "本文".into())
            );
            // Read and found not made, it can be created again.
            click(handle, "create-entity", cx);
            assert_eq!(seed.all().len(), 1);
        }
    }
}

#[gpui_kit::test]
fn a_failed_save_keeps_the_input_and_can_be_made_again(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (project, seed) = Seed::new(&data, "読書会");
    let issue = seed.create(Kind::Issue, "会場", None);
    let (handle, app) = open(&data, cx);
    open_entity(handle, &issue, cx);
    click(handle, "write-note", cx);
    type_text(handle, "残しておきたいこと", cx);

    let records = data.project_root(&project).join(".axon/records");
    let aside = records.with_extension("aside");
    std::fs::rename(&records, &aside).unwrap();
    click(handle, "add-note", cx);
    assert!(matches!(
        outcome_of(&app, cx),
        Some(OutcomeKind::NotApplied(_))
    ));
    assert_eq!(note_text(&app, cx).as_deref(), Some("残しておきたいこと"));

    std::fs::rename(&aside, &records).unwrap();
    click(handle, "add-note", cx);
    assert_eq!(seed.notes(&issue), ["残しておきたいこと"]);
    assert_eq!(note_text(&app, cx), None);
}

#[gpui_kit::test]
fn drafts_stay_through_switching_entities_projects_and_reading_again(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (first, seed) = Seed::new(&data, "読書会");
    let issue = seed.create(Kind::Issue, "会場", None);
    let other = seed.create(Kind::Issue, "予算", None);
    let (second, _) = Seed::new(&data, "家計");
    let (handle, app) = open(&data, cx);
    cx.read(|cx| assert!(!app.read(cx).has_unsaved(cx)));
    open_entity(handle, &issue, cx);
    click(handle, "edit-entity", cx);
    set_edit_title(handle, &app, "会場を決める（下書き）", cx);
    click(handle, "write-note", cx);
    type_text(handle, "Note の下書き", cx);
    cx.read(|cx| assert!(app.read(cx).has_unsaved(cx)));

    // Another Entity has neither draft.
    open_entity(handle, &other, cx);
    assert_eq!(edit_text(&app, cx), None);
    assert_eq!(note_text(&app, cx), None);

    // Another project, with something typed in the workbench there.
    app.update(cx, |app, cx| app.select(second.clone(), cx));
    cx.run_until_parked();
    fill_workbench(handle, &app, "家計簿をつける", "", cx);
    app.update(cx, |app, cx| app.select(first.clone(), cx));
    cx.run_until_parked();
    assert_eq!(workbench_text(&app, cx).0, "家計簿をつける");

    open_entity(handle, &issue, cx);
    click(handle, "reload-list", cx);
    assert_eq!(selected(&app, cx), Some(issue.clone()));
    assert_eq!(
        edit_text(&app, cx).map(|text| text.0),
        Some("会場を決める（下書き）".into())
    );
    assert_eq!(note_text(&app, cx).as_deref(), Some("Note の下書き"));
    assert!(exists(handle, "edit-form", cx));
    assert!(exists(handle, "note-form", cx));
}

#[gpui_kit::test]
fn closing_or_quitting_with_something_unsaved_asks_first(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    Seed::new(&data, "読書会");
    let (handle, app) = open(&data, cx);
    let mut window = VisualTestContext::from_window(handle.into(), cx);
    // Nothing typed: the window closes without asking.
    assert!(window.simulate_close());
    assert!(!window.has_pending_prompt());

    let (handle, app2) = open(&data, cx);
    drop(app);
    fill_workbench(handle, &app2, "会場を決める", "", cx);
    let mut window = VisualTestContext::from_window(handle.into(), cx);
    assert!(!window.simulate_close(), "kept open while asking");
    assert!(window.has_pending_prompt());
    window.simulate_prompt_answer("キャンセル");
    window.run_until_parked();
    assert_eq!(workbench_text(&app2, cx).0, "会場を決める");

    // Quitting asks too, also when the action arrives through the window, as cmd-q does.
    with_window(handle, cx, |window, cx| {
        window.dispatch_action(Box::new(axon_gui::Quit), cx)
    });
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("キャンセル");
    cx.run_until_parked();
    assert_eq!(workbench_text(&app2, cx).0, "会場を決める");

    let mut window = VisualTestContext::from_window(handle.into(), cx);
    assert!(!window.simulate_close());
    window.simulate_prompt_answer("破棄して終了");
    window.run_until_parked();
    assert!(
        cx.update_window(handle.into(), |_, _, _| ()).is_err(),
        "the window closed"
    );
}

#[gpui_kit::test]
fn the_smallest_window_keeps_the_forms_usable(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    let long = "とても長いタイトルでも編集と Note の欄が使えることを確かめる".repeat(3);
    let issue = seed.create(Kind::Issue, &long, None);
    let (handle, app) = open_sized(&data, MIN_WINDOW_SIZE.0, MIN_WINDOW_SIZE.1, cx);
    let body = workbench_body(&app, cx);
    let fits = |handle: Window, ids: Vec<ElementId>, cx: &mut TestAppContext| {
        with_window(handle, cx, |window, _| {
            let viewport = window.viewport_size();
            for id in ids {
                let element = window.find(id.clone());
                let bounds = element.bounds();
                assert!(
                    bounds.size.width >= px(20.) && bounds.size.height >= px(14.),
                    "{id:?} is too small to use: {bounds:?}"
                );
                assert!(
                    bounds.bottom_right().x <= viewport.width
                        && bounds.bottom_right().y <= viewport.height,
                    "{id:?} overflows the window: {bounds:?}"
                );
            }
        })
    };
    fits(
        handle,
        vec![
            "title".into(),
            "label".into(),
            body,
            "create-kind-group".into(),
            "create-not-started".into(),
            "create-entity".into(),
        ],
        cx,
    );

    // The forms of the detail are reached by scrolling it, and fit its width.
    open_entity(handle, &issue, cx);
    click(handle, "edit-entity", cx);
    let start_note = app.clone();
    with_window(handle, cx, |window, cx| {
        start_note.update(cx, |app, cx| app.start_note(window, cx))
    });
    let body = edit_body(&app, cx);
    let note = note_body(&app, cx);
    with_window(handle, cx, |window, _| {
        let viewport = window.viewport_size();
        for id in [
            "edit-title".into(),
            "edit-label".into(),
            body,
            "save-edit".into(),
            note,
            "add-note".into(),
        ] {
            let bounds = window.find(id.clone()).bounds();
            assert!(
                bounds.right() <= viewport.width && bounds.size.width >= px(20.),
                "{id:?} does not fit the pane: {bounds:?}"
            );
        }
    });
}

#[gpui_kit::test]
fn an_unknown_creation_stays_until_a_read_tells_what_it_left(cx: &mut TestAppContext) {
    use std::sync::{Arc, Mutex};
    let (_dir, data) = data();
    // Set once the project exists: the publication stops after writing, and the store
    // is moved away so the read after it fails too.
    let records: Arc<Mutex<Option<std::path::PathBuf>>> = Arc::default();
    let armed = records.clone();
    let data = data.with_fault(move |at| {
        if at == FaultPoint::AfterPublish
            && let Some(records) = armed.lock().unwrap().take()
        {
            std::fs::rename(&records, records.with_extension("aside")).unwrap();
            return Err(std::io::Error::other("injected"));
        }
        Ok(())
    });
    let (project, seed) = Seed::new(&data, "読書会");
    let (handle, app) = open(&data, cx);
    fill_workbench(handle, &app, "会場を決める", "", cx);
    let path = data.project_root(&project).join(".axon/records");
    *records.lock().unwrap() = Some(path.clone());
    click(handle, "create-entity", cx);

    assert!(matches!(
        outcome_of(&app, cx),
        Some(OutcomeKind::Unknown {
            found: Found::Pending,
            ..
        })
    ));
    // Neither put away nor sent again while no read has told.
    assert!(!exists(handle, "dismiss-create-outcome", cx));
    click(handle, "create-entity", cx);
    std::fs::rename(path.with_extension("aside"), &path).unwrap();
    assert_eq!(seed.all().len(), 1);
    assert_eq!(workbench_text(&app, cx).0, "会場を決める");

    click(handle, "reload", cx);
    assert!(matches!(
        outcome_of(&app, cx),
        Some(OutcomeKind::Unknown {
            found: Found::Made,
            ..
        })
    ));
    assert_eq!(workbench_text(&app, cx).0, "");
    assert_eq!(seed.all().len(), 1);
}

#[gpui_kit::test]
fn a_creation_saved_after_leaving_its_project_says_where_it_went(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (first, seed) = Seed::new(&data, "読書会");
    let (second, _) = Seed::new(&data, "家計");
    let group = seed.create(Kind::Group, "秋の読書会", None);
    let (handle, app) = open(&data, cx);
    app.update(cx, |app, cx| app.select(first.clone(), cx));
    cx.run_until_parked();
    open_entity(handle, &group, cx);
    click(handle, "create-inside", cx);
    type_text(handle, "会場を決める", cx);
    // The write runs on; the project changes before it ends.
    let switcher = app.clone();
    with_window(handle, cx, |window, cx| {
        window.click("create-entity", cx);
        switcher.update(cx, |app, cx| app.select(second.clone(), cx));
    });
    let created: Vec<_> = seed
        .all()
        .into_iter()
        .filter(|(_, current)| current.title == "会場を決める")
        .collect();
    assert_eq!(created.len(), 1);
    assert_eq!(created[0].1.parent.as_ref(), Some(&group));
    assert_eq!(workbench_text(&app, cx).0, "");
    assert!(exists(handle, "created-elsewhere", cx));

    // One that does not finish says so too, and keeps what was typed.
    app.update(cx, |app, cx| app.select(first.clone(), cx));
    cx.run_until_parked();
    click(handle, "title", cx);
    type_text(handle, "案内を送る", cx);
    let records = data.project_root(&first).join(".axon/records");
    std::fs::rename(&records, records.with_extension("aside")).unwrap();
    let switcher = app.clone();
    let away = second.clone();
    with_window(handle, cx, |window, cx| {
        window.click("create-entity", cx);
        switcher.update(cx, |app, cx| app.select(away, cx));
    });
    std::fs::rename(records.with_extension("aside"), &records).unwrap();
    assert_eq!(workbench_text(&app, cx).0, "案内を送る");
    let notice = cx.read(|cx| app.read(cx).explorer().project().cloned());
    assert_eq!(notice, Some(second.clone()));
    assert!(exists(handle, "created-elsewhere", cx));

    // "＋ 作成" starts an unrelated creation, outside the Group chosen before.
    app.update(cx, |app, cx| app.select(first.clone(), cx));
    cx.run_until_parked();
    open_entity(handle, &group, cx);
    click(handle, "create-inside", cx);
    cx.read(|cx| assert_eq!(app.read(cx).create_parent(), Some(&group)));
    click(handle, "new-entity", cx);
    cx.read(|cx| assert_eq!(app.read(cx).create_parent(), None));
}

#[gpui_kit::test]
fn putting_a_form_away_puts_its_refusal_away(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    let issue = seed.create(Kind::Issue, "会場", None);
    let (handle, app) = open(&data, cx);
    open_entity(handle, &issue, cx);
    click(handle, "edit-entity", cx);
    set_edit_title(handle, &app, "", cx);
    click(handle, "save-edit", cx);
    assert!(exists(handle, "edit-outcome", cx));
    click(handle, "cancel-edit", cx);
    assert!(!exists(handle, "edit-outcome", cx));

    click(handle, "write-note", cx);
    click(handle, "add-note", cx);
    assert_eq!(refusal(&app, cx), Refusal::EmptyNote);
    click(handle, "cancel-note", cx);
    assert!(!exists(handle, "note-outcome", cx));
}

#[gpui_kit::test]
fn a_field_changed_elsewhere_after_the_form_opened_is_not_overwritten_unasked(
    cx: &mut TestAppContext,
) {
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    let issue = seed.create(Kind::Issue, "会場", None);
    let (handle, app) = open(&data, cx);
    open_entity(handle, &issue, cx);
    click(handle, "edit-entity", cx);
    set_edit_title(handle, &app, "会場を決める", cx);
    seed.write(|records, _| {
        Entry::Record(
            records
                .write(
                    &issue,
                    Some("会場を予約する".into()),
                    Some("別の場所で書いた本文".into()),
                    None,
                    now(),
                )
                .unwrap()
                .unwrap(),
        )
    });
    click(handle, "reload-list", cx);
    let before = seed.entries();
    click(handle, "save-edit", cx);
    assert!(matches!(
        outcome_of(&app, cx),
        Some(OutcomeKind::Warning(_))
    ));
    assert!(exists(handle, "edit-outcome", cx));
    assert_eq!(seed.entries(), before);
    assert_eq!(seed.current(&issue).title, "会場を予約する");
    // Asked once, the second save writes what was typed, and only that.
    click(handle, "save-edit", cx);
    let current = seed.current(&issue);
    assert_eq!(current.title, "会場を決める");
    assert_eq!(current.description, "別の場所で書いた本文");
    assert_eq!(edit_text(&app, cx), None);
}
