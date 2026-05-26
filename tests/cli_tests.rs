//! End-to-end CLI tests.
//!
//! Each test runs the compiled `kee` binary as a subprocess (via assert_cmd)
//! with a fresh `HOME` pointing at a tempdir. No global state leaks between
//! tests, so they can run in parallel.
//!
//! Tests that need to mock the `aws` CLI use `aws_stub_dir()` to create a
//! tempdir containing an executable `aws` shell script, then prepend that
//! dir to the child's `PATH` via `Command::env`. This is child-env-aware
//! and parallel-safe (each test owns its own stub).
//!
//! Coverage for `kee console` is intentionally excluded: it hits the AWS
//! federation endpoint over HTTPS, which would need an HTTP mock server
//! to test cleanly. Tracked separately.

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
///
/// On Windows, `dirs::home_dir()` reads `USERPROFILE` rather than `HOME`,
/// so we set both to keep the helper portable.
fn kee(home: &Path) -> Command {
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
fn aws_stub_dir(stub_body: &str) -> TempDir {
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
fn kee_with_stub(home: &Path, stub: &TempDir) -> Command {
    let mut cmd = kee(home);
    let parent_path = std::env::var("PATH").unwrap_or_default();
    cmd.env("PATH", format!("{}:{}", stub.path().display(), parent_path));
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
        .env("USERPROFILE", tmp.path())
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
        .env("USERPROFILE", tmp.path())
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

// =============================================================================
// AWS-shelling commands (#[cfg(unix)] — the stub is a POSIX shell script)
// =============================================================================

#[cfg(unix)]
mod aws_shelling {
    use super::*;

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
}
