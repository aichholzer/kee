//! `kee ls` and `kee ls --names`.

use crate::common::{fixture_config, kee, seed_config};
use predicates::str::contains;
use tempfile::TempDir;

#[test]
fn ls_with_no_profiles_shows_help_message() {
    let tmp = TempDir::new().unwrap();
    kee(tmp.path())
        .arg("ls")
        .assert()
        .success()
        .stdout(contains("No profiles configured"))
        .stdout(contains("kee add"));
}

#[test]
fn ls_names_with_no_profiles_is_silent() {
    // `kee ls --names` is meant for scripting; empty configs should produce
    // no output rather than the help text.
    let tmp = TempDir::new().unwrap();
    let output = kee(tmp.path()).args(["ls", "--names"]).output().unwrap();
    assert!(output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout).trim().is_empty(),
        "stdout should be empty for scripting"
    );
}

#[test]
fn ls_names_lists_each_profile_on_its_own_line() {
    let tmp = TempDir::new().unwrap();
    seed_config(tmp.path(), &fixture_config());

    let output = kee(tmp.path()).args(["ls", "--names"]).output().unwrap();
    assert!(output.status.success());

    let names: Vec<&str> = std::str::from_utf8(&output.stdout)
        .unwrap()
        .lines()
        .collect();
    assert_eq!(names.len(), 2);
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(sorted, vec!["acme.dev", "acme.prod"]);
}

#[test]
fn ls_shows_account_id_and_role_per_profile() {
    let tmp = TempDir::new().unwrap();
    seed_config(tmp.path(), &fixture_config());

    kee(tmp.path())
        .arg("ls")
        .assert()
        .success()
        .stdout(contains("acme.dev"))
        .stdout(contains("123456789012"))
        .stdout(contains("Developer"))
        .stdout(contains("acme.prod"))
        .stdout(contains("999999999999"))
        .stdout(contains("Admin"));
}

#[test]
fn ls_marks_current_profile() {
    let tmp = TempDir::new().unwrap();
    let mut cfg = fixture_config();
    cfg.current_profile = Some("acme.dev".to_string());
    seed_config(tmp.path(), &cfg);

    kee(tmp.path())
        .arg("ls")
        .assert()
        .success()
        .stdout(contains("Current profile"));
}
