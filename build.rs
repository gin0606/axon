use std::env;
use std::path::Path;
use std::process::Command;

fn main() {
    // Git refs, the index and arbitrary untracked files can change without a source mtime change.
    // A deliberately absent input makes Cargo refresh provenance on every build.
    let refresh = Path::new(&env::var_os("OUT_DIR").unwrap()).join("provenance-refresh");
    println!("cargo:rerun-if-changed={}", refresh.display());
    println!("cargo:rerun-if-env-changed=AXON_BUILD_COMMIT");
    println!("cargo:rerun-if-env-changed=AXON_BUILD_SOURCE_STATE");

    let (commit, state) = if env::var_os("AXON_BUILD_COMMIT").is_some()
        || env::var_os("AXON_BUILD_SOURCE_STATE").is_some()
    {
        (
            metadata("AXON_BUILD_COMMIT", valid_commit),
            metadata("AXON_BUILD_SOURCE_STATE", |value| {
                matches!(value, "clean" | "modified" | "unknown")
            }),
        )
    } else {
        git_provenance(Path::new(&env::var_os("CARGO_MANIFEST_DIR").unwrap()))
    };
    println!(
        "cargo:rustc-env=AXON_VERSION={} (commit {commit}; source {state})",
        env::var("CARGO_PKG_VERSION").unwrap()
    );
}

fn valid_commit(value: &str) -> bool {
    value == "unknown"
        || (matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn metadata(name: &str, valid: impl FnOnce(&str) -> bool) -> String {
    match env::var(name) {
        Ok(value) if valid(&value) => value,
        Err(env::VarError::NotPresent) => "unknown".into(),
        _ => {
            println!("cargo:warning=Invalid {name}; using unknown");
            "unknown".into()
        }
    }
}

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let mut command = Command::new("git");
    // Hooks and packaging tools may export repository or pathspec overrides.
    for (name, _) in env::vars_os() {
        if name.to_string_lossy().starts_with("GIT_") {
            command.env_remove(name);
        }
    }
    let output = command
        .current_dir(root)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .args(args)
        .output()
        .ok()?;
    output.status.success().then_some(())?;
    String::from_utf8(output.stdout).ok()
}

fn git_provenance(root: &Path) -> (String, String) {
    let same_root = git(root, &["rev-parse", "--show-toplevel"])
        .and_then(|path| Path::new(path.trim_end()).canonicalize().ok())
        .zip(root.canonicalize().ok())
        .is_some_and(|(git_root, package_root)| git_root == package_root);
    if !same_root {
        return ("unknown".into(), "unknown".into());
    }
    let commit = git(root, &["rev-parse", "--verify", "HEAD^{commit}"])
        .map(|value| value.trim_end().to_owned())
        .filter(|value| valid_commit(value))
        .unwrap_or_else(|| "unknown".into());
    let state = git(
        root,
        &[
            "status",
            "--porcelain=v1",
            "--untracked-files=normal",
            "--ignore-submodules=none",
            "--",
            ".",
            ":(exclude,top).axon",
            ":(exclude,top)target",
        ],
    )
    .map(|status| {
        if status.is_empty() {
            "clean"
        } else {
            "modified"
        }
    })
    .unwrap_or("unknown");
    (commit, state.into())
}
