use crate::common::{TestRepo, assert_failure, assert_success, stdout};
use std::fs;

#[test]
fn manual_set_and_clear_preserve_every_progress_and_disposition_for_both_kinds() {
    for (group, progress, disposition) in [
        (false, "not_started", "accepted"),
        (true, "in_progress", "undecided"),
        (false, "ended", "rejected"),
    ] {
        let repo = TestRepo::new();
        repo.init("test");
        let id = if group {
            repo.group_plan("wait")
        } else {
            repo.plan("wait")
        };
        if progress != "not_started" {
            assert_success(&repo.axon(&["start", &id]));
        }
        if progress == "ended" {
            assert_success(&repo.axon(&["done", &id]));
        }
        match disposition {
            "undecided" => repo.undecide(&id),
            "rejected" => {
                assert_success(&repo.axon(&["decide", "reject", &id]));
            }
            _ => {}
        }
        let before = repo.snapshot(&id);
        let claims = stdout(&repo.axon(&["claims"]));
        let revision = stdout(&repo.axon(&["revision", "list", &id]));
        assert_success(&repo.axon(&["when", "manual", &id, "-r", "external preparation"]));
        let manual = repo.snapshot(&id);
        assert_eq!(manual.progress, before.progress);
        assert_eq!(manual.disposition, before.disposition);
        assert_eq!(manual.progress_events, before.progress_events);
        assert_eq!(manual.decision_events, before.decision_events + 1);
        assert_eq!(manual.resurface_kind.as_deref(), Some("manual"));
        assert!(manual.resurface_date.is_none());
        assert!(manual.resurface_ref.is_none());
        assert!(manual.resurface_command.is_none());
        assert_eq!(stdout(&repo.axon(&["claims"])), claims);
        assert_eq!(stdout(&repo.axon(&["revision", "list", &id])), revision);
        assert!(stdout(&repo.axon(&["show", &id])).contains("Surfaced: no"));
        assert!(stdout(&repo.axon(&["list"])).contains(&id));
        assert!(!stdout(&repo.axon(&["ready"])).contains(&id));
        assert!(!stdout(&repo.axon(&["triage"])).contains(&id));
        assert_failure(&repo.axon(&["when", "manual", &id]));
        assert_eq!(repo.snapshot(&id), manual);
        assert_success(&repo.axon(&["when", "clear", &id, "-r", "preparation complete"]));
        let cleared = repo.snapshot(&id);
        assert_eq!(cleared.progress, before.progress);
        assert_eq!(cleared.disposition, before.disposition);
        assert_eq!(cleared.progress_events, before.progress_events);
        assert!(cleared.resurface_kind.is_none());
        assert_eq!(stdout(&repo.axon(&["claims"])), claims);
        assert_eq!(stdout(&repo.axon(&["revision", "list", &id])), revision);
        let log = stdout(&repo.axon(&["log", &id]));
        assert!(log.contains("Always -> Manual"));
        assert!(log.contains("Manual -> Always"));
        assert!(log.contains("external preparation"));
        assert!(log.contains("preparation complete"));
        let ready = stdout(&repo.axon(&["ready"]));
        assert_eq!(
            ready.contains(&id),
            progress == "not_started" && disposition == "accepted"
        );
        let triage = stdout(&repo.axon(&["triage"]));
        assert_eq!(
            triage.contains(&id),
            progress != "ended" && disposition == "undecided"
        );
    }
}

#[test]
fn manual_group_closes_active_scope_without_changing_descendants_or_claims() {
    let repo = TestRepo::new();
    repo.init("test");
    let group = repo.group_plan("gate");
    let running = repo.plan("running");
    let ready = repo.plan("ready");
    let undecided = repo.plan("decision");
    for child in [&running, &ready, &undecided] {
        repo.set_parent(child, &group);
    }
    repo.undecide(&undecided);
    assert_success(&repo.axon(&["start", &group]));
    assert_success(&repo.axon(&["start", &running]));
    let snapshots = [&running, &ready, &undecided].map(|id| repo.snapshot(id));
    let claims = stdout(&repo.axon(&["claims"]));
    assert_success(&repo.axon(&["when", "manual", &group]));
    assert_eq!(
        [&running, &ready, &undecided].map(|id| repo.snapshot(id)),
        snapshots
    );
    assert_eq!(stdout(&repo.axon(&["claims"])), claims);
    assert!(stdout(&repo.axon(&["show", &ready])).contains("Active scope: no"));
    assert!(stdout(&repo.axon(&["ready"])).is_empty());
    assert!(stdout(&repo.axon(&["triage"])).is_empty());
    assert_success(&repo.axon(&["when", "clear", &group]));
    assert!(stdout(&repo.axon(&["ready"])).contains(&ready));
    assert!(stdout(&repo.axon(&["triage"])).contains(&undecided));
    assert_eq!(
        [&running, &ready, &undecided].map(|id| repo.snapshot(id)),
        snapshots
    );
}

