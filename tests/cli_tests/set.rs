//! `kee set`: production / no-production toggle.

use crate::common::{fixture_config, kee, read_config, seed_config};
use predicates::str::contains;
use tempfile::TempDir;

#[test]
fn set_production_flips_flag_to_true() {
    let tmp = TempDir::new().unwrap();
    seed_config(tmp.path(), &fixture_config());

    kee(tmp.path())
        .args(["set", "acme.dev", "--production"])
        .assert()
        .success()
        .stdout(contains("marked as production"));

    let cfg = read_config(tmp.path());
    assert!(cfg.profiles["acme.dev"].production);
    // Other profile untouched.
    assert!(cfg.profiles["acme.prod"].production);
}

#[test]
fn set_no_production_flips_flag_to_false() {
    let tmp = TempDir::new().unwrap();
    seed_config(tmp.path(), &fixture_config());

    kee(tmp.path())
        .args(["set", "acme.prod", "--no-production"])
        .assert()
        .success()
        .stdout(contains("unmarked as production"));

    let cfg = read_config(tmp.path());
    assert!(!cfg.profiles["acme.prod"].production);
}

#[test]
fn set_unknown_profile_reports_not_found() {
    let tmp = TempDir::new().unwrap();
    seed_config(tmp.path(), &fixture_config());

    kee(tmp.path())
        .args(["set", "nope", "--production"])
        .assert()
        .success()
        .stdout(contains("not found"));
}

#[test]
fn set_rejects_conflicting_flags() {
    let tmp = TempDir::new().unwrap();
    seed_config(tmp.path(), &fixture_config());

    kee(tmp.path())
        .args(["set", "acme.dev", "--production", "--no-production"])
        .assert()
        .failure();
}
