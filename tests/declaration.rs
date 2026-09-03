mod common;

use common::{TestRepo, assert_failure, assert_success, stderr, stdout};
use std::fs;
use std::path::{Path, PathBuf};

fn plan_path(repo: &TestRepo, name: &str) -> PathBuf {
    repo.root().join(name)
}

fn write(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
}

fn assert_show_relation(output: &str, label: &str, id: &str) {
    assert!(
        output.lines().any(|line| {
            line.trim_start().starts_with(label) && line.split_whitespace().any(|field| field == id)
        }),
        "missing {label} for {id} in:\n{output}"
    );
}

fn new_plan(dependencies: &str) -> String {
    format!(
        r#"schema: axon-plan/v2
issues:
  - id: null
    key: api
    base: null
    title: import API
    description: |-
      Parse and apply one declaration.
      Keep the transaction atomic.
    observed:
      progress: not_started
      claim: null
      disposition: accepted
      resurface:
        kind: always
  - id: null
    key: storage
    base: null
    title: storage layer
    description: null
    observed:
      progress: not_started
      claim: null
      disposition: accepted
      resurface:
        kind: always
groups:
  - id: null
    key: import
    base: null
    title: import plan
    description: null
    observed:
      progress: not_started
      claim: null
      disposition: accepted
      resurface:
        kind: always
  - id: null
    key: release
    base: null
    title: release plan
    description: null
    observed:
      progress: not_started
      claim: null
      disposition: accepted
      resurface:
        kind: always
relations:
  editable:
    parents:
      - child: {{ key: api }}
        parent: {{ key: import }}
    dependencies:{dependencies}
  readonly:
    parents: []
    dependencies: []
references:
  entities: []
"#
    )
}

fn entity_id(list: &str, title: &str) -> String {
    list.lines()
        .find(|line| line.ends_with(title))
        .and_then(|line| line.split_whitespace().next())
        .unwrap()
        .to_string()
}

#[test]
fn prepare_check_and_apply_create_mixed_entities_and_dependencies() {
    let repo = TestRepo::new();
    repo.init("test");
    let path = plan_path(&repo, "plan.yml");
    write(
        &path,
        &new_plan(
            r#"
      - dependent: { key: api }
        prerequisite: { key: storage }
      - dependent: { key: api }
        prerequisite: { key: release }
      - dependent: { key: import }
        prerequisite: { key: storage }
      - dependent: { key: import }
        prerequisite: { key: release }"#,
        ),
    );

    let prepare = repo.axon(&["import", "prepare", path.to_str().unwrap()]);
    assert_success(&prepare);
    let canonical = fs::read_to_string(&path).unwrap();
    assert_eq!(canonical.matches("id: test-").count(), 4);
    assert!(canonical.contains("description: |-"));
    assert!(canonical.contains("dependent: { key: api }"));

    let check = repo.axon(&["import", "check", path.to_str().unwrap()]);
    assert_success(&check);
    assert!(stdout(&check).contains("create Issue"));
    assert!(stdout(&check).contains("parent null -> test-"));
    assert!(stdout(&check).contains("dependency added test-"));
    assert!(stdout(&check).contains("Derived changes"));

    let apply = repo.axon(&["import", "apply", path.to_str().unwrap()]);
    assert_success(&apply);
    let list = stdout(&repo.axon(&["list"]));
    let api = entity_id(&list, "import API");
    let storage = entity_id(&list, "storage layer");
    let import = entity_id(&list, "import plan");
    let release = entity_id(&list, "release plan");
    let api_show = stdout(&repo.axon(&["show", &api]));
    assert_show_relation(&api_show, "Dependency:", &storage);
    assert_show_relation(&api_show, "Dependency:", &release);
    let import_show = stdout(&repo.axon(&["show", &import]));
    assert_show_relation(&import_show, "Dependency:", &storage);
    assert_show_relation(&import_show, "Dependency:", &release);
    assert!(import_show.contains("  Direct children: 1"));
    let (revision, title, parent, dependency_count) = repo.current_revision(&api);
    assert_eq!(revision, 1);
    assert_eq!(title, "import API");
    assert_eq!(parent, Some(import.clone()));
    assert_eq!(dependency_count, 2);
    assert!(
        fs::read_to_string(&path)
            .unwrap()
            .matches("base: \"blake3:")
            .count()
            >= 4
    );
}

