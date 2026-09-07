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
        .find(|line| line.contains(&format!("  {title}")))
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
    assert!(stdout(&apply).contains("create Issue"));
    assert!(stdout(&apply).contains(&format!("Applied  {}", path.display())));
    let refreshed = fs::read(&path).unwrap();
    assert_ne!(refreshed, canonical.as_bytes());
    let saved_path = repo.root().join(".axon/axon.db");
    let saved = fs::read(&saved_path).unwrap();
    let repeated = repo.axon(&["import", "apply", path.to_str().unwrap()]);
    assert_success(&repeated);
    assert!(stdout(&repeated).contains("Changes:\n  none\n"));
    assert!(stdout(&repeated).contains(&format!("Applied  {}", path.display())));
    assert_eq!(fs::read(&saved_path).unwrap(), saved);
    assert_eq!(fs::read(&path).unwrap(), refreshed);

    let list = stdout(&repo.axon(&["list"]));
    let api = entity_id(&list, "import API");
    let storage = entity_id(&list, "storage layer");
    let import = entity_id(&list, "import plan");
    let release = entity_id(&list, "release plan");
    let api_show = stdout(&repo.axon(&["show", &api]));
    assert_show_relation(&api_show, "Dependency:", &storage);
    assert_show_relation(&api_show, "Dependency:", &release);
    let import_show = stdout(&repo.axon(&["show", &import]));
    assert_show_relation(&import_show, "Needs:", &storage);
    assert_show_relation(&import_show, "Needs:", &release);
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
        "UPDATE entities SET title = 'line one' || char(10) || 'line two' WHERE id = '{issue}';
         UPDATE declaration_revisions SET title = 'line one' || char(10) || 'line two' WHERE entity_id = '{issue}';
         UPDATE history_baselines SET payload = json_set(payload, '$.bundle.entity.title', 'line one' || char(10) || 'line two')
           WHERE json_extract(payload, '$.bundle.entity.id') = '{issue}';"
    ));

    let export = repo.axon(&["export", &issue]);
    assert_failure(&export);
    assert!(stderr(&export).contains("non-empty single-line"));
}

