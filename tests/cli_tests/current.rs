//! `kee current`.

use crate::common::kee;
use assert_cmd::Command;
use predicates::str::contains;
use tempfile::TempDir;

#[test]
fn current_with_no_session_says_nothing_active() {
    let tmp = TempDir::new().unwrap();
    kee(tmp.path())
        .arg("current")
        .assert()
        .success()
        .stdout(contains("No profile is currently active"));
}

#[test]
fn current_inside_session_reports_profile_from_env() {
    let tmp = TempDir::new().unwrap();
    let mut cmd = Command::cargo_bin("kee").unwrap();
    cmd.env("HOME", tmp.path())
        .env("USERPROFILE", tmp.path())
        .env("KEE_ACTIVE_PROFILE", "1")
        .env("KEE_CURRENT_PROFILE", "acme.prod")
        .arg("current")
        .assert()
        .success()
        .stdout(contains("acme.prod"));
}
