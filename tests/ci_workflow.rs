use crate::common::TestDir;
use serde_json::Value;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

const WORKFLOW: &str = include_str!("../.github/workflows/full-verification.yml");
const FULL_VERIFICATION: &str = include_str!("../scripts/full-verification");
const LEFTHOOK: &str = include_str!("../lefthook.yml");

fn workflow() -> Value {
    serde_saphyr::from_str(WORKFLOW).unwrap()
}

fn lefthook() -> Value {
    serde_saphyr::from_str(LEFTHOOK).unwrap()
}

fn run_with_fake_cargo(root: &Path, fail: Option<&str>) -> (Output, String) {
    let bin = root.join("bin");
    let log = root.join("cargo.log");
    fs::create_dir(&bin).unwrap();
    let cargo = bin.join("cargo");
    fs::write(
        &cargo,
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$CI_TEST_LOG\"\n[ \"$*\" != \"$CI_TEST_FAIL\" ]\n",
    )
    .unwrap();
    let mut permissions = fs::metadata(&cargo).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&cargo, permissions).unwrap();

    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut paths = vec![bin.clone()];
    paths.extend(std::env::split_paths(&path));
    let output = Command::new("sh")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/full-verification"))
        .env("PATH", std::env::join_paths(paths).unwrap())
        .env("CI_TEST_LOG", &log)
        .env("CI_TEST_FAIL", fail.unwrap_or_default())
        .output()
        .unwrap();
    let calls = fs::read_to_string(log).unwrap();
    (output, calls)
}

#[test]
fn workflow_runs_full_verification_for_pull_requests_and_main_pushes() {
    let workflow = workflow();
    assert_eq!(workflow["on"]["pull_request"], Value::Null);
    assert_eq!(
        workflow["on"]["push"]["branches"],
        serde_json::json!(["main"])
    );
    assert_eq!(workflow["permissions"]["contents"], "read");

    let job = &workflow["jobs"]["full-verification"];
    assert_eq!(job["runs-on"], "ubuntu-24.04");
    let steps = job["steps"].as_array().unwrap();
    assert_eq!(
        steps[0]["uses"],
        "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1"
    );
    assert_eq!(steps[1]["run"], "rustup toolchain install");
    assert_eq!(
        steps[2]["uses"],
        "Swatinem/rust-cache@63fed3e2fecf6f7b51dc6f043341b79ef82a9ae7"
    );
    assert_eq!(steps.last().unwrap()["run"], "./scripts/full-verification");
    assert!(
        steps
            .iter()
            .all(|step| step.get("continue-on-error").is_none())
    );
    assert!(FULL_VERIFICATION.contains("cargo fmt --check"));
    assert!(FULL_VERIFICATION.contains("cargo clippy --all-targets --all-features -- -D warnings"));
    assert!(FULL_VERIFICATION.contains("cargo test"));
}

#[test]
fn full_verification_propagates_each_cargo_failure() {
    let commands = [
        "fmt --check",
        "clippy --all-targets --all-features -- -D warnings",
        "test",
    ];

    let temp = TestDir::new("ci-success");
    let (output, calls) = run_with_fake_cargo(temp.path(), None);
    assert!(output.status.success());
    assert_eq!(calls.lines().collect::<Vec<_>>(), commands);

    for (failed_index, failed) in commands.iter().enumerate() {
        let temp = TestDir::new(&format!("ci-failure-{failed_index}"));
        let (output, calls) = run_with_fake_cargo(temp.path(), Some(failed));
        assert!(!output.status.success());
        assert_eq!(calls.lines().collect::<Vec<_>>(), commands[..=failed_index]);
    }
}

#[test]
fn pre_commit_keeps_rust_checks_and_uses_the_fast_test_targets() {
    let config = lefthook();
    let jobs = config["pre-commit"]["jobs"].as_array().unwrap();
    assert_eq!(jobs.len(), 3);
    assert_eq!(jobs[0]["name"], "fmt");
    assert_eq!(jobs[0]["glob"], "*.rs");
    assert_eq!(jobs[0]["run"], "mise exec -- cargo fmt --check");
    assert_eq!(jobs[1]["name"], "clippy");
    assert_eq!(jobs[1]["glob"], "*.rs");
    assert_eq!(
        jobs[1]["run"],
        "mise exec -- cargo clippy --all-targets --all-features -- -D warnings"
    );
    assert_eq!(jobs[2]["name"], "test");
    assert_eq!(jobs[2]["glob"], "*.rs");
    assert_eq!(
        jobs[2]["run"],
        "mise exec -- cargo test --bin axon --test smoke"
    );
}
