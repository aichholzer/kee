<div align="center">
  <img src="https://raw.githubusercontent.com/aichholzer/kee/refs/heads/main/kee.png" alt="Kee" />
</div>

[![Tests](https://github.com/aichholzer/kee/actions/workflows/test.yml/badge.svg?branch=main)](https://github.com/aichholzer/kee/actions/workflows/test.yml) [![Latest version](https://img.shields.io/crates/v/kee.svg)](https://crates.io/crates/kee) ![License](https://img.shields.io/crates/l/kee.svg)<br />![OSX](https://img.shields.io/badge/-OSX-black) ![Linux](https://img.shields.io/badge/-Linux-green) ![Windows](https://img.shields.io/badge/-Windows-blue)

A simple tool to help you manage multiple AWS profiles, with SSO support and easy account access.

## Features

- 🔐 **SSO integration**: Full support for AWS SSO authentication
- 🚀 **Easy profile access**: Use any configured profile with a single command
- 🎯 **Interactive picker**: Run `kee use` with no arguments to pick a profile with fuzzy search
- 🐚 **Sub-shell isolation**: Each profile runs in its own sub-shell with proper credential isolation
- ⚙️ **One-shot commands**: Run a single command with a profile's credentials via `kee run` or `kee aws`
- 🌐 **Open the console**: `kee console` opens the AWS Management Console in your browser for the chosen profile
- 📝 **Custom aliases**: Use friendly names for your AWS profiles
- 🔍 **Profile management**: Easily list, add, update, and remove profiles
- 🚫 **No stored credentials**: No AWS credentials are stored anywhere - uses AWS SSO tokens
- 🎨 **Shell integration**: Shows current profile in your shell prompt
- ⚡ **Auto-refresh**: Proactively refreshes tokens on every use and keeps sessions alive in the background
- 🚨 **Production safety**: Mark accounts as production to get a visible warning banner

## Security notes

- **No credential storage**: `Kee` never stores AWS access keys or secrets
- **SSO token management**: Uses AWS CLI's built-in SSO token caching
- **Sub-shell isolation**: Each profile's session is isolated in its own shell
- **Automatic cleanup**: Environment variables are cleared when exiting sub-shells

## Why Rust?

- 🚀 **Performance**: Compiled binary, faster startup times
- ⛑️ **Memory safety**: No runtime errors, guaranteed memory safety
- 🌍 **Cross-platform**: Single binary works across platforms
- 👌 **Zero dependencies**: No Python runtime required
- ⚡️ **Concurrent**: Built-in concurrency support for future enhancements

## Installation

### Prerequisites

- Rust 1.86+ (install from [rustup.rs](https://rustup.rs/)) (On Mac with brew: `brew install rust`)
- AWS CLI v2 installed and configured
- Configured AWS SSO account access

### Install from crates.io

```bash
cargo install kee
```

After install, set up shell completions in one command:

```bash
kee completions install
```

This auto-detects your shell (bash, zsh, or fish), drops the script in
the right place, and edits your shell config to load it. Restart your
terminal or `source` the relevant rc file to pick up completions.

### Install from source

Clone this repository:

```bash
git clone https://github.com/keecli/kee.rs.git ~/.kee
```

**Option 1: Automated (recommended)**

```bash
cd ~/.kee
./install.sh
```

> This script will build an optimized `Kee` binary, install it (in `~/.cargo/bin`), and add the folder to your `PATH`.
> It will also install Kee's auto completions.

**Option 2: Manual**

```bash
cd ~/.kee

# Install the binary
cargo install --path .

# Add Cargo's bin directory to your PATH
echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> ~/.zshrc  # For zsh (macOS default)
# OR
echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> ~/.bashrc  # For bash

# Reload your shell configuration
source ~/.zshrc  # or ~/.bashrc
```

**Option 3: Direct copy**

```bash
cd ~/.kee

# Build and copy to a directory already in PATH
cargo build --release
cp target/release/kee ~/.local/bin/  # Make sure ~/.local/bin is in your PATH
```

## Quick Start

### 1. Add your first profile

```bash
kee add mycompany.dev
```

This will:

- Run `aws configure sso --profile company.dev`
- Prompt you for your SSO configuration (start URL, region, etc.)
- Open your browser for SSO authentication
- Let you select your AWS account and role interactively
- Automatically save the configuration to `Kee`

> **Tip:** A session can be liked to multiple profiles. When prompted for a 'session name', use something generic, like your company name.

### 2. Use a profile

Pick interactively:

```bash
kee use
```

Or jump straight to one by name:

```bash
kee use mycompany.dev
```

Either path will:

- Check if SSO credentials are valid
- Automatically run `aws sso login` if needed
- Start a sub-shell with AWS credentials configured
- Update your shell prompt to show the active profile

### 3. Work with AWS

Inside the sub-shell, all AWS CLI commands will use the selected profile's credentials:

```bash
aws:mycompany.dev $ aws s3 ls
aws:mycompany.dev $ aws ec2 describe-instances
aws:mycompany.dev $ exit  # Terminate the session and return to your main shell
```

## Commands

### Show status or help

```bash
kee
```

With no arguments, Kee shows the current active profile if you are inside a session, or prints help text otherwise.

### Add a profile

```bash
kee add PROFILE_NAME
```

Interactively configure a new AWS profile with SSO settings. You'll be asked whether this is a production account — production profiles display a warning banner when active.

### Use a profile

```bash
kee use                # Pick interactively with fuzzy search
kee use PROFILE_NAME   # Skip the picker
```

Use a profile and start a sub-shell with its AWS credentials. With no name, Kee opens a fuzzy picker over your configured profiles. Every `kee use` proactively refreshes the token to give you the maximum session window.

### Run a single command

Use `kee aws` for AWS CLI commands (the common case):

```bash
kee aws PROFILE_NAME ARGS...
```

```bash
kee aws mycompany.dev s3 ls
kee aws mycompany.dev sts get-caller-identity
```

For anything else, use `kee run`:

```bash
kee run PROFILE_NAME -- CMD ARGS...
```

```bash
kee run mycompany.dev -- terraform plan
kee run mycompany.dev -- ./deploy.sh
kee run mycompany.dev -- aws s3 ls    # works too, just longer
```

Both run a single command with the profile's credentials and exit. No sub-shell, no prompt change. The wrapped command's exit code is propagated. Kee's own status messages go to stderr so they don't pollute the wrapped command's stdout. Production profiles still print a warning banner to stderr.

The `--` separator in `kee run` is recommended any time the wrapped command starts with a flag, so Kee doesn't try to interpret it.

### Open the AWS Management Console

```bash
kee console                # Use the active session, or pick interactively
kee console PROFILE_NAME   # Open the console for a specific profile
```

Federates your temporary credentials with AWS and opens the console in your default browser, already signed in to the chosen account and role. No more flipping accounts in the console picker.

The destination region matches the profile's SSO region. You can navigate to other regions from inside the console as usual.

Requires AWS CLI v2.15+ (which provides `aws configure export-credentials`).

### List all profiles

```bash
kee ls
```

Show a quick overview of all configured profiles.

### Show profile status

```bash
kee status
```

Show detailed status of all profiles: session health (active/expired), token expiry countdown, account ID, account alias, and role. Checks run in parallel so the output appears quickly even with many profiles.

### Show current profile

```bash
kee current
```

Display which profile is currently active (if any).

### Update profile settings

```bash
kee set PROFILE_NAME --production       # Mark as production
kee set PROFILE_NAME --no-production    # Unmark as production
```

Update settings for an existing profile.

### Remove a profile

```bash
kee rm                 # Pick interactively
kee rm PROFILE_NAME    # Skip the picker
```

Removes a profile configuration from `Kee` and the AWS config file.

## How It Works

### Configuration storage

- `Kee` stores its configuration in `~/.kee/config.json`
- AWS profiles are created in `~/.aws/config`, following the AWS config pattern
- No AWS credentials are stored - only SSO configuration

### Sub-shell environment

When you use a profile, `Kee`:

1. Validates SSO credentials (refreshes if needed)
2. Updates shell prompt to show current profile
3. Starts a new shell session
4. Cleans up when you exit

### Session management

When you run `kee use`, your session is refreshed proactively — every invocation gives you the maximum session window regardless of how much time was left.

While the sub-shell is active, a background process monitors the token's expiry and refreshes it automatically before it lapses. This means your session stays alive indefinitely as long as the sub-shell is open (limited only by the refresh token registration, typically ~3 months).

If the refresh token is expired or unavailable, `Kee` falls back to the full `aws sso login` flow.

```
 ⠹ Refreshing session...
 [✓] Session refreshed.

 Profile: mycompany.dev
 Kee is starting a sub-shell...
 Type exit to return to your main shell.
```

`Kee` also prevents you from starting a sub-shell while already in one:

```bash
aws:mycompany.dev $ kee use mycompany.prod

You are using a Kee profile: mycompany.dev
Exit the current session first by typing 'exit'
```

### Shell prompt integration

Your shell prompt will show the active profile:

```bash
(mycompany.dev) user@hostname:
```

### Production safety

Profiles marked as production display a bold red warning when you enter the sub-shell:

```
 ⚠️  PRODUCTION ACCOUNT

 Profile: mycompany.prod
 Kee is starting a sub-shell...
 Type exit to return to your main shell.
```

Mark a profile as production during `kee add` or at any time with `kee set PROFILE_NAME --production`.

## Environment variables

When you're using a `Kee` profile, the following environment variables are set:

- `AWS_PROFILE` - The AWS profile name (e.g., `mycompany.dev`)
- `KEE_CURRENT_PROFILE` - The current `Kee` profile name (e.g., `mycompany.dev`)
- `KEE_ACTIVE_PROFILE` - Set to `1` to indicate an active `Kee` profile
- `PS1` - Updated to show the current profile in your prompt (Unix-like systems only)

These variables help `Kee` manage sessions and prevent nested sub-shells.

## Configuration files

### Kee configuration (`~/.kee/config.json`)

```json
{
  "profiles": {
    "mycompany-prod": {
      "profile_name": "mycompany.prod",
      "sso_start_url": "https://mycompany.awsapps.com/start",
      "sso_region": "ap-southeast-2",
      "sso_account_id": "123456789012",
      "sso_role_name": "AdministratorAccess",
      "session_name": "mycompany",
      "production": true
    }
  },
  "current_profile": null
}
```

### AWS config (`~/.aws/config`)

```ini
[profile mycompany.dev]
sso_role_name = AdministratorAccess
sso_session = mycompany
sso_account_id = 123456789098
output = json

[sso-session mycompany]
sso_region = ap-southeast-2
sso_start_url = https://mycompany.awsapps.com/start
sso_registration_scopes = sso:account:access
```

## Cross-platform support

`Kee` works on:

- **macOS**: Full support with shell prompt integration
- **Linux**: Full support with shell prompt integration
- **Windows**: Full support (prompt integration not available)

## Troubleshooting

### SSO login issues

If SSO login fails:

```bash
# Manual SSO login
aws sso login --profile PROFILE_NAME

# Then try using again
kee use PROFILE_NAME
```

### Profile not found

If you get "profile not found" errors:

```bash
# Check AWS config
cat ~/.aws/config

# Re-add the profile if needed
kee rm PROFILE_NAME
kee add PROFILE_NAME
```

### Permission issues

If you get permission errors:

```bash
# Check AWS credentials
aws sts get-caller-identity --profile PROFILE_NAME

# Refresh SSO login
aws sso login --profile PROFILE_NAME
```

## Future enhancements

- **Built-in AWS SDK** integration (no AWS CLI dependency)
- **Configuration validation** at compile time
- **Plugin system** with dynamic loading
- **TUI interface** with real-time updates

**Binary distribution:**

- Single executable file
- No runtime dependencies
- Easy deployment to servers
- Container-friendly

**Package managers:**

- **Cargo**: `cargo install kee`
- **Homebrew**: `brew install kee` (planned)
- **Scoop**: `scoop install kee` (Windows, planned)
- **APT/YUM**: Native packages possible (planned)

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Add tests, if applicable
5. Test your changes: `make test`
6. Submit a pull request

> There is a utilities script which will set up a `pre-commit` hook to run some basic checks on your code before you commit.

```bash
cd ~/.kee
./utilities/githooks.sh
```

### Test coverage

CI runs [`cargo-tarpaulin`](https://github.com/xd009642/tarpaulin) on Linux and fails the build if line coverage drops below 60%. To check locally:

```bash
cargo install cargo-tarpaulin
cargo tarpaulin --out Stdout
```

Tarpaulin only runs on Linux; macOS and Windows users can rely on CI for the coverage report.

A few notes on the floor:

- It's deliberately conservative. Tests for subprocess-driven code (anything that goes through `assert_cmd::Command`) don't show up cleanly in tarpaulin since the binary launches a child.
- The floor is meant to catch regressions, not chase a number. Bump it up over time as coverage improves.

### Versioning

We use [semantic versioning](https://semver.org/). Version bumps are handled with [`cargo-release`](https://github.com/crate-ci/cargo-release).

```bash
cargo install cargo-release
```

When your changes are ready:

```bash
cargo release patch   # Bug fixes:        1.1.0 → 1.1.1
cargo release minor   # New features:     1.1.0 → 1.2.0
cargo release major   # Breaking changes: 1.0.0 → 2.0.0
```

This updates `Cargo.toml`, commits, and tags in one step. Add `--execute` to apply (without it, it runs in dry-run mode).

## License

MIT License - see LICENSE file for details.

## Support

RTFM, then RTFC... If you are still stuck or just need an additional feature, file an [issue](https://github.com/aichholzer/kee/issues).

<div align="center">
✌🏼
</div>
