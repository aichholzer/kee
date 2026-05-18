<div align="center">
  <img src="https://raw.githubusercontent.com/aichholzer/kee/refs/heads/main/kee.png" alt="Kee" />
</div>

![OSX](https://img.shields.io/badge/-OSX-black) ![OSX](https://img.shields.io/badge/-Linux-red) ![OSX](https://img.shields.io/badge/-Windows-blue)

A simple tool to help you manage multiple AWS profiles, with SSO support and easy account access.

> 🦀 — There's also **Python** [implementation](https://github.com/aichholzer/kee.py).<br />
> However, this version might not receive updates or new features.

## Features

- 🔐 **SSO integration**: Full support for AWS SSO authentication
- 🚀 **Easy profile access**: Use any configured profile with a single command
- 🐚 **Sub-shell isolation**: Each profile runs in its own sub-shell with proper credential isolation
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

- Rust 1.80+ (install from [rustup.rs](https://rustup.rs/)) (On Mac with brew: `brew install rust`)
- AWS CLI v2 installed and configured
- Configured AWS SSO account access

### Install from crates.io

```bash
cargo install kee
```

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

```bash
kee use mycompany.dev
```

This will:

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

### Add a profile

```bash
kee add PROFILE_NAME
```

Interactively configure a new AWS profile with SSO settings. You'll be asked whether this is a production account — production profiles display a warning banner when active.

### Use a profile

```bash
kee use PROFILE_NAME
```

Use a profile and start a sub-shell with its AWS credentials. Every `kee use` proactively refreshes the token to give you the maximum session window.

### List all profiles

```bash
kee ls
```

Show a quick overview of all configured profiles.

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
kee rm PROFILE_NAME
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
 Profile: mycompany.prod
 ⚠️  PRODUCTION ACCOUNT
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