#[test]
fn declaration_apply_rejects_a_fixed_owner_without_partial_changes() {
    let repo = TestRepo::new();
    repo.init("test");
    let fixed = repo.plan("fixed title");
    let fixed_rejected = repo.group_plan("rejected fixed group");
    assert_success(&repo.axon(&[
        "decide",
        "reject",
        &fixed_rejected,
        "-r",
        "declined as declared",
    ]));
    let draft = repo.capture("draft title");
    let path = plan_path(&repo, "fixed.yml");
    let exported = stdout(&repo.axon(&["export", &fixed, &fixed_rejected, &draft]));
    let edited = exported
        .replacen("title: fixed title", "title: forbidden title", 1)
        .replacen(
            "title: rejected fixed group",
            "title: forbidden group title",
            1,
        )
        .replacen("title: draft title", "title: allowed title", 1);
    write(&path, &edited);

    let check = repo.axon(&["import", "check", path.to_str().unwrap()]);
    assert_failure(&check);
    assert!(stderr(&check).contains("plan declaration"));
    let apply = repo.axon(&["import", "apply", path.to_str().unwrap()]);
    assert_failure(&apply);
    assert_eq!(repo.snapshot(&fixed).title, "fixed title");
    assert_eq!(repo.snapshot(&fixed_rejected).title, "rejected fixed group");
    assert_eq!(repo.snapshot(&draft).title, "draft title");
}

#[test]
fn export_selectors_do_not_expand_the_edit_set_through_relations() {
    let repo = TestRepo::new();
    repo.init("test");
    let root = repo.group_plan("root plan");
    let child = repo.plan("direct task");
    let nested = repo.group_plan("nested plan");
    let grandchild = repo.plan("deep task");
    let prerequisite = repo.plan("outside prerequisite");
    let dependent = repo.plan("outside dependent");
    repo.set_parent(&child, &root);
    repo.set_parent(&nested, &root);
    repo.set_parent(&grandchild, &nested);
    repo.add_dependency(&child, &prerequisite);
    repo.add_dependency(&dependent, &child);

    let direct = repo.axon(&["export", "--group", &root]);
    assert_success(&direct);
    let direct = stdout(&direct);
    let editable = direct.split("relations:").next().unwrap();
    assert!(editable.contains("root plan"));
    assert!(editable.contains("direct task"));
    assert!(editable.contains("nested plan"));
    assert!(!editable.contains("deep task"));
    assert!(direct.contains("readonly:"));
    assert!(direct.contains("deep task"));
    assert!(direct.contains("outside prerequisite"));
    assert!(direct.contains("outside dependent"));

    let recursive = repo.axon(&["export", "--group", &root, "--recursive"]);
    assert_success(&recursive);
    let recursive = stdout(&recursive);
    assert!(
        recursive
            .split("relations:")
            .next()
            .unwrap()
            .contains("deep task")
    );

    let union = repo.axon(&["export", &child, "--group", &root]);
    assert_success(&union);
    assert_eq!(stdout(&union).matches("title: direct task").count(), 1);
}

