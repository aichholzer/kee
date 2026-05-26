//! Bare `kee` invocations: no subcommand, --help, --version, unknown commands.

use crate::common::kee;
use assert_cmd::Command;
use predicates::prelude::*;
use predicates::str::contains;
use tempfile::TempDir;

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
        .env("USERPROFILE", tmp.path())
        .env("KEE_ACTIVE_PROFILE", "1")
        .env("KEE_CURRENT_PROFILE", "acme.dev")
        .assert()
        .success()
        .stdout(contains("Current profile"))
        .stdout(contains("acme.dev"));
}
