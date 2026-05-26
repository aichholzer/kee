//! Tests for the commands that shell out to `aws`. Unix-only because the
//! stub is a POSIX shell script.

use crate::common::{aws_stub_dir, fixture_config, kee_with_stub, read_config, seed_config};
use predicates::prelude::*;
use predicates::str::contains;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

/// Pre-seed `~/.aws/config` with a working SSO profile so `kee add`
/// has something to read after `aws configure sso` "succeeds".
fn seed_aws_config(home: &Path, profile_name: &str) {
    let aws_dir = home.join(".aws");
    fs::create_dir_all(&aws_dir).unwrap();
    let body = format!(
        "[profile {profile_name}]\n\
         sso_session = acme\n\
         sso_account_id = 123456789012\n\
         sso_role_name = Developer\n\
         output = json\n\
         \n\
         [sso-session acme]\n\
         sso_region = ap-southeast-2\n\
         sso_start_url = https://acme.awsapps.com/start\n\
         sso_registration_scopes = sso:account:access\n"
    );
    fs::write(aws_dir.join("config"), body).unwrap();
}

/// Default stub: `sts get-caller-identity` succeeds, `iam list-account-aliases`
/// returns a known alias, anything else exits 0 silently. Used by tests
/// that just need preflight to pass.
const STUB_DEFAULT: &str = r#"#!/bin/sh
case "$1" in
    sts)
        # get-caller-identity
        exit 0
        ;;
    iam)
        # list-account-aliases
        echo '{"AccountAliases":["acme-prod"]}'
        exit 0
        ;;
    sso-oidc)
        # No cache to refresh in tests; pretend it failed silently.
        exit 1
        ;;
    sso)
        # sso login: succeed without doing anything.
        exit 0
        ;;
    configure)
        # `configure sso` (kee add) or `configure export-credentials`.
        # `kee add` pre-seeds ~/.aws/config in the test; we just succeed.
        # `export-credentials` would need a JSON response, but kee console
        # is excluded from these tests.
        exit 0
        ;;
    *)
        # Pass-through commands invoked by `kee run` / `kee aws`. Echo args
        # so tests can assert on stdout.
        echo "stub-aws-output: $*"
        exit 0
        ;;
esac
"#;

/// Stub that fails the preflight `sts get-caller-identity` so we can
/// verify the SSO-login fallback path.
const STUB_STS_FAILS: &str = r#"#!/bin/sh
case "$1" in
    sts) exit 255 ;;
    sso) exit 0 ;;
    *) exit 1 ;;
esac
"#;

// --- kee add --------------------------------------------------------------

#[test]
fn add_writes_profile_to_kee_config_and_marks_production_when_y() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path();
    seed_aws_config(home, "acme.dev");

    let stub = aws_stub_dir(STUB_DEFAULT);
    kee_with_stub(home, &stub)
        .arg("add")
        .arg("acme.dev")
        .write_stdin("y\n")
        .assert()
        .success()
        .stdout(contains("working").or(contains("added")));

    let cfg = read_config(home);
    let profile = &cfg.profiles["acme.dev"];
    assert_eq!(profile.profile_name, "acme.dev");
    assert_eq!(profile.sso_account_id, "123456789012");
    assert_eq!(profile.sso_role_name, "Developer");
    assert_eq!(profile.sso_region, "ap-southeast-2");
    assert!(profile.production, "y answer should set production=true");
}

#[test]
fn add_marks_non_production_when_n() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path();
    seed_aws_config(home, "acme.dev");

    let stub = aws_stub_dir(STUB_DEFAULT);
    kee_with_stub(home, &stub)
        .arg("add")
        .arg("acme.dev")
        .write_stdin("n\n")
        .assert()
        .success();

    let cfg = read_config(home);
    assert!(!cfg.profiles["acme.dev"].production);
}

// --- kee status -----------------------------------------------------------

#[test]
fn status_renders_per_profile_rows_with_alias() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path();
    seed_config(home, &fixture_config());

    let stub = aws_stub_dir(STUB_DEFAULT);
    kee_with_stub(home, &stub)
        .arg("status")
        .assert()
        .success()
        .stdout(contains("acme.dev"))
        .stdout(contains("acme.prod"))
        .stdout(contains("123456789012"))
        .stdout(contains("999999999999"))
        .stdout(contains("Developer"))
        .stdout(contains("Admin"))
        // Token expiry can't be read in tests (no cache file); rows
        // should still render with an Expired status.
        .stdout(contains("Expired"));
}

#[test]
fn status_handles_alias_lookup_failure_gracefully() {
    // Stub fails the iam call; status should still print the rows
    // without an alias next to the account ID.
    let stub_body = r#"#!/bin/sh
case "$1" in
    iam) exit 1 ;;
    *)   exit 0 ;;
esac
"#;
    let tmp = TempDir::new().unwrap();
    let home = tmp.path();
    seed_config(home, &fixture_config());

    let stub = aws_stub_dir(stub_body);
    kee_with_stub(home, &stub)
        .arg("status")
        .assert()
        .success()
        .stdout(contains("acme.dev"))
        .stdout(contains("123456789012"));
}