#[test]
fn strict_yaml_and_read_only_or_stale_snapshots_are_rejected() {
    let repo = TestRepo::new();
    repo.init("test");
    let issue = repo.plan("original title");
    let path = plan_path(&repo, "strict.yml");
    let exported = stdout(&repo.axon(&["export", &issue]));

    let duplicate = exported.replacen(
        "schema: axon-plan/v2",
        "schema: axon-plan/v2\nschema: axon-plan/v2",
        1,
    );
    write(&path, &duplicate);
    let check = repo.axon(&["import", "check", path.to_str().unwrap()]);
    assert_failure(&check);
    assert!(stderr(&check).contains("duplicate"));

    let tagged = exported.replacen("title: original title", "title: !custom original title", 1);
    write(&path, &tagged);
    let check = repo.axon(&["import", "check", path.to_str().unwrap()]);
    assert_failure(&check);
    assert!(stderr(&check).contains("tag"));

    let anchored = exported.replacen("title: original title", "title: &shared original title", 1);
    write(&path, &anchored);
    let check = repo.axon(&["import", "check", path.to_str().unwrap()]);
    assert_failure(&check);
    assert!(stderr(&check).contains("anchor"));

    let unknown = format!("{exported}unexpected: true\n");
    write(&path, &unknown);
    let check = repo.axon(&["import", "check", path.to_str().unwrap()]);
    assert_failure(&check);
    assert!(stderr(&check).contains("unknown field"));

    let observed = exported.replacen("disposition: accepted", "disposition: rejected", 1);
    write(&path, &observed);
    let check = repo.axon(&["import", "check", path.to_str().unwrap()]);
    assert_failure(&check);
    assert!(stderr(&check).contains("read-only observed"));

    write(&path, &exported);
    assert_success(&repo.axon(&["decide", "undecide", &issue, "-r", "edit declaration"]));
    assert_success(&repo.axon(&["write", &issue, "--title", "changed elsewhere"]));
    let check = repo.axon(&["import", "check", path.to_str().unwrap()]);
    assert_failure(&check);
    assert!(stderr(&check).contains("stale base"));

    let current = stdout(&repo.axon(&["export", &issue]));
    write(&path, &current);
    assert_success(&repo.axon(&["decide", "accept", &issue, "-r", "declaration fixed"]));
    assert_success(&repo.axon(&["start", &issue]));
    let check = repo.axon(&["import", "check", path.to_str().unwrap()]);
    assert_failure(&check);
    assert!(stderr(&check).contains("stale base"));
}

#[test]
fn prepare_rejects_ids_that_it_could_not_have_assigned() {
    let repo = TestRepo::new();
    repo.init("test");
    let path = plan_path(&repo, "identity.yml");

    let arbitrary = new_plan(" []").replacen("id: null", "id: arbitrary", 1);
    write(&path, &arbitrary);
    let prepare = repo.axon(&["import", "prepare", path.to_str().unwrap()]);
    assert_failure(&prepare);
    assert!(stderr(&prepare).contains("assigned by `axon import prepare`"));

    let missing_key = new_plan(" []")
        .replacen(
            "id: null\n    key: api",
            "id: test-000000\n    key: null",
            1,
        )
        .replace("{ key: api }", "{ id: test-000000 }");
    write(&path, &missing_key);
    let prepare = repo.axon(&["import", "prepare", path.to_str().unwrap()]);
    assert_failure(&prepare);
    assert!(stderr(&prepare).contains("retain a key"));
}

#[test]
fn reference_like_lines_in_multiline_descriptions_are_preserved() {
    let repo = TestRepo::new();
    repo.init("test");
    let issue = repo.capture("literal content");
    let description = plan_path(&repo, "description.md");
    write(
        &description,
        "first line\nentity: {id: keep-exact}\n- child: {key: also-exact}\nlast line",
    );
    assert_success(&repo.axon(&["write", &issue, "--file", description.to_str().unwrap()]));

    let path = plan_path(&repo, "literal.yml");
    write(&path, &stdout(&repo.axon(&["export", &issue])));
    let exported = fs::read_to_string(&path).unwrap();
    assert!(exported.contains("      entity: {id: keep-exact}"));
    assert!(exported.contains("      - child: {key: also-exact}"));
    assert_success(&repo.axon(&["import", "check", path.to_str().unwrap()]));
    assert_success(&repo.axon(&["import", "prepare", path.to_str().unwrap()]));
    assert_eq!(fs::read_to_string(&path).unwrap(), exported);
}

#[test]
fn export_rejects_stored_titles_outside_the_declaration_contract() {
    let repo = TestRepo::new();
    repo.init("test");
    let issue = repo.plan("valid title");
    repo.execute_batch(&format!(
        "UPDATE entities SET title = 'line one' || char(10) || 'line two' WHERE id = '{issue}';"
    ));

    let export = repo.axon(&["export", &issue]);
    assert_failure(&export);
    assert!(stderr(&export).contains("non-empty single-line"));
}

#[test]
fn sqlite_failure_rolls_back_every_imported_entity() {
    let repo = TestRepo::new();
    repo.init("test");
    let path = plan_path(&repo, "rollback.yml");
    write(&path, &new_plan(" []"));
    assert_success(&repo.axon(&["import", "prepare", path.to_str().unwrap()]));
    repo.execute_batch(
        "CREATE TRIGGER reject_import BEFORE INSERT ON entities
         WHEN NEW.title = 'release plan'
         BEGIN SELECT RAISE(ABORT, 'injected import failure'); END;",
    );

    let apply = repo.axon(&["import", "apply", path.to_str().unwrap()]);
    assert_failure(&apply);
    assert!(stderr(&apply).contains("injected import failure"));
    let list = repo.axon(&["list"]);
    assert_success(&list);
    assert!(stdout(&list).is_empty());
}