#[test]
fn manual_replaces_every_condition_and_removes_after_entity_wait_edges() {
    let repo = TestRepo::new();
    repo.init("test");
    for group in [false, true] {
        let id = if group {
            repo.group_plan("wait")
        } else {
            repo.plan("wait")
        };
        let target = repo.plan("target");
        for condition in [
            vec!["when", "at", &id, "2099-01-01T00:00:00Z"],
            vec!["when", "after", &id, &target],
            vec!["when", "command", &id, "exit 7"],
        ] {
            assert_success(&repo.axon(&condition));
            assert_success(&repo.axon(&["when", "manual", &id]));
            let stored = repo.snapshot(&id);
            assert!(stored.resurface_date.is_none());
            assert!(stored.resurface_ref.is_none());
            assert!(stored.resurface_command.is_none());
            assert!(stdout(&repo.axon(&["show", &id])).contains("Surfaced: no"));
        }
        assert_success(&repo.axon(&["when", "command", &id, "exit 0"]));
        assert!(stdout(&repo.axon(&["ready"])).contains(&id));
        assert_success(&repo.axon(&["when", "manual", &id]));
        assert_success(&repo.axon(&["when", "after", &target, &id]));
        assert_failure(&repo.axon(&["when", "after", &id, &target]));
        assert_eq!(repo.snapshot(&id).resurface_kind.as_deref(), Some("manual"));
    }
}

#[test]
fn manual_export_import_preserves_control_and_rejects_edits_and_stale_snapshots() {
    let repo = TestRepo::new();
    repo.init("test");
    let id = repo.capture("before title");
    assert_success(&repo.axon(&["when", "manual", &id]));
    let exported = stdout(&repo.axon(&["export", &id]));
    assert!(exported.contains("kind: manual"));
    let path = repo.root().join("manual.yaml");
    let file = path.to_str().unwrap();
    fs::write(&path, exported.replace("before title", "after title")).unwrap();
    assert_success(&repo.axon(&["import", "check", file]));
    assert_success(&repo.axon(&["import", "apply", file]));
    assert_eq!(repo.snapshot(&id).resurface_kind.as_deref(), Some("manual"));
    let current = fs::read_to_string(&path).unwrap();
    fs::write(&path, current.replace("kind: manual", "kind: always")).unwrap();
    assert_failure(&repo.axon(&["import", "check", file]));
    assert_eq!(repo.snapshot(&id).resurface_kind.as_deref(), Some("manual"));
    fs::write(&path, &current).unwrap();
    assert_success(&repo.axon(&["when", "clear", &id]));
    assert_failure(&repo.axon(&["import", "apply", file]));
    assert!(repo.snapshot(&id).resurface_kind.is_none());
}

#[test]
fn manual_help_and_completion_are_discoverable() {
    let repo = TestRepo::new();
    let help = repo.axon(&["when", "manual", "--help"]);
    assert_success(&help);
    assert!(stdout(&help).contains("unsurfaced"));
    assert!(stdout(&help).contains("--reason"));
    let docs = repo.axon(&["docs"]);
    assert_success(&docs);
    assert!(stdout(&docs).contains("Manual"));
    let completion = repo.axon(&["completion", "bash"]);
    assert_success(&completion);
    assert!(stdout(&completion).contains("manual"));
}

#[test]
fn clearing_manual_keeps_dependency_blocking_and_orphaned_triage() {
    let repo = TestRepo::new();
    repo.init("test");
    let id = repo.plan("dependent");
    let target = repo.plan("prerequisite");
    repo.add_dependency(&id, &target);
    assert_success(&repo.axon(&["when", "manual", &id]));
    assert_success(&repo.axon(&["when", "clear", &id]));
    assert!(!stdout(&repo.axon(&["ready"])).contains(&id));
    assert!(stdout(&repo.axon(&["show", &id])).contains("Blocked: yes"));
    assert_success(&repo.axon(&["when", "manual", &id]));
    assert_success(&repo.axon(&["decide", "reject", &target]));
    assert!(!stdout(&repo.axon(&["triage"])).contains(&id));
    assert_success(&repo.axon(&["when", "clear", &id]));
    assert!(stdout(&repo.axon(&["triage"])).contains(&id));
    assert!(!stdout(&repo.axon(&["ready"])).contains(&id));
}