#[test]
fn unknown_trigger_prevents_import_before_any_entity_is_written() {
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
    assert!(stderr(&apply).contains("unknown structure"));
    repo.execute_batch("DROP TRIGGER reject_import");
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

    let prepared = fs::read(&path).unwrap();
    let original_mode = fs::metadata(repo.root()).unwrap().permissions().mode();
    fs::set_permissions(repo.root(), fs::Permissions::from_mode(0o555)).unwrap();
    let first = repo.axon(&["import", "apply", path.to_str().unwrap()]);
    fs::set_permissions(repo.root(), fs::Permissions::from_mode(original_mode)).unwrap();
    assert_failure(&first);
    assert!(stderr(&first).contains("I/O"));
    assert!(stderr(&first).contains(&format!(
        "{} import apply: declaration file refresh failed:",
        path.display()
    )));
    assert!(stderr(&first).contains("Applied: storage declaration values"));
    assert!(stderr(&first).contains("Not applied: declaration file refresh"));
    assert_eq!(fs::read(&path).unwrap(), prepared);
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

#[test]
fn builtin_example_applies_without_editing_and_preserves_its_plan_structure() {
    let repo = TestRepo::new();
    let example = repo.axon(&["docs", "declaration", "--example"]);
    assert_success(&example);
    assert!(example.stderr.is_empty());
    assert!(!repo.root().join(".axon").exists());
    let yaml = stdout(&example);
    assert!(yaml.starts_with("schema: axon-plan/v2\n"));
    assert!(!yaml.contains("\x1b"));
    assert!(!yaml.contains("```"));
    assert_eq!(yaml.matches("id: null").count(), 3);
    assert_eq!(yaml.matches("base: null").count(), 3);

    repo.init("test");
    let db_path = repo.root().join(".axon/axon.db");
    let before = fs::read(&db_path).unwrap();
    for args in [
        &["docs", "declaration"][..],
        &["docs", "declaration", "--example"][..],
    ] {
        let output = repo.axon(args);
        assert_success(&output);
        assert!(output.stderr.is_empty());
        if args.last() == Some(&"--example") {
            assert_eq!(output.stdout, example.stdout);
        }
    }
    assert_eq!(fs::read(&db_path).unwrap(), before);

    let path = plan_path(&repo, "example.yml");
    fs::write(&path, &example.stdout).unwrap();
    let file = path.to_str().unwrap();
    assert_success(&repo.axon(&["import", "prepare", file]));
    assert_eq!(repo.entity_count(), 0);
    let check = repo.axon(&["import", "check", file]);
    assert_success(&check);
    assert_eq!(stdout(&check).matches("create Issue").count(), 2);
    assert_eq!(stdout(&check).matches("create Group").count(), 1);
    assert_eq!(repo.entity_count(), 0);
    assert_success(&repo.axon(&["import", "apply", file]));
    let check = repo.axon(&["import", "check", file]);
    assert_success(&check);
    assert_eq!(
        stdout(&check),
        "Plan is valid.\nChanges:\n  none\nDerived changes (ready, blocked, orphaned, active_scope, group_completable):\n  none\n"
    );
    assert_eq!(repo.entity_count(), 3);
    assert_eq!(repo.dep_count(), 1);

    let list = repo.axon(&["list"]);
    assert_success(&list);
    let list = stdout(&list);
    let group = entity_id(&list, "Deliver a feature");
    let implement = entity_id(&list, "Implement the feature");
    let verify = entity_id(&list, "Verify the feature");
    for (id, kind, parent) in [
        (&group, "group", None),
        (&implement, "issue", Some(group.clone())),
        (&verify, "issue", Some(group.clone())),
    ] {
        let snapshot = repo.snapshot(id);
        assert_eq!(snapshot.kind, kind);
        assert_eq!(snapshot.parent, parent);
        assert_eq!(snapshot.progress, "not_started");
        assert_eq!(snapshot.disposition, "accepted");
        assert_eq!(snapshot.resurface_kind, None);
        assert_eq!(snapshot.progress_events, 0);
    }
    let show = repo.axon(&["show", &verify]);
    assert_success(&show);
    assert_show_relation(&stdout(&show), "Dependency:", &implement);
    let claims = repo.axon(&["claims"]);
    assert_success(&claims);
    assert!(claims.stdout.is_empty());
}

fn export_value(repo: &TestRepo, id: &str) -> serde_json::Value {
    let output = repo.axon(&["export", id]);
    assert_success(&output);
    serde_saphyr::from_str(&stdout(&output)).unwrap()
}

fn external_snapshot(source: &serde_json::Value, list: &str) -> serde_json::Value {
    let mut record = source[list][0].clone();
    let fields = record.as_object_mut().unwrap();
    fields.remove("key");
    fields.remove("description");
    fields.insert(
        "kind".into(),
        if list == "issues" { "issue" } else { "group" }.into(),
    );
    record
}

#[test]
fn external_snapshots_support_new_and_existing_owners_without_editing_targets() {
    use serde_json::json;
    for existing in [false, true] {
        let repo = TestRepo::new();
        repo.init("test");
        let prerequisite = repo.plan("external prerequisite");
        let parent = repo.group_plan("external parent");
        let waiter_target = repo.plan("schedule target");
        assert_success(&repo.axon(&["when", "after", &prerequisite, &waiter_target]));
        let before = [&prerequisite, &parent, &waiter_target].map(|id| repo.snapshot(id));
        let source = export_value(&repo, &prerequisite);
        let parent_source = export_value(&repo, &parent);
        let owner = existing.then(|| repo.capture("existing owner"));
        let mut plan = if let Some(id) = &owner {
            export_value(&repo, id)
        } else {
            serde_saphyr::from_str::<serde_json::Value>(include_str!(
                "../src/docs/declaration-example.yaml"
            ))
            .unwrap()
        };
        let owner_ref = owner
            .as_ref()
            .map_or(json!({"key": "implement"}), |id| json!({"id": id}));
        let parent_ref = if existing {
            owner_ref.clone()
        } else {
            json!({"key": "feature"})
        };
        plan["relations"]["editable"]["parents"]
            .as_array_mut()
            .unwrap()
            .push(json!({"child": parent_ref, "parent": {"id": parent}}));
        plan["relations"]["editable"]["dependencies"]
            .as_array_mut()
            .unwrap()
            .push(json!({"dependent": owner_ref, "prerequisite": {"id": prerequisite}}));
        let refs = plan["references"]["entities"].as_array_mut().unwrap();
        refs.push(external_snapshot(&source, "issues"));
        refs.push(external_snapshot(&parent_source, "groups"));
        refs.extend(
            source["references"]["entities"]
                .as_array()
                .unwrap()
                .iter()
                .cloned(),
        );
        let path = plan_path(&repo, "external.yml");
        write(&path, &serde_saphyr::to_string(&plan).unwrap());
        for operation in ["prepare", "check", "apply", "check"] {
            let output = repo.axon(&["import", operation, path.to_str().unwrap()]);
            assert_success(&output);
        }
        let check = repo.axon(&["import", "check", path.to_str().unwrap()]);
        assert!(stdout(&check).contains("Changes:\n  none\n"));
        let applied: serde_json::Value =
            serde_saphyr::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let owner_id = owner.unwrap_or_else(|| {
            applied["issues"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["key"] == "implement")
                .unwrap()["id"]
                .as_str()
                .unwrap()
                .to_string()
        });
        let parent_owner = if existing {
            owner_id.clone()
        } else {
            applied["groups"][0]["id"].as_str().unwrap().to_string()
        };
        assert_eq!(
            repo.snapshot(&parent_owner).parent.as_deref(),
            Some(parent.as_str())
        );
        assert_show_relation(
            &stdout(&repo.axon(&["show", &owner_id])),
            "Dependency:",
            &prerequisite,
        );
        assert_eq!(
            before,
            [&prerequisite, &parent, &waiter_target].map(|id| repo.snapshot(id))
        );
        let edit_ids = applied["issues"]
            .as_array()
            .unwrap()
            .iter()
            .chain(applied["groups"].as_array().unwrap())
            .map(|r| r["id"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert!(!edit_ids.contains(&prerequisite.as_str()));
        assert!(!edit_ids.contains(&parent.as_str()));
        let after = export_value(&repo, &prerequisite);
        assert_eq!(source["issues"], after["issues"]);
        assert_eq!(
            source["relations"]["editable"],
            after["relations"]["editable"]
        );
        let after_parent = export_value(&repo, &parent);
        assert_eq!(parent_source["groups"], after_parent["groups"]);
        assert_eq!(
            parent_source["relations"]["editable"],
            after_parent["relations"]["editable"]
        );
        if existing {
            let mut removal = applied.clone();
            removal["relations"]["editable"] = json!({"parents": [], "dependencies": []});
            removal["references"]["entities"] = json!([]);
            write(&path, &serde_saphyr::to_string(&removal).unwrap());
            for operation in ["prepare", "check", "apply", "check"] {
                assert_success(&repo.axon(&["import", operation, path.to_str().unwrap()]));
            }
            assert_eq!(repo.snapshot(&owner_id).parent, None);
            assert_eq!(
                before,
                [&prerequisite, &parent, &waiter_target].map(|id| repo.snapshot(id))
            );
        }
    }
}

#[test]
fn external_reference_diagnostics_preserve_database_and_failed_prepare_file() {
    let repo = TestRepo::new();
    repo.init("test");
    let owner = repo.capture("owner");
    let prerequisite = repo.plan("external prerequisite");
    assert_success(&repo.axon(&["dep", "add", &owner, "--needs", &prerequisite]));
    let output = repo.axon(&["export", &owner]);
    assert_success(&output);
    let original = stdout(&output);
    let reference_start = original.find("references:\n").unwrap();
    let dependency = format!(
        "    dependencies:\n      - dependent: {{ id: {owner} }}\n        prerequisite: {{ id: {prerequisite} }}"
    );
    let base = export_value(&repo, &prerequisite)["issues"][0]["base"]
        .as_str()
        .unwrap()
        .to_string();
    let cases = [
        (
            format!(
                "{}references:\n  entities: []\n",
                &original[..reference_start]
            ),
            "missing from this declaration file",
            "axon export",
        ),
        (
            original.replace(&prerequisite, "test-zzzzzz"),
            "does not exist in the current DB",
            "active root",
        ),
        (
            original.replace(
                "title: external prerequisite",
                "title: altered prerequisite",
            ),
            "despite a matching base",
            "Restore kind, title and observed",
        ),
        (
            original.replace(&base, &format!("blake3:{}", "0".repeat(64))),
            "stale or incorrect reference base",
            "fresh",
        ),
        (
            original.replace(&dependency, "    dependencies: []"),
            "unrelated snapshots",
            "Keep only snapshots required",
        ),
    ];
    let saved_path = repo.root().join(".axon/axon.db");
    let saved = fs::read(&saved_path).unwrap();
    let path = plan_path(&repo, "invalid-external.yml");
    for (input, diagnostic, guidance) in cases {
        assert_ne!(input, original);
        write(&path, &input);
        for operation in ["prepare", "check", "apply"] {
            let output = repo.axon(&["import", operation, path.to_str().unwrap()]);
            assert_eq!(output.status.code(), Some(1));
            let error = stderr(&output);
            assert!(error.contains(diagnostic), "{operation}: {error}");
            assert!(
                error.contains("Help:") && error.contains(guidance),
                "{error}"
            );
            assert_eq!(fs::read(&path).unwrap(), input.as_bytes());
            assert_eq!(fs::read(&saved_path).unwrap(), saved);
        }
    }
}
