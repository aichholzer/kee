//! End-to-end CLI tests.
//!
//! Tests are split across files in `tests/cli_tests/` by command surface
//! to keep each file readable. They share a single integration test
//! crate, so `cargo test --test cli_tests` builds them together.
//!
//! Coverage for `kee console` is intentionally limited to the
//! session_token guard: the federation step hits an HTTPS endpoint that
//! would need an HTTP mock server to test cleanly.

#[path = "common/mod.rs"]
mod common;

#[path = "cli_tests/bare.rs"]
mod bare;

#[path = "cli_tests/completions.rs"]
mod completions;

#[path = "cli_tests/current.rs"]
mod current;

#[path = "cli_tests/ls.rs"]
mod ls;

#[path = "cli_tests/rm.rs"]
mod rm;

#[path = "cli_tests/set.rs"]
mod set;

#[path = "cli_tests/use_cmd.rs"]
mod use_cmd;

#[cfg(unix)]
#[path = "cli_tests/aws_shelling.rs"]
mod aws_shelling;