#[cfg(unix)]
#[test]
fn apply_retry_repairs_the_file_after_a_post_commit_rewrite_failure() {
    use std::os::unix::fs::PermissionsExt;

    let repo = TestRepo::new();
    repo.init("test");
    let path = plan_path(&repo, "recovery.yml");
    write(&path, &new_plan(" []"));
    assert_success(&repo.axon(&["import", "prepare", path.to_str().unwrap()]));

    let original_mode = fs::metadata(repo.root()).unwrap().permissions().mode();
    fs::set_permissions(repo.root(), fs::Permissions::from_mode(0o555)).unwrap();
    let first = repo.axon(&["import", "apply", path.to_str().unwrap()]);
    fs::set_permissions(repo.root(), fs::Permissions::from_mode(original_mode)).unwrap();
    assert_failure(&first);
    assert!(stderr(&first).contains("I/O"));
    assert!(stdout(&repo.axon(&["list"])).contains("import API"));

    let retry = repo.axon(&["import", "apply", path.to_str().unwrap()]);
    assert_success(&retry);
    assert!(
        fs::read_to_string(&path)
            .unwrap()
            .contains("base: \"blake3:")
    );
}

#[test]
fn observed_shapes_and_file_local_aliases_round_trip() {
    let repo = TestRepo::new();
    repo.init("test");
    let trigger = repo.plan("trigger");
    let waiting = repo.group_plan("waiting");
    let dated = repo.plan("dated");
    let claimed = repo.plan("claimed");
    assert_success(&repo.axon(&["when", "after", &waiting, &trigger]));
    assert_success(&repo.axon(&["when", "at", &dated, "2099-01-02"]));
    assert_success(&repo.axon(&["start", &claimed]));
    let path = plan_path(&repo, "observed.yml");
    let mut exported = stdout(&repo.axon(&["export", &trigger, &waiting, &dated, &claimed]));
    exported = exported.replacen(
        &format!("id: {trigger}\n    key: null"),
        &format!("id: {trigger}\n    key: trigger"),
        1,
    );
    exported = exported.replace(
        &format!("entity: {{ id: {trigger} }}"),
        "entity: { key: trigger }",
    );
    write(&path, &exported);

    assert_success(&repo.axon(&["import", "prepare", path.to_str().unwrap()]));
    let prepared = fs::read_to_string(&path).unwrap();
    assert!(prepared.contains("kind: at_date"));
    assert!(prepared.contains("progress: in_progress"));
    assert!(prepared.contains("entity: { key: trigger }"));
    assert_success(&repo.axon(&["import", "check", path.to_str().unwrap()]));
}

#[test]
fn a_hundred_entity_plan_remains_a_single_atomic_edit() {
    let repo = TestRepo::new();
    repo.init("scale");
    let path = plan_path(&repo, "large.yml");
    let mut yaml = String::from("schema: axon-plan/v2\nissues:\n");
    for index in 0..100 {
        yaml.push_str(&format!(
            "  - id: null\n    key: task-{index}\n    base: null\n    title: task {index}\n    description: long description {index}\n    observed:\n      progress: not_started\n      claim: null\n      disposition: accepted\n      resurface:\n        kind: always\n"
        ));
    }
    yaml.push_str("groups: []\nrelations:\n  editable:\n    parents: []\n    dependencies:\n");
    for index in 1..100 {
        yaml.push_str(&format!(
            "      - dependent: {{ key: task-{index} }}\n        prerequisite: {{ key: task-{} }}\n",
            index - 1
        ));
    }
    yaml.push_str(
        "  readonly:\n    parents: []\n    dependencies: []\nreferences:\n  entities: []\n",
    );
    write(&path, &yaml);

    assert_success(&repo.axon(&["import", "prepare", path.to_str().unwrap()]));
    assert_success(&repo.axon(&["import", "check", path.to_str().unwrap()]));
    assert_success(&repo.axon(&["import", "apply", path.to_str().unwrap()]));
    assert_eq!(stdout(&repo.axon(&["list"])).lines().count(), 100);
}
