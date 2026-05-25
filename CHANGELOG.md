# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.7.0] - 2026-05-26

### Added

- Global `--verbose` / `-v` flag prints diagnostic detail to stderr,
  prefixed with `[v]`. Useful when something silently fails:
  - `check_credentials` shows the AWS CLI's stderr on failure
    instead of swallowing it.
  - `do_refresh_token` reports cache parse errors, expired client
    registrations, and the `aws sso-oidc create-token` exit status
    plus stderr on failure.
  - The background `SessionRefresher` logs every refresh attempt
    with timing and outcome.
  - Default behaviour stays silent; verbose is opt-in.

## [1.6.3] - 2026-05-26

### Changed

- `build_issuer()` now reads the hostname via `gethostname` (libc) instead
  of shelling out to the `hostname` binary. Saves a fork+exec on every
  `kee console` invocation and works in environments where `hostname`
  isn't on PATH.

### Fixed

- `kee console` now bails out with a clear message if the profile returns
  credentials without a session token (e.g. long-term IAM creds), instead
  of silently sending an empty string to the AWS federation endpoint and
  surfacing an opaque error.
- `kee use` now reports a clear error if the sub-shell fails to spawn
  (missing `$SHELL`, broken binary, etc). Previously the user just saw
  "Session ended" with no context.

## [1.6.2] - 2026-05-25

### Changed

- `kee status` now shows a spinner while it fetches account aliases from
  AWS, so it's clear something is happening on slow connections.

## [1.6.1] - 2026-05-25

### Breaking

- `kee completions <shell>` is now `kee completions print <shell>`.
  The single-form is replaced by nested actions (`print`, `install`,
  `uninstall`) so the same subcommand owns the whole lifecycle.

### Added

- `kee completions install [--shell SHELL]` auto-detects the user's
  shell, drops the completion script in the right place, and edits
  the shell config idempotently. Re-running is a no-op.
- `kee completions uninstall [--shell SHELL]` reverses the install:
  removes the script and undoes the rc-file edit.

### Removed

- The `scripts/` directory and its three shell scripts. The binary now
  owns the entire completion lifecycle. `install.sh` and `Makefile`
  call `kee completions install` directly.

## [1.6.0] - 2026-05-25

### Added

- `kee completions <shell>` generates a ready-to-source completion
  script for bash, zsh, fish, PowerShell, or Elvish. Completions stay
  in sync with the CLI automatically; new subcommands no longer need
  hand-edited completion files.
- Dynamic profile-name completion in zsh, bash, and fish: tab-complete
  on `kee use`, `kee rm`, `kee set`, `kee run`, `kee aws`, and
  `kee console` enumerates configured profiles via `kee ls --names`.

### Changed

- `scripts/install-auto-complete.sh` now generates completions from the
  binary itself instead of copying static files. Requires `kee` to be on
  `PATH` before running.

### Removed

- The hand-maintained `completions/` directory. Generate fresh scripts
  with `kee completions <shell>` instead.

## [1.5.1] - 2026-05-25

### Changed

- Extracted `ensure_session()` to deduplicate the preflight logic across
  `kee run`, `kee aws`, and `kee console`. No behaviour change.
- Trimmed the CI test matrix to stable on Ubuntu/macOS/Windows plus beta
  on Ubuntu. Replaced manual cache blocks with `Swatinem/rust-cache` and
  swapped `cargo install` calls for `taiki-e/install-action`.

### Fixed

- `utilities/githooks.sh`: ensure `.git/hooks/` exists before copying the
  pre-commit script, and use the correct `${RESET}` variable.

## [1.5.0] - 2026-05-25

### Added

- `kee status` shows session health (active/expired and time remaining)
  for every configured profile, including the AWS account alias when
  available. Profiles are queried in parallel.

## [1.4.2] - 2026-05-21

### Fixed

- `try_refresh_token` now tolerates token rotation by other processes
  (AWS CLI, SDKs, SOPS). When a concurrent process invalidates the
  refresh token, Kee re-reads the cache and treats the session as alive
  if the new token is still valid, instead of triggering a false
  "background session refresh failed" warning.

## [1.4.1] - 2026-05-21

### Added

- `kee console [PROFILE]` federates the profile's temporary credentials
  with AWS and opens the AWS Management Console in the default browser,
  already signed in. Profile resolution: explicit name, then current
  session, then the fuzzy picker. Requires AWS CLI v2.15+.

