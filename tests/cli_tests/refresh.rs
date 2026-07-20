//! `kee refresh`.

use crate::common::{aws_stub_dir, fixture_config, kee_with_stub, seed_config};
use predicates::str::contains;
use tempfile::TempDir;

/// Stub whose `aws sso login` succeeds.
const STUB_SSO_OK: &str = r#"#!/bin/sh
case "$1" in
    sso) exit 0 ;;
    *) exit 0 ;;
esac
"#;

/// Stub whose `aws sso login` fails.
const STUB_SSO_FAILS: &str = r#"#!/bin/sh
case "$1" in
    sso) exit 1 ;;
    *) exit 0 ;;
esac
"#;

#[test]
fn refresh_uses_active_session_profile_when_no_arg() {
    // Inside a session, KEE_CURRENT_PROFILE names the profile to refresh, so
    // `kee refresh` with no argument re-auths the current session in place.
    let tmp = TempDir::new().unwrap();
    let home = tmp.path();
    seed_config(home, &fixture_config());

    let stub = aws_stub_dir(STUB_SSO_OK);
    kee_with_stub(home, &stub)
        .env("KEE_CURRENT_PROFILE", "acme.dev")
        .arg("refresh")
        .assert()
        .success()
        .stdout(contains("acme.dev"))
        .stdout(contains("refreshed"));
}

#[test]
fn refresh_accepts_explicit_profile() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path();
    seed_config(home, &fixture_config());

    let stub = aws_stub_dir(STUB_SSO_OK);
    kee_with_stub(home, &stub)
        .args(["refresh", "acme.prod"])
        .assert()
        .success()
        .stdout(contains("acme.prod"))
        .stdout(contains("refreshed"));
}

#[test]
fn refresh_reports_failure_when_sso_login_fails() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path();
    seed_config(home, &fixture_config());

    let stub = aws_stub_dir(STUB_SSO_FAILS);
    kee_with_stub(home, &stub)
        .args(["refresh", "acme.dev"])
        .assert()
        .code(1)
        .stderr(contains("Failed to refresh"));
}
