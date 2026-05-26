//! `kee rm`.

use crate::common::{fixture_config, kee, read_config, seed_config};
use predicates::str::contains;
use tempfile::TempDir;

#[test]
fn rm_unknown_profile_reports_not_found() {
    let tmp = TempDir::new().unwrap();
    seed_config(tmp.path(), &fixture_config());

    kee(tmp.path())
        .args(["rm", "nope"])
        .write_stdin("y\n")
        .assert()
        .success()
        .stdout(contains("not found"));
}

#[test]
fn rm_with_y_removes_profile_from_kee_config() {
    let tmp = TempDir::new().unwrap();
    seed_config(tmp.path(), &fixture_config());

    // The AWS CLI step also tries to remove the profile from ~/.aws/config;
    // since that file doesn't exist, kee logs a warning but still succeeds.
    kee(tmp.path())
        .args(["rm", "acme.dev"])
        .write_stdin("y\n")
        .assert()
        .success();

    let cfg = read_config(tmp.path());
    assert!(!cfg.profiles.contains_key("acme.dev"));
    assert!(cfg.profiles.contains_key("acme.prod"));
}

#[test]
fn rm_with_n_keeps_profile() {
    let tmp = TempDir::new().unwrap();
    seed_config(tmp.path(), &fixture_config());

    kee(tmp.path())
        .args(["rm", "acme.dev"])
        .write_stdin("n\n")
        .assert()
        .success();

    let cfg = read_config(tmp.path());
    assert!(cfg.profiles.contains_key("acme.dev"));
}
