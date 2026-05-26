//! `kee completions install / uninstall / print`. Zsh-focused; the bash
//! and fish branches are exercised by in-binary unit tests.

use crate::common::kee;
use predicates::str::contains;
use std::fs;
use tempfile::TempDir;

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
