mod common;

use common::{TestDir, assert_success};
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn run(command: &mut Command) -> String {
    let output = command.output().unwrap();
    assert_success(&output);
    String::from_utf8(output.stdout).unwrap().trim().into()
}

fn git(root: &Path, args: &[&str]) -> String {
    let mut command = Command::new("git");
    // Hook-provided index/object paths must never escape into fixture repositories.
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("GIT_") {
            command.env_remove(name);
        }
    }
    run(command
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args))
}

fn fixture(root: &Path) {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='provenance-probe'\nversion='1.2.3'\nedition='2024'\n",
    )
    .unwrap();
    fs::write(root.join("build.rs"), include_str!("../build.rs")).unwrap();
    fs::write(
        root.join("src/main.rs"),
        "fn main() { println!(\"{}\", env!(\"AXON_VERSION\")); }\n",
    )
    .unwrap();
    fs::write(root.join(".gitignore"), "/target\n").unwrap();
}

fn build(root: &Path, metadata: &[(&str, &str)]) -> Output {
    Command::new(env!("CARGO"))
        .current_dir(root)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("AXON_BUILD_COMMIT")
        .env_remove("AXON_BUILD_SOURCE_STATE")
        .envs(metadata.iter().copied())
        .args(["build", "--offline", "--quiet"])
        .output()
        .unwrap()
}

fn version(root: &Path, metadata: &[(&str, &str)]) -> String {
    assert_success(&build(root, metadata));
    run(&mut Command::new(
        root.join("target/debug/provenance-probe"),
    ))
}

#[test]
fn cargo_refreshes_git_metadata_including_linked_worktrees() {
    let temp = TestDir::new("provenance-git");
    let root = temp.path().join("source");
    fixture(&root);
    git(&root, &["init", "--quiet", "--initial-branch=main"]);
    git(&root, &["config", "core.excludesFile", "/dev/null"]);
    version(&root, &[]);
    git(&root, &["add", "."]);
    git(&root, &["commit", "--quiet", "-m", "fixture"]);
    let first = git(&root, &["rev-parse", "HEAD"]);
    assert_eq!(
        version(&root, &[]),
        format!("1.2.3 (commit {first}; source clean)")
    );

    git(
        &root,
        &["commit", "--quiet", "--allow-empty", "-m", "commit only"],
    );
    let second = git(&root, &["rev-parse", "HEAD"]);
    assert_ne!(first, second);
    assert_eq!(
        version(&root, &[]),
        format!("1.2.3 (commit {second}; source clean)")
    );
    fs::write(root.join("new-source"), "untracked").unwrap();
    assert!(version(&root, &[]).ends_with("source modified)"));
    fs::remove_file(root.join("new-source")).unwrap();
    assert!(version(&root, &[]).ends_with("source clean)"));
    fs::write(root.join(".gitignore"), "/target\n/ignored\n").unwrap();
    assert!(version(&root, &[]).ends_with("source modified)"));
    git(&root, &["add", ".gitignore"]);
    assert!(version(&root, &[]).ends_with("source modified)"));
    git(
        &root,
        &["commit", "--quiet", "-m", "ignore generated files"],
    );
    fs::write(root.join("ignored"), "generated").unwrap();
    fs::create_dir(root.join(".axon")).unwrap();
    fs::write(root.join(".axon/state.jsonl"), "management data").unwrap();
    git(&root, &["add", ".axon/state.jsonl"]);
    assert!(version(&root, &[]).ends_with("source clean)"));
    git(&root, &["commit", "--quiet", "-m", "management data"]);
    fs::write(root.join(".axon/state.jsonl"), "changed management data").unwrap();
    assert!(version(&root, &[]).ends_with("source clean)"));

    let linked = temp.path().join("linked");
    git(
        &root,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "linked",
            linked.to_str().unwrap(),
        ],
    );
    let linked_before = version(&linked, &[]);
    git(
        &linked,
        &["commit", "--quiet", "--allow-empty", "-m", "linked commit"],
    );
    let linked_commit = git(&linked, &["rev-parse", "HEAD"]);
    assert_eq!(
        version(&linked, &[]),
        format!("1.2.3 (commit {linked_commit}; source clean)")
    );
    assert_ne!(linked_before, version(&linked, &[]));
    fs::write(linked.join("new-source"), "untracked").unwrap();
    assert!(version(&linked, &[]).ends_with("source modified)"));
    fs::remove_file(linked.join("new-source")).unwrap();
    assert!(version(&linked, &[]).ends_with("source clean)"));
}

#[test]
fn explicit_metadata_and_archive_fallback_never_infer_parent_provenance() {
    let temp = TestDir::new("provenance-archive");
    let root = temp.path().join("archive");
    fixture(&root);
    assert_eq!(
        version(&root, &[]),
        "1.2.3 (commit unknown; source unknown)"
    );
    git(temp.path(), &["init", "--quiet"]);
    git(temp.path(), &["config", "core.excludesFile", "/dev/null"]);
    git(temp.path(), &["add", "gitconfig"]);
    git(
        temp.path(),
        &["commit", "--quiet", "-m", "unrelated parent"],
    );
    assert_eq!(
        version(&root, &[]),
        "1.2.3 (commit unknown; source unknown)"
    );

    let commit = "a".repeat(40);
    let explicit = [
        ("AXON_BUILD_COMMIT", commit.as_str()),
        ("AXON_BUILD_SOURCE_STATE", "modified"),
    ];
    assert_eq!(
        version(&root, &explicit),
        format!("1.2.3 (commit {commit}; source modified)")
    );
    assert_eq!(
        version(&root, &[("AXON_BUILD_COMMIT", &commit)]),
        format!("1.2.3 (commit {commit}; source unknown)")
    );
    let second = "b".repeat(64);
    assert_eq!(
        version(
            &root,
            &[
                ("AXON_BUILD_COMMIT", &second),
                ("AXON_BUILD_SOURCE_STATE", "clean")
            ]
        ),
        format!("1.2.3 (commit {second}; source clean)")
    );
    let output = build(
        &root,
        &[
            ("AXON_BUILD_COMMIT", "123\ninjected"),
            ("AXON_BUILD_SOURCE_STATE", ""),
        ],
    );
    assert_success(&output);
    assert_eq!(
        run(&mut Command::new(
            root.join("target/debug/provenance-probe")
        )),
        "1.2.3 (commit unknown; source unknown)"
    );
    assert_eq!(
        version(&root, &[]),
        "1.2.3 (commit unknown; source unknown)"
    );
}

#[test]
fn relocated_cli_version_needs_neither_git_nor_db() {
    let temp = TestDir::new("provenance-cli");
    let binary = temp.path().join("axon-copy");
    fs::copy(env!("CARGO_BIN_EXE_axon"), &binary).unwrap();
    fs::create_dir(temp.path().join(".axon")).unwrap();
    fs::write(temp.path().join(".axon/config.json"), "invalid config").unwrap();
    let expected = format!("axon {}", env!("AXON_VERSION"));
    for flag in ["--version", "-V"] {
        assert_eq!(
            run(Command::new(&binary)
                .current_dir(temp.path())
                .env("PATH", "")
                .env("AXON_BUILD_COMMIT", "runtime-must-not-override")
                .arg(flag)),
            expected
        );
    }
    assert_eq!(
        fs::read_to_string(temp.path().join(".axon/config.json")).unwrap(),
        "invalid config"
    );
}
