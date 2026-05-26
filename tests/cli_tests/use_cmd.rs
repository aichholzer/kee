//! `kee use` (without trailing args). The success path lives in
//! `aws_shelling`, since it shells out to `aws` for the preflight.

use crate::common::{fixture_config, kee, seed_config};
use predicates::str::contains;
use tempfile::TempDir;

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
