use super::*;

fn created(output: String) -> String {
    output.split_whitespace().next().unwrap().into()
}

#[test]
fn the_daily_workflow_runs_from_registration_to_group_completion() {
    let f = Fixture::new();
    f.ok(&["init", "trial"]);
    let group = created(f.ok(&[
        "capture",
        "--label",
        "chore",
        "--kind",
        "group",
        "--title",
        "納品",
        "-m",
        "全成果を検証",
    ]));
    let first = created(f.ok(&[
        "capture", "--label", "chore", "--title", "調査", "--parent", &group,
    ]));
    let second = created(f.ok(&[
        "capture",
        "--label",
        "chore",
        "--title",
        "実装",
        "--parent",
        &group,
        "--needs",
        &first,
        "-m",
        "保存本文",
    ]));
    assert!(f.ok(&["show", &second]).contains("Undecided"));
    f.ok(&["accept", &second]);
    let wait = f.ok(&["show", &second]);
    assert!(wait.contains("Required to start"));
    assert!(wait.contains(&format!("Ancestor must be adopted: {group}")));
    assert!(wait.contains("Dependency must complete:"));
    assert!(!wait.contains("Parent:"));
    assert!(failure(f.run(&["start", &second])).contains("all ancestor Groups must be adopted"));
    f.ok(&["accept", &group]);
    assert!(f.ok(&["show", &second]).contains("Parent:"));
    assert!(failure(f.run(&["start", &second])).contains("dependencies must be Completed"));
    assert!(f.ok(&["proposals"]).contains(&first));
    f.ok(&["accept", &first]);
    assert!(f.ok(&["proposals"]).is_empty());
    f.ok(&["condition", "set", &second, "--command", "exit 1"]);
    assert!(!f.ok(&["tasks"]).contains(&second));
    assert!(f.ok(&["list"]).contains(&second));
    f.ok(&["condition", "unset", &second]);
    assert!(f.ok(&["tasks"]).contains(&second));
    let rejected = failure(f.run(&["start", &group]));
    assert!(rejected.contains("not started directly"), "{rejected}");
    f.ok(&["start", &first]);
    assert!(
        f.ok(&["show", &group])
            .contains("Group  InProgress  chore  納品")
    );
    assert!(failure(f.run(&["release", &group])).contains("a Group is not released directly"));
    assert!(failure(f.run(&["start", &second])).contains("dependencies must be Completed"));
    f.ok(&["note", "add", &first, "-m", "調査結果"]);
    assert!(
        f.ok(&["note", "list", &first, "--recorder-details"])
            .contains("調査結果")
    );
    f.ok(&["complete", &first]);
    f.ok(&[
        "write",
        &second,
        "--title",
        "実装済みの目的",
        "-m",
        "編集本文",
    ]);
    f.ok(&["start", &second]);
    assert!(failure(f.run(&["complete", &group])).contains("all children must be terminal"));
    f.ok(&["note", "add", &second, "-m", "成果を統合・検証済み"]);
    let show = f.ok(&["show", &second]);
    assert!(show.starts_with(&second));
    assert!(show.contains("1 notes"));
    assert!(show.contains("編集本文"));
    assert!(!show.contains("成果を統合・検証済み"));
    assert!(!show.contains("Dependency must complete:"));
    f.ok(&["complete", &second, "--reason", "検証完了"]);
    let log = f.ok(&["log", &second]);
    assert!(log.contains("InProgress → Completed"));
    assert!(log.contains("検証完了"));
    assert!(!log.contains("record-"));
    let review = f.ok(&["show", &group]);
    assert!(review.contains("2/2 terminal (2 completed, 0 cancelled)"));
    assert!(review.contains("Awaiting final confirmation"));
    assert!(
        f.ok(&["tasks"])
            .contains(&format!("{group}  Group  Confirmable  chore  納品"))
    );
    assert!(
        f.ok(&["note", "list", &second])
            .contains("成果を統合・検証済み")
    );
    f.ok(&["complete", &group]);
    assert!(f.ok(&["tasks"]).is_empty());
    let log = f.ok(&["log", &group]);
    assert!(log.contains("NotStarted → Completed"));
    assert!(!log.contains("InProgress"));
    let list = f.ok(&["list"]);
    let positions: Vec<_> = [&group, &first, &second]
        .map(|id| list.find(id).unwrap())
        .into();
    assert!(positions.is_sorted(), "{list}");
    let files = f.record_files();
    assert!(failure(f.run(&["reconsider", &second])).contains("cannot Reconsider from Completed"));
    assert_eq!(files, f.record_files());
}

#[test]
fn unknown_commands_and_subcommand_help() {
    let f = Fixture::new();
    let rejected = f.run(&["no-such-command"]);
    assert_eq!(rejected.status.code(), Some(2));
    assert!(failure(rejected).contains("unrecognized subcommand"));
    let proposals_help = f.ok(&["proposals", "--help"]);
    for option in [
        "--kind",
        "--search",
        "--condition-timeout",
        "--trace-conditions",
    ] {
        assert!(proposals_help.contains(option));
    }
    let docs = f.ok(&["docs"]);
    assert!(docs.contains("axon proposals"));
    for args in [
        vec!["init", "--help"],
        vec!["storage", "check", "--help"],
        vec!["note", "list", "--help"],
    ] {
        f.ok(&args);
    }
    assert_eq!(f.run(&["merge", "--help"]).status.code(), Some(2));
    let init_help = f.ok(&["init", "--help"]);
    for text in [
        "Init creates .axon/records/, .axon/header.json, .axon/.gitignore and an .axon/.gitattributes holding only \"* -text\"",
        "which stops Git line-ending conversion of record files; it sets no merge attributes",
        "prints how to keep the store ignored or to track it",
        "does not create or edit the repository root's .gitignore, .gitattributes or Git config",
        "does not stage or commit any files",
    ] {
        assert!(init_help.contains(text), "missing from init help: {text}");
    }
}

/// The documented text with its line wrapping removed, so the wording is what is asserted.
fn unwrapped(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn docs_describe_the_one_store_and_the_discovery_order() {
    let f = Fixture::new();
    let docs = unwrapped(&f.ok(&["docs"]));
    for text in [
        // Init creates the store inside .axon/ with only a line-ending attribute; ignoring or
        // tracking it is the reader's choice.
        ".axon/records/, .axon/header.json, an .axon/.gitignore",
        "an .axon/.gitattributes holding only \"* -text\"",
        "keeps Git line-ending conversion (core.autocrlf, eol) from turning record files into corruption",
        "it sets no merge or union attributes",
        "never creates or edits the repository root's .gitignore, .gitattributes or Git config",
        "records from both sides without merge attributes or configuration",
    ] {
        assert!(docs.contains(text), "missing from docs: {text}");
    }
    // The current worktree's store is read before the main worktree's.
    let current = docs.find("current worktree root's .axon").unwrap();
    let main = docs.find("main worktree's .axon").unwrap();
    assert!(current < main, "{docs}");
    // Neither a second storage format nor a way to choose one is described.
    // Only merging is described as needing no attributes.
    for absent in ["--backend", "axon.db", "without any attributes"] {
        assert!(!docs.contains(absent), "{docs}");
    }
}