#[test]
fn status_with_no_profiles_shows_help() {
    let tmp = TempDir::new().unwrap();
    let stub = aws_stub_dir(STUB_DEFAULT);
    kee_with_stub(tmp.path(), &stub)
        .arg("status")
        .assert()
        .success()
        .stdout(contains("No profiles configured"));
}

// --- kee aws --------------------------------------------------------------

#[test]
fn aws_passthrough_runs_command_and_propagates_stdout() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path();
    seed_config(home, &fixture_config());

    let stub = aws_stub_dir(STUB_DEFAULT);
    kee_with_stub(home, &stub)
        .arg("aws")
        .arg("acme.dev")
        .arg("s3")
        .arg("ls")
        .assert()
        .success()
        // Default stub echoes any unrecognised top-level command; we
        // expect "s3 ls" to land in stdout from the wrapped invocation.
        .stdout(contains("stub-aws-output"))
        .stdout(contains("s3 ls"));
}

#[test]
fn aws_unknown_profile_exits_with_error_code() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path();
    seed_config(home, &fixture_config());

    let stub = aws_stub_dir(STUB_DEFAULT);
    kee_with_stub(home, &stub)
        .arg("aws")
        .arg("ghost")
        .arg("s3")
        .arg("ls")
        .assert()
        .failure()
        .stderr(contains("not found"));
}

#[test]
fn aws_falls_back_to_sso_login_when_preflight_fails() {
    // sts fails -> kee runs `aws sso login`, which in our stub also
    // succeeds. Then sts is called again. We can't easily distinguish
    // the second sts call here, so we stub it to keep failing and
    // verify kee surfaces a clear authentication failure.
    let tmp = TempDir::new().unwrap();
    let home = tmp.path();
    seed_config(home, &fixture_config());

    let stub = aws_stub_dir(STUB_STS_FAILS);
    kee_with_stub(home, &stub)
        .arg("aws")
        .arg("acme.dev")
        .arg("s3")
        .arg("ls")
        .assert()
        .failure()
        .stderr(contains("Failed to authenticate"));
}

// --- kee run --------------------------------------------------------------

#[test]
fn run_wraps_arbitrary_command_with_aws_profile_env() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path();
    seed_config(home, &fixture_config());

    let stub = aws_stub_dir(STUB_DEFAULT);
    // Use /bin/sh -c so we can check $AWS_PROFILE inside the wrapped
    // command. Stdout from the wrapped command should pass through
    // cleanly.
    kee_with_stub(home, &stub)
        .arg("run")
        .arg("acme.dev")
        .arg("--")
        .arg("/bin/sh")
        .arg("-c")
        .arg("echo aws=$AWS_PROFILE kee=$KEE_CURRENT_PROFILE active=$KEE_ACTIVE_PROFILE")
        .assert()
        .success()
        .stdout(contains("aws=acme.dev"))
        .stdout(contains("kee=acme.dev"))
        .stdout(contains("active=1"));
}

#[test]
fn run_propagates_wrapped_command_exit_code() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path();
    seed_config(home, &fixture_config());

    let stub = aws_stub_dir(STUB_DEFAULT);
    kee_with_stub(home, &stub)
        .arg("run")
        .arg("acme.dev")
        .arg("--")
        .arg("/bin/sh")
        .arg("-c")
        .arg("exit 42")
        .assert()
        .code(42);
}

#[test]
fn run_with_empty_command_returns_usage_error() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path();
    seed_config(home, &fixture_config());

    let stub = aws_stub_dir(STUB_DEFAULT);
    kee_with_stub(home, &stub)
        .arg("run")
        .arg("acme.dev")
        .assert()
        .code(2)
        .stderr(contains("specify a command"));
}

#[test]
fn run_unknown_profile_exits_with_error() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path();
    seed_config(home, &fixture_config());

    let stub = aws_stub_dir(STUB_DEFAULT);
    kee_with_stub(home, &stub)
        .arg("run")
        .arg("ghost")
        .arg("--")
        .arg("/bin/true")
        .assert()
        .failure()
        .stderr(contains("not found"));
}

// --- production banner ---------------------------------------------------

#[test]
fn aws_command_shows_production_warning_for_prod_profile() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path();
    seed_config(home, &fixture_config());

    let stub = aws_stub_dir(STUB_DEFAULT);
    kee_with_stub(home, &stub)
        .arg("aws")
        .arg("acme.prod")
        .arg("s3")
        .arg("ls")
        .assert()
        .success()
        .stdout(contains("PRODUCTION ACCOUNT"));
}

// --- --verbose flag -----------------------------------------------------