### Changed

- Bumped MSRV to 1.85 for `edition2024` support required by transitive
  dependencies.

## [1.3.0] - 2026-05-19

### Added

- Interactive fuzzy picker (via `dialoguer`) for `kee use` and `kee rm`
  when called without a profile name.
- `kee run PROFILE -- CMD ARGS...` runs a single command with the
  profile's credentials and exits with the command's exit code. No
  sub-shell.
- `kee aws PROFILE ARGS...` is sugar for `kee run PROFILE -- aws ...`.

### Changed

- Bare `kee` is now informational. Inside a session it shows the current
  profile; outside it prints help. The fuzzy picker stays explicit under
  `kee use` and `kee rm`.

## [1.2.2] - 2026-05-19

### Fixed

- SSO cache writes are now atomic (tmp file plus rename) to avoid a race
  where the AWS CLI could read a half-written cache file while minting
  STS credentials.
- The background session refresher now surfaces failures on the healthy
  to failing transition, instead of silently giving up.

## [1.2.1] - 2026-05-18

### Changed

- Tightened MSRV check.

## [1.2.0] - 2026-05-18

### Added

- Background `SessionRefresher` keeps the SSO session alive while a
  sub-shell is open. Tokens refresh automatically before expiry so
  long-running sessions never lapse.
- Production safety flag on profiles. `kee add` asks if a profile is
  production, and `kee set PROFILE --production`/`--no-production` can
  toggle it later. Production profiles show a bold red warning banner
  when entering the sub-shell.
- `kee set` command for updating profile settings on existing profiles.

## [1.1.2] - 2026-05-18

### Added

- Spinner UI during session refresh and SSO login.

### Changed

- Refresh tokens are now used to silently re-authenticate when the
  access token expires, falling back to the full browser-based SSO
  login only when the refresh token is also unavailable.
- `kee use` proactively refreshes the token on every invocation to
  maximise the session window.

### Fixed

- Various housekeeping: removed dead code, fixed `kee ls --names` empty
  output for scripting, and other small polish.

## [1.1.0] - 2026-05-18

### Added

- Automatic SSO token refresh using cached refresh tokens.
- Published to crates.io as `kee`.

## [1.0.0] - Initial Rust release

### Added

- AWS SSO profile management: `add`, `use`, `ls`, `current`, `rm`.
- Sub-shell isolation per profile with `AWS_PROFILE`,
  `KEE_CURRENT_PROFILE`, and `KEE_ACTIVE_PROFILE` environment variables.
- Shell prompt integration on Unix-like systems via `PS1`.
- Configuration stored in `~/.kee/config.json`; AWS profiles in
  `~/.aws/config` using the modern `sso-session` format.
- Shell completions for zsh, bash, and fish.

[1.7.0]: https://github.com/aichholzer/kee.rs/compare/v1.6.3...v1.7.0
[1.6.3]: https://github.com/aichholzer/kee.rs/compare/v1.6.2...v1.6.3
[1.6.2]: https://github.com/aichholzer/kee.rs/compare/v1.6.1...v1.6.2
[1.6.1]: https://github.com/aichholzer/kee.rs/compare/v1.6.0...v1.6.1
[1.6.0]: https://github.com/aichholzer/kee.rs/compare/v1.5.1...v1.6.0
[1.5.1]: https://github.com/aichholzer/kee.rs/compare/v1.5.0...v1.5.1
[1.5.0]: https://github.com/aichholzer/kee.rs/compare/v1.4.2...v1.5.0
[1.4.2]: https://github.com/aichholzer/kee.rs/compare/v1.4.1...v1.4.2
[1.4.1]: https://github.com/aichholzer/kee.rs/compare/v1.3.0...v1.4.1
[1.3.0]: https://github.com/aichholzer/kee.rs/compare/v1.2.2...v1.3.0
[1.2.2]: https://github.com/aichholzer/kee.rs/compare/v1.2.1...v1.2.2
[1.2.1]: https://github.com/aichholzer/kee.rs/compare/v1.2.0...v1.2.1
[1.2.0]: https://github.com/aichholzer/kee.rs/compare/v1.1.2...v1.2.0
[1.1.2]: https://github.com/aichholzer/kee.rs/compare/v1.1.0...v1.1.2
[1.1.0]: https://github.com/aichholzer/kee.rs/releases/tag/v1.1.0
