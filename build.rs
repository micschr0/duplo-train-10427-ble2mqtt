//! Build script: embed a git build identifier so dev builds self-identify.
//!
//! Sets `GIT_DESCRIBE` (read in `main` via `env!`) to `git describe
//! --tags --always --dirty` — e.g. `v0.2.1-3-gabc123` (3 commits past the
//! `v0.2.1` tag) or `v0.2.1-3-gabc123-dirty` for an uncommitted tree.
//! Falls back to the crate version when git is unavailable (release
//! tarball, crates.io, or a cross container without `git`).

use std::process::Command;

fn main() {
    // Re-run when the git state changes so the embedded id stays current.
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs");
    // Allow the host to inject the value (e.g. cross builds where the
    // container lacks git): `GIT_DESCRIBE=... cargo build`.
    println!("cargo:rerun-if-env-changed=GIT_DESCRIBE");

    let describe = std::env::var("GIT_DESCRIBE")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(git_describe)
        .unwrap_or_else(fallback_version);

    println!("cargo:rustc-env=GIT_DESCRIBE={}", describe.trim());
}

/// `git describe --tags --always --dirty`, or `None` if git is unavailable.
fn git_describe() -> Option<String> {
    let output = Command::new("git")
        .args(["describe", "--tags", "--always", "--dirty"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let described = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (!described.is_empty()).then_some(described)
}

/// `v<crate version>` when git can't be queried.
fn fallback_version() -> String {
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".to_string());
    format!("v{version}")
}
