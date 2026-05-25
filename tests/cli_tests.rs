//! End-to-end CLI tests.
//!
//! Each test runs the compiled `kee` binary as a subprocess (via assert_cmd)
//! with a fresh `HOME` pointing at a tempdir. No global state leaks between
//! tests, so they can run in parallel.
//!
//! Tests for commands that shell out to `aws` (`kee add`, `kee status`,
//! `kee aws`, `kee run`, `kee console`) are out of scope here; they need a
//! child-env-aware AWS shim and are tracked separately.

use assert_cmd::Command;
use kee::{KeeConfig, ProfileInfo};
use predicates::prelude::*;
use predicates::str::contains;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

/// Build a populated KeeConfig with two profiles, one of which is current.
fn fixture_config() -> KeeConfig {
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
fn seed_config(home: &Path, cfg: &KeeConfig) {
    let dir = home.join(".kee");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("config.json"),
        serde_json::to_string_pretty(cfg).unwrap(),
    )
    .unwrap();
}

/// Read the persisted KeeConfig from `<home>/.kee/config.json`.
fn read_config(home: &Path) -> KeeConfig {
    let body = fs::read_to_string(home.join(".kee/config.json")).unwrap();
    serde_json::from_str(&body).unwrap()
}

/// Build a `kee` Command with HOME pointed at the tempdir, and KEE_*
/// session env vars cleared so the child sees a clean state regardless of
/// the test runner's environment.
fn kee(home: &Path) -> Command {
    let mut cmd = Command::cargo_bin("kee").unwrap();
    cmd.env("HOME", home)
        .env_remove("KEE_ACTIVE_PROFILE")
        .env_remove("KEE_CURRENT_PROFILE");
    cmd
}

// --- bare invocations ---------------------------------------------------------

#[test]
fn help_lists_all_subcommands() {
    let tmp = TempDir::new().unwrap();
    kee(tmp.path())
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("AWS CLI profile manager"))
        .stdout(contains("Commands:"))
        .stdout(contains("add"))
        .stdout(contains("use"))
        .stdout(contains("ls"))
        .stdout(contains("current"))
        .stdout(contains("rm"))
        .stdout(contains("set"))
        .stdout(contains("status"))
        .stdout(contains("completions"));
}

#[test]
fn version_matches_cargo_pkg_version() {
    let tmp = TempDir::new().unwrap();
    kee(tmp.path())
        .arg("--version")
        .assert()
        .success()
        .stdout(contains("kee"))
        .stdout(contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn unknown_subcommand_exits_nonzero() {
    let tmp = TempDir::new().unwrap();
    kee(tmp.path())
        .arg("not-a-command")
        .assert()
        .failure()
        .stderr(contains("unrecognized").or(contains("error")));
}

#[test]
fn bare_kee_outside_session_prints_help() {
    // Outside an active session, running `kee` with no args should print
    // help (clap's --help output flows to stdout on a fresh terminal).
    let tmp = TempDir::new().unwrap();
    kee(tmp.path())
        .assert()
        .success()
        .stdout(contains("AWS CLI profile manager"));
}

#[test]
fn bare_kee_inside_session_shows_current_profile() {
    // When KEE_ACTIVE_PROFILE is set, `kee` should report the current
    // profile rather than printing help.
    let tmp = TempDir::new().unwrap();
    let mut cmd = Command::cargo_bin("kee").unwrap();
    cmd.env("HOME", tmp.path())
        .env("KEE_ACTIVE_PROFILE", "1")
        .env("KEE_CURRENT_PROFILE", "acme.dev")
        .assert()
        .success()
        .stdout(contains("Current profile"))
        .stdout(contains("acme.dev"));
}

// --- ls -----------------------------------------------------------------------

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
    let output = kee(tmp.path())
        .args(["ls", "--names"])
        .output()
        .unwrap();
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

// --- current ------------------------------------------------------------------

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
        .env("KEE_ACTIVE_PROFILE", "1")
        .env("KEE_CURRENT_PROFILE", "acme.prod")
        .arg("current")
        .assert()
        .success()
        .stdout(contains("acme.prod"));
}

// --- rm -----------------------------------------------------------------------

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

// --- set ----------------------------------------------------------------------

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

// --- use (without trailing args) ----------------------------------------------

#[test]
fn use_unknown_profile_offers_to_add_then_aborts_on_n() {
    let tmp = TempDir::new().unwrap();
    seed_config(tmp.path(), &fixture_config());

    // Decline the "would you like to add now?" prompt.
    kee(tmp.path())
        .args(["use", "ghost"])
        .write_stdin("n\n")
        .assert()
        .success()
        .stdout(contains("not found"));
}

// --- completions install / uninstall (zsh) ------------------------------------

#[test]
fn completions_install_zsh_writes_script_and_edits_rc() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path();

    kee(home)
        .args(["completions", "install", "--shell", "zsh"])
        .assert()
        .success()
        .stdout(contains("Wrote completions to"));

    let completion = home.join(".kee/completions/_kee");
    assert!(completion.exists(), "completion script should be written");
    let body = fs::read_to_string(&completion).unwrap();
    assert!(body.contains("compdef kee"));
    assert!(body.contains("_kee_profiles"));

    let zshrc = fs::read_to_string(home.join(".zshrc")).unwrap();
    assert!(zshrc.contains("# Kee completion"));
    assert!(zshrc.contains("fpath=(~/.kee/completions"));
}

#[test]
fn completions_install_zsh_is_idempotent() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path();

    kee(home)
        .args(["completions", "install", "--shell", "zsh"])
        .assert()
        .success();
    kee(home)
        .args(["completions", "install", "--shell", "zsh"])
        .assert()
        .success()
        .stdout(contains("already configured"));

    // rc edit should appear exactly once.
    let zshrc = fs::read_to_string(home.join(".zshrc")).unwrap();
    assert_eq!(zshrc.matches("# Kee completion").count(), 1);
}

#[test]
fn completions_uninstall_zsh_removes_script_and_rc_edit() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path();

    kee(home)
        .args(["completions", "install", "--shell", "zsh"])
        .assert()
        .success();
    kee(home)
        .args(["completions", "uninstall", "--shell", "zsh"])
        .assert()
        .success()
        .stdout(contains("Removed"));

    assert!(!home.join(".kee/completions/_kee").exists());
    let zshrc = fs::read_to_string(home.join(".zshrc")).unwrap();
    assert!(!zshrc.contains("# Kee completion"));
    assert!(!zshrc.contains("fpath=(~/.kee/completions"));
}

#[test]
fn completions_print_emits_script_to_stdout() {
    let tmp = TempDir::new().unwrap();
    let output = kee(tmp.path())
        .args(["completions", "print", "zsh"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let body = std::str::from_utf8(&output.stdout).unwrap();
    assert!(body.contains("compdef kee"));
    assert!(body.contains("_kee_profiles"));
}