#[test]
fn verbose_flag_accepted_as_global() {
    // Both before and after the subcommand should work because of
    // `global = true` on the flag.
    let tmp = TempDir::new().unwrap();
    let stub = aws_stub_dir(STUB_DEFAULT);

    kee_with_stub(tmp.path(), &stub)
        .arg("--verbose")
        .arg("ls")
        .assert()
        .success();

    kee_with_stub(tmp.path(), &stub)
        .arg("ls")
        .arg("--verbose")
        .assert()
        .success();
}

#[test]
fn verbose_flag_short_form_works() {
    let tmp = TempDir::new().unwrap();
    let stub = aws_stub_dir(STUB_DEFAULT);

    kee_with_stub(tmp.path(), &stub)
        .args(["-v", "ls"])
        .assert()
        .success();
}

#[test]
fn verbose_emits_diagnostic_when_aws_fails() {
    // With --verbose and the STS-fails stub, the AWS CLI's stderr should
    // be surfaced. The default stub for `kee aws` would normally swallow
    // the `aws sts get-caller-identity` stderr.
    let stub_body = r#"#!/bin/sh
case "$1" in
    sts) echo "kee-test: sts failed loudly" >&2; exit 255 ;;
    sso) exit 0 ;;
    *) exit 1 ;;
esac
"#;
    let tmp = TempDir::new().unwrap();
    let home = tmp.path();
    seed_config(home, &fixture_config());

    let stub = aws_stub_dir(stub_body);
    let output = kee_with_stub(home, &stub)
        .args(["--verbose", "aws", "acme.dev", "s3", "ls"])
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    // The vlog! macro prefixes diagnostic lines with " [v] ".
    assert!(
        stderr.contains("[v]"),
        "expected verbose diagnostic in stderr, got: {stderr}"
    );
    assert!(
        stderr.contains("kee-test: sts failed loudly"),
        "expected aws stderr to be surfaced under --verbose, got: {stderr}"
    );
}

// --- kee console: session_token guard ------------------------------------

/// Stub that:
/// - Lets the `sts get-caller-identity` preflight succeed.
/// - Returns export-credentials JSON *without* a SessionToken field,
///   simulating long-term IAM creds (no STS).
///
/// Any other call falls through to a no-op success.
const STUB_NO_SESSION_TOKEN: &str = r#"#!/bin/sh
case "$1" in
    sts) exit 0 ;;
    sso-oidc) exit 1 ;;
    configure)
        if [ "$2" = "export-credentials" ]; then
            echo '{"Version":1,"AccessKeyId":"AKIA","SecretAccessKey":"secret"}'
            exit 0
        fi
        exit 0
        ;;
    *) exit 0 ;;
esac
"#;

#[test]
fn console_bails_when_credentials_lack_session_token() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path();
    seed_config(home, &fixture_config());

    let stub = aws_stub_dir(STUB_NO_SESSION_TOKEN);
    kee_with_stub(home, &stub)
        .args(["console", "acme.dev"])
        .assert()
        .code(1)
        .stderr(contains("did not return a session token"))
        .stderr(contains(
            "Console federation requires temporary credentials",
        ));
}

#[test]
fn console_bails_when_session_token_is_empty_string() {
    // An explicit but empty SessionToken should hit the same guard as
    // a missing field. Catches the !t.is_empty() check.
    let stub_body = r#"#!/bin/sh
case "$1" in
    sts) exit 0 ;;
    configure)
        if [ "$2" = "export-credentials" ]; then
            echo '{"Version":1,"AccessKeyId":"AKIA","SecretAccessKey":"s","SessionToken":""}'
            exit 0
        fi
        exit 0
        ;;
    *) exit 0 ;;
esac
"#;
    let tmp = TempDir::new().unwrap();
    let home = tmp.path();
    seed_config(home, &fixture_config());

    let stub = aws_stub_dir(stub_body);
    kee_with_stub(home, &stub)
        .args(["console", "acme.dev"])
        .assert()
        .code(1)
        .stderr(contains("did not return a session token"));
}

// --- kee use: shell-launch failure ---------------------------------------

#[test]
fn use_reports_clear_error_when_shell_binary_is_missing() {
    // start_subshell builds Command::new($SHELL) and runs status(). If
    // $SHELL points at a non-existent path, status() returns Err and we
    // should print a helpful message instead of silently saying
    // "Session ended". This is the regression test for Vikunja #9.
    //
    // Command::status() returns Err immediately on ENOENT — no actual
    // fork happens, so the test is safe and fast.
    let tmp = TempDir::new().unwrap();
    let home = tmp.path();
    seed_config(home, &fixture_config());

    let stub = aws_stub_dir(STUB_DEFAULT);
    let output = kee_with_stub(home, &stub)
        .arg("use")
        .arg("acme.dev")
        .env("SHELL", "/nonexistent/path/to/shell")
        .output()
        .unwrap();

    // Exit code is whatever the function returned (Ok); the error is
    // printed to stderr and execution continues to "Session ended".
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Failed to start sub-shell"),
        "expected shell-launch error in stderr, got: {stderr}"
    );
    assert!(
        stderr.contains("Check your $SHELL"),
        "expected $SHELL hint in stderr, got: {stderr}"
    );
}
