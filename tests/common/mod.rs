//! Shared helpers for the integration test suite.
//!
//! Each test runs the compiled `kee` binary as a subprocess (via assert_cmd)
//! with a fresh `HOME` pointing at a tempdir. No global state leaks between
//! tests, so they can run in parallel.
//!
//! Tests that need to mock the `aws` CLI use `aws_stub_dir()` to create a
//! tempdir containing an executable `aws` shell script, then prepend that
//! dir to the child's `PATH` via `Command::env`. This is child-env-aware
//! and parallel-safe (each test owns its own stub).

use assert_cmd::Command;
use kee::{KeeConfig, ProfileInfo};
use std::fs;
use std::path::Path;
#[cfg(unix)]
use tempfile::TempDir;

/// Build a populated KeeConfig with two profiles: `acme.dev` (non-prod)
/// and `acme.prod` (prod).
pub fn fixture_config() -> KeeConfig {
    let mut cfg = KeeConfig::default();
    cfg.profiles.insert(
        "acme.dev".to_string(),
        ProfileInfo {
            profile_name: "acme.dev".to_string(),
            sso_start_url: "https://acme.awsapps.com/start".to_string(),
            sso_region: "ap-southeast-2".to_string(),
            sso_account_id: "123456789012".to_string(),
            sso_role_name: "Developer".to_string(),
            session_name: "acme".to_string(),
            production: false,
        },
    );
    cfg.profiles.insert(
        "acme.prod".to_string(),
        ProfileInfo {
            profile_name: "acme.prod".to_string(),
            sso_start_url: "https://acme.awsapps.com/start".to_string(),
            sso_region: "ap-southeast-2".to_string(),
            sso_account_id: "999999999999".to_string(),
            sso_role_name: "Admin".to_string(),
            session_name: "acme".to_string(),
            production: true,
        },
    );
    cfg
}

/// Write a KeeConfig to `<home>/.kee/config.json`.
pub fn seed_config(home: &Path, cfg: &KeeConfig) {
    let dir = home.join(".kee");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("config.json"),
        serde_json::to_string_pretty(cfg).unwrap(),
    )
    .unwrap();
}

/// Read the persisted KeeConfig from `<home>/.kee/config.json`.
pub fn read_config(home: &Path) -> KeeConfig {
    let body = fs::read_to_string(home.join(".kee/config.json")).unwrap();
    serde_json::from_str(&body).unwrap()
}

/// Build a `kee` Command with HOME pointed at the tempdir, and KEE_*
/// session env vars cleared so the child sees a clean state regardless of
/// the test runner's environment.
///
/// On Windows, `dirs::home_dir()` reads `USERPROFILE` rather than `HOME`,
/// so we set both to keep the helper portable.
pub fn kee(home: &Path) -> Command {
    let mut cmd = Command::cargo_bin("kee").unwrap();
    cmd.env("HOME", home)
        .env("USERPROFILE", home)
        .env_remove("KEE_ACTIVE_PROFILE")
        .env_remove("KEE_CURRENT_PROFILE");
    cmd
}

/// Create a tempdir containing an executable `aws` script that dispatches
/// based on its first argument. The stub body is a single shell script:
///
/// ```sh
/// #!/bin/sh
/// case "$1" in ... esac
/// ```
///
/// Returns the tempdir; the caller wires it into PATH on the child.
#[cfg(unix)]
pub fn aws_stub_dir(stub_body: &str) -> TempDir {
    use std::os::unix::fs::PermissionsExt;
    let tmp = TempDir::new().unwrap();
    let stub = tmp.path().join("aws");
    fs::write(&stub, stub_body).unwrap();
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
    tmp
}

/// Build a `kee` Command with HOME, cleared KEE_* env, and a tempdir
/// prepended to PATH that contains the given AWS stub.
#[cfg(unix)]
pub fn kee_with_stub(home: &Path, stub: &TempDir) -> Command {
    let mut cmd = kee(home);
    let parent_path = std::env::var("PATH").unwrap_or_default();
    cmd.env("PATH", format!("{}:{}", stub.path().display(), parent_path));
    cmd
}
