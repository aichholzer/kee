use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

mod aws;
use aws::{home_dir, AwsManager, ProfileInfo, SessionHealth};

const BOLD_WHITE: &str = "\x1b[1;37m";
const RESET: &str = "\x1b[0m";
const KEE_ART: &str = r#"

 ██╗  ██╗███████╗███████╗
 ██║ ██╔╝██╔════╝██╔════╝
 █████╔╝ █████╗  █████╗
 ██╔═██╗ ██╔══╝  ██╔══╝
 ██║  ██╗███████╗███████╗
 ╚═╝  ╚═╝╚══════╝╚══════╝

 AWS CLI profile manager"#;

// Environment variable names
const KEE_ACTIVE_PROFILE: &str = "KEE_ACTIVE_PROFILE";
const KEE_CURRENT_PROFILE: &str = "KEE_CURRENT_PROFILE";
const AWS_PROFILE: &str = "AWS_PROFILE";
const AWS_CLI_AUTO_PROMPT: &str = "AWS_CLI_AUTO_PROMPT";
const AWS_PAGER: &str = "AWS_PAGER";

#[derive(Parser)]
#[command(name = "kee")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = format!("{KEE_ART}\n V. {}", env!("CARGO_PKG_VERSION")))]
#[command(long_about = format!("{KEE_ART}\n V. {}\n\nExamples:\n  kee                        Show current profile or this help\n  kee add myprofile          Add a new AWS profile\n  kee use                    Pick a profile interactively (starts sub-shell)\n  kee use myprofile          Use a specific profile (starts sub-shell)\n  kee aws myprofile s3 ls    Run an AWS CLI command with a profile\n  kee run myprofile -- CMD   Run any command with a profile (no sub-shell)\n  kee console                Open the AWS console (current session or picker)\n  kee console myprofile      Open the AWS console for a specific profile\n  kee refresh                Refresh the current session's SSO token in place\n  kee status                 Show all profiles with session status and expiry\n  kee ls                     List all available profiles\n  kee current                Show current, active profile\n  kee rm                     Pick a profile to remove interactively\n  kee rm myprofile           Remove a specific profile", env!("CARGO_PKG_VERSION")))]
struct Cli {
    /// Print diagnostic detail to stderr (AWS CLI errors, refresh outcomes,
    /// cache parsing issues). Useful when something silent-fails.
    #[arg(long, short = 'v', global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Add a new AWS profile
    Add {
        #[arg(value_name = "PROFILE_NAME", help = "Name for the new AWS profile")]
        profile_name: String,
    },
    /// Use an available profile
    Use {
        #[arg(
            value_name = "PROFILE_NAME",
            help = "Name of the AWS profile to use (interactive picker if omitted)"
        )]
        profile_name: Option<String>,
    },
    /// List all available profiles
    Ls {
        /// Only show profile names (useful for scripting)
        #[arg(long)]
        names: bool,
    },
    /// Show current active profile
    Current,
    /// Remove a profile
    Rm {
        #[arg(
            value_name = "PROFILE_NAME",
            help = "Name of the AWS profile to remove (interactive picker if omitted)"
        )]
        profile_name: Option<String>,
    },
    /// Run a command with a profile's credentials (no sub-shell)
    Run {
        #[arg(
            value_name = "PROFILE_NAME",
            help = "Name of the AWS profile to use for the command"
        )]
        profile_name: String,
        #[arg(
            value_name = "COMMAND",
            trailing_var_arg = true,
            allow_hyphen_values = true,
            help = "Command and arguments to run (use -- before flags)"
        )]
        cmd: Vec<String>,
    },
    /// Run an AWS CLI command with a profile's credentials (sugar over `run`)
    Aws {
        #[arg(
            value_name = "PROFILE_NAME",
            help = "Name of the AWS profile to use for the command"
        )]
        profile_name: String,
        #[arg(
            value_name = "ARGS",
            trailing_var_arg = true,
            allow_hyphen_values = true,
            help = "Arguments to pass to 'aws' (e.g. s3 ls)"
        )]
        args: Vec<String>,
    },
    /// Open the AWS Management Console in your browser for a profile
    Console {
        #[arg(
            value_name = "PROFILE_NAME",
            help = "Name of the AWS profile (defaults to current session, then interactive picker)"
        )]
        profile_name: Option<String>,
    },
    /// Refresh a profile's SSO session without leaving the sub-shell
    Refresh {
        #[arg(
            value_name = "PROFILE_NAME",
            help = "Name of the AWS profile to refresh (defaults to current session, then interactive picker)"
        )]
        profile_name: Option<String>,
    },
    /// Show detailed status of all profiles (sessions, expiry, aliases)
    Status,
    /// Update profile settings
    Set {
        #[arg(
            value_name = "PROFILE_NAME",
            help = "Name of the AWS profile to update"
        )]
        profile_name: String,
        /// Mark as a production account
        #[arg(long, conflicts_with = "no_production")]
        production: bool,
        /// Unmark as a production account
        #[arg(long)]
        no_production: bool,
    },
    /// Generate or install shell completion scripts
    Completions {
        #[command(subcommand)]
        action: CompletionsAction,
    },
}

#[derive(Subcommand)]
enum CompletionsAction {
    /// Print the completion script to stdout
    Print {
        #[arg(value_name = "SHELL", help = "Shell to generate completions for")]
        shell: Shell,
    },
    /// Install completions for the current shell (or one specified with --shell)
    Install {
        #[arg(
            long,
            value_name = "SHELL",
            help = "Shell to install completions for (auto-detected if omitted)"
        )]
        shell: Option<Shell>,
    },
    /// Remove installed completions
    Uninstall {
        #[arg(
            long,
            value_name = "SHELL",
            help = "Shell to remove completions for (auto-detected if omitted)"
        )]
        shell: Option<Shell>,
    },
}

#[derive(Serialize, Deserialize, Default)]
struct KeeConfig {
    profiles: HashMap<String, ProfileInfo>,
    current_profile: Option<String>,
}

struct KeeManager {
    config_file: PathBuf,
    aws_manager: AwsManager,
}

fn hlt(text: &str) -> String {
    format!("{BOLD_WHITE}{text}{RESET}")
}

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Generic cancellable background thread. Wraps the running flag plus the
/// JoinHandle and signals/joins on drop. Specialised by Spinner and
/// SessionRefresher; if a third use shows up, just spawn another.
struct BackgroundThread {
    running: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl BackgroundThread {
    /// Spawn a thread. The closure receives a clone of the running flag and
    /// is expected to exit when it sees `false`.
    fn spawn<F>(f: F) -> Self
    where
        F: FnOnce(Arc<AtomicBool>) + Send + 'static,
    {
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = Arc::clone(&running);
        let handle = thread::spawn(move || f(running_clone));
        Self {
            running,
            handle: Some(handle),
        }
    }

    /// Signal stop and wait for the thread to exit. Idempotent.
    fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for BackgroundThread {
    fn drop(&mut self) {
        self.stop();
    }
}

struct Spinner {
    thread: BackgroundThread,
}

impl Spinner {
    fn start(message: &str) -> Self {
        let message = message.to_string();
        let thread = BackgroundThread::spawn(move |running| {
            let mut i = 0;
            while running.load(Ordering::Relaxed) {
                print!(
                    "\r {} {}",
                    SPINNER_FRAMES[i % SPINNER_FRAMES.len()],
                    message
                );
                let _ = io::stdout().flush();
                thread::sleep(Duration::from_millis(80));
                i += 1;
            }
        });
        Self { thread }
    }

    fn stop(mut self, result: &str) {
        self.thread.stop();
        // Clear the spinner line and print the result.
        print!("\r\x1b[2K");
        let _ = io::stdout().flush();
        println!("{result}");
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        // Joining is handled by the inner BackgroundThread's Drop. We just
        // need to clear the spinner line so we don't leave a half-drawn
        // frame on the terminal if someone forgot to call stop().
        self.thread.stop();
        print!("\r\x1b[2K");
        let _ = io::stdout().flush();
    }
}

impl KeeManager {
    fn prompt_user(&self, message: &str) -> io::Result<bool> {
        // In an interactive terminal, use dialoguer for visual consistency
        // with the fuzzy profile picker. When stdin isn't a TTY (piped input
        // from scripts or tests), dialoguer errors with "not a terminal" so
        // we fall back to a plain stdin read that the caller can drive.
        if std::io::IsTerminal::is_terminal(&io::stdin()) {
            dialoguer::Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
                .with_prompt(message)
                .default(false)
                .interact_opt()
                .map(|opt| opt.unwrap_or(false))
                .map_err(io::Error::other)
        } else {
            print!(" {message} (y/N): ");
            io::stdout().flush()?;
            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            Ok(input.trim().eq_ignore_ascii_case("y"))
        }
    }

    /// Show a fuzzy picker over configured profiles. Returns the chosen profile
    /// name, or None if there are no profiles or the user aborted.
    fn pick_profile(&self, prompt: &str) -> io::Result<Option<String>> {
        let config = self.load_config();

        if config.profiles.is_empty() {
            println!(
                "\n [!] No profiles configured.\n Run {} to add one.",
                hlt("kee add PROFILE_NAME")
            );
            return Ok(None);
        }

        // Sorted for stable ordering across invocations.
        let mut names: Vec<String> = config.profiles.keys().cloned().collect();
        names.sort();

        let selection =
            dialoguer::FuzzySelect::with_theme(&dialoguer::theme::ColorfulTheme::default())
                .with_prompt(prompt)
                .items(&names)
                .default(0)
                .interact_opt()
                .map_err(io::Error::other)?;

        Ok(selection.map(|i| names[i].clone()))
    }

    fn new() -> io::Result<Self> {
        let home_dir = home_dir().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "\n [X] Could not find home directory\n",
            )
        })?;

        let config_dir = home_dir.join(".kee");
        let config_file = config_dir.join("config.json");

        // Create .kee directory if it doesn't exist
        fs::create_dir_all(&config_dir)?;

        let aws_manager = AwsManager::new()?;

        Ok(Self {
            config_file,
            aws_manager,
        })
    }

    /// Test-only constructor that lets callers point at a synthetic config
    /// file and AWS manager. Used to drive command logic without touching
    /// the real $HOME or hitting the filesystem in unexpected places.
    #[cfg(test)]
    fn new_with_paths(config_file: PathBuf, aws_manager: AwsManager) -> Self {
        Self {
            config_file,
            aws_manager,
        }
    }

    fn load_config(&self) -> KeeConfig {
        if !self.config_file.exists() {
            return KeeConfig::default();
        }

        match fs::read_to_string(&self.config_file) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => KeeConfig::default(),
        }
    }

    fn save_config(&self, config: &KeeConfig) -> io::Result<()> {
        let content = serde_json::to_string_pretty(config)?;
        fs::write(&self.config_file, content)
    }

    fn add_profile(&self, profile_name: &str) -> io::Result<bool> {
        println!("\n Starting SSO configuration...");
        println!(" (This will open your browser to complete authentication.)");
        println!("\n Follow the prompts:");
        println!("  {} Enter your SSO start URL", hlt("1."));
        println!("  {} Enter your SSO region", hlt("2."));
        println!("  {} Authenticate in your browser", hlt("3."));
        println!("  {} Select your AWS account", hlt("4."));
        println!("  {} Select your role", hlt("5."));
        println!(
            "  {} Choose your output format (recommend: json)",
            hlt("6.")
        );
        println!(
            "\n  {} A session can be liked to multiple profiles.\n  When prompted for a 'session name', use something generic, like your company name.\n",
            hlt("Tip:")
        );

        // Run aws configure sso
        let status = Command::new("aws")
            .args(["configure", "sso", "--profile", profile_name])
            .status()?;

        if !status.success() {
            println!(" [X] SSO configuration failed.");
            return Ok(false);
        }

        println!(
            "\n {} You can ignore the AWS CLI example above.\n {} will handle profiles for you.",
            hlt("Note:"),
            hlt("Kee")
        );

        // Reformat the AWS config file
        self.aws_manager.format_config()?;

        // Read profile info
        let profile_info = match self.aws_manager.read_profile(profile_name) {
            Some(info) => info,
            None => {
                println!("\n [X] Could not read profile information.");
                return Ok(false);
            }
        };

        // Save to kee config
        let mut config = self.load_config();

        // Ask if this is a production account
        let production =
            self.prompt_user(&format!("Is {} a production account?", hlt(profile_name)))?;
        let mut profile_info = profile_info;
        profile_info.production = production;

        config
            .profiles
            .insert(profile_name.to_string(), profile_info);
        self.save_config(&config)?;

        // Test the profile
        if self.check_credentials(profile_name) {
            println!("\n [✓] The profile was added and it's working!");
        } else {
            println!("\n [X] I created the profile but credentials may need a refresh...");
            println!(" {} aws sso login --profile {}", hlt("Try:"), profile_name);
        }

        Ok(true)
    }

    fn list_profiles(&self, names: bool) {
        let config = self.load_config();

        if config.profiles.is_empty() {
            if !names {
                println!(
                    "\n [!] No profiles configured.\n Run {} to add one.",
                    hlt("kee add PROFILE_NAME")
                );
            }
            return;
        }

        if names {
            for profile_name in config.profiles.keys() {
                println!("{profile_name}");
            }
            return;
        }

        println!();
        for (profile_name, profile_info) in &config.profiles {
            let status = if Some(profile_name.as_str()) == config.current_profile.as_deref() {
                " (Current profile)"
            } else {
                ""
            };

            println!(" {}{}", hlt(profile_name), status);
            println!(" • {} {}", hlt("Account ID:"), profile_info.sso_account_id);
            println!(" • {} {}\n", hlt("Role:"), profile_info.sso_role_name);
        }
    }

    fn remove_profile(&self, profile_name: &str) -> io::Result<bool> {
        let mut config = self.load_config();

        if !config.profiles.contains_key(profile_name) {
            println!("\n [!] Profile '{}' not found.", hlt(profile_name));
            return Ok(false);
        }

        // Confirm removal
        if !self.prompt_user(&format!(
            "Are you sure you want to remove profile '{}'?",
            hlt(profile_name)
        ))? {
            return Ok(false);
        }

        // Get profile info before removal
        let profile_info = config.profiles.remove(profile_name).unwrap();

        // Clear current profile if it's the one being removed
        if config.current_profile.as_deref() == Some(profile_name) {
            config.current_profile = None;
        }

        self.save_config(&config)?;

        // Remove the AWS profile from config file
        let hlt_profile = hlt(profile_name);
        match self.aws_manager.remove_profile(&profile_info.profile_name) {
            Ok(_) => {
                println!(" [✓] Profile '{hlt_profile}' has been removed.");
            }
            Err(e) => {
                println!(" [✓] Profile '{hlt_profile}' removed from {}.", hlt("Kee"));
                println!(
                    " [!] Could not remove AWS profile '{}': {}",
                    hlt(&profile_info.profile_name),
                    e
                );
                println!(
                    " You may want to remove it manually from {}",
                    hlt("~/.aws/config")
                );
            }
        }

        Ok(true)
    }

    fn set_profile(
        &self,
        profile_name: &str,
        production: bool,
        no_production: bool,
    ) -> io::Result<bool> {
        let mut config = self.load_config();

        let profile = match config.profiles.get_mut(profile_name) {
            Some(p) => p,
            None => {
                println!("\n [!] Profile '{}' not found.", hlt(profile_name));
                return Ok(false);
            }
        };

        if production {
            profile.production = true;
            println!(
                "\n [✓] Profile '{}' marked as production.",
                hlt(profile_name)
            );
        } else if no_production {
            profile.production = false;
            println!(
                "\n [✓] Profile '{}' unmarked as production.",
                hlt(profile_name)
            );
        }

        self.save_config(&config)?;
        Ok(true)
    }

    fn use_profile(&self, profile_name: &str) -> io::Result<bool> {
        // Check if already in a Kee profile
        if env::var(KEE_ACTIVE_PROFILE).is_ok() {
            let current_profile =
                env::var(KEE_CURRENT_PROFILE).unwrap_or_else(|_| "unknown".to_string());
            println!(
                "\n [!] You are using a {} profile: {}",
                hlt("Kee"),
                hlt(&current_profile)
            );
            println!(" Exit the current session first by typing {}", hlt("exit"));
            return Ok(false);
        }

        let mut config = self.load_config();
        let hlt_profile = hlt(profile_name);

        if !config.profiles.contains_key(profile_name) {
            println!("\n [!] Profile '{hlt_profile}' not found.");

            if !config.profiles.is_empty() {
                println!(" Available profiles:");
                for name in config.profiles.keys() {
                    println!(" • {}\n", hlt(name));
                }
            }

            // Offer to add the profile
            if self.prompt_user("Would you like to add now?")? {
                if self.add_profile(profile_name)? {
                    if self.prompt_user(&format!(
                        "Would you like to use profile '{hlt_profile}' now?"
                    ))? {
                        // Reload config
                        config = self.load_config();
                    } else {
                        println!(
                            "\n Profile '{}' is ready to use. Run {} when needed.",
                            hlt_profile,
                            hlt(&format!("kee use {profile_name}"))
                        );
                        return Ok(true);
                    }
                } else {
                    println!(" [X] Failed to add profile '{hlt_profile}'.");
                    return Ok(false);
                }
            } else {
                return Ok(false);
            }
        }

        let profile_info = config.profiles.get(profile_name).unwrap().clone();
        let profile_name = profile_info.profile_name.clone();

        // Validate the session. We don't proactively rotate the SSO token;
        // the AWS CLI/SDK refresh it on demand when it expires. We just check
        // the session is usable and, if not, run a full SSO login.
        println!();
        let spinner = Spinner::start("Checking session...");
        if self.check_credentials(&profile_name) {
            spinner.stop(" [✓] Session is valid.");
        } else {
            spinner.stop(" [!] Session expired. Opening SSO login...");
            if !self.sso_login(&profile_name)? {
                println!(
                    " [X] Failed to authenticate. Please run {} manually.",
                    hlt("aws sso login")
                );
                return Ok(false);
            }
        }

        // Update current profile
        config.current_profile = Some(profile_name.clone());
        self.save_config(&config)?;

        // Start subshell
        self.start_subshell(&profile_info)?;

        // Clear current profile when subshell exits
        config.current_profile = None;
        self.save_config(&config)?;

        Ok(true)
    }

    /// Run a single command with the chosen profile's credentials and exit
    /// with the command's exit code. No sub-shell. Output of the wrapped
    /// command is left untouched (Kee's own status messages go to stderr).
    fn run_command(&self, profile_name: &str, cmd: &[String]) -> io::Result<i32> {
        if cmd.is_empty() {
            eprintln!("\n [X] Please specify a command to run");
            eprintln!(" Usage: {}", hlt("kee run PROFILE_NAME -- CMD ARGS..."));
            return Ok(2);
        }

        let profile_info = match self.ensure_session(profile_name)? {
            Some(p) => p,
            None => return Ok(1),
        };
        let aws_profile = &profile_info.profile_name;

        let (program, args) = cmd.split_first().unwrap();
        let status = Command::new(program)
            .args(args)
            .env(AWS_PROFILE, aws_profile)
            .env(KEE_CURRENT_PROFILE, aws_profile)
            .env(KEE_ACTIVE_PROFILE, "1")
            .status();

        match status {
            Ok(s) => Ok(s.code().unwrap_or(1)),
            Err(e) => {
                eprintln!(" [X] Failed to execute '{program}': {e}");
                Ok(127)
            }
        }
    }

    /// Show detailed status of all profiles: account ID, role, alias, token
    /// expiry, and whether the session is currently valid. Checks run in
    /// parallel (one thread per profile) to keep latency low.
    fn status_command(&self) -> io::Result<()> {
        let config = self.load_config();

        if config.profiles.is_empty() {
            println!(
                "\n [!] No profiles configured.\n Run {} to add one.",
                hlt("kee add PROFILE_NAME")
            );
            return Ok(());
        }

        // Collect profiles, then group them by the SSO token each one uses.
        // Profiles that share an `sso-session` share a single cached SSO
        // token whose refresh token is single-use and rotates on every
        // refresh. Firing concurrent AWS calls for two such profiles makes
        // them race to refresh that one token: the loser gets an
        // InvalidGrantException and the shared cache can be left holding a
        // consumed refresh token, which breaks unrelated tools (SOPS, the AWS
        // CLI) until the next `aws sso login`. So we parallelise across
        // distinct tokens but walk the profiles within a token serially.
        let profiles: Vec<(String, ProfileInfo)> = config
            .profiles
            .iter()
            .map(|(name, info)| (name.clone(), info.clone()))
            .collect();

        let current = env::var(KEE_CURRENT_PROFILE).ok();

        // One thread per SSO token group. Within a group the first query warms
        // the token and the rest reuse it, so at most one refresh is ever in
        // flight for a given token.
        let handles: Vec<_> = group_profiles_by_session(profiles)
            .into_iter()
            .map(|group| {
                let aws_manager = self.aws_manager.clone();
                thread::spawn(move || {
                    let mut rows = Vec::with_capacity(group.len());
                    for (name, info) in group {
                        let health = aws_manager.read_session_health(&info);
                        let alias = get_account_alias(&info.profile_name);
                        rows.push((name, info, health, alias));
                    }
                    rows
                })
            })
            .collect();

        // Spinner runs while we wait for the AWS calls (account aliases) to
        // come back. Dropping it clears the line; we don't use stop() because
        // we don't want a result-line printed before the status output.
        println!();
        let spinner = Spinner::start("Fetching session details...");
        let mut results: Vec<_> = handles
            .into_iter()
            .flat_map(|h| h.join().unwrap())
            .collect();
        drop(spinner);

        // Grouping reorders profiles relative to the config, so sort by name
        // for stable, predictable output.
        results.sort_by(|a, b| a.0.cmp(&b.0));

        for (name, info, health, alias) in results {
            let is_current = current.as_deref() == Some(&name);
            let marker = if is_current { " *" } else { "" };

            // Single status line reflecting session health.
            let status_line = format_status_line(health);

            let alias_str = alias.unwrap_or_default();
            println!(" {}{}", hlt(&name), marker);
            println!("   Status:     {status_line}");
            println!(
                "   Account:    {}{}",
                info.sso_account_id,
                if alias_str.is_empty() {
                    String::new()
                } else {
                    format!(" ({alias_str})")
                }
            );
            println!("   Role:       {}", info.sso_role_name);
            println!();
        }

        Ok(())
    }

    /// Open the AWS Management Console in the default browser, federated as
    /// the chosen profile. If profile_name is None, falls back to the active
    /// session, then an interactive picker.
    fn console_command(&self, profile_name: Option<&str>) -> io::Result<i32> {
        // Resolve the profile to use: arg → active session → picker.
        let resolved: Option<String> = match profile_name {
            Some(n) => Some(n.to_string()),
            None => match env::var(KEE_CURRENT_PROFILE) {
                Ok(n) if !n.is_empty() => Some(n),
                _ => self.pick_profile("Open the console for which profile?")?,
            },
        };

        let name = match resolved {
            Some(n) => n,
            None => return Ok(0),
        };

        let profile_info = match self.ensure_session(&name)? {
            Some(p) => p,
            None => return Ok(1),
        };
        let aws_profile = &profile_info.profile_name;

        // Get temp credentials via the AWS CLI. Output is the standard
        // credential_process JSON shape.
        let creds_out = Command::new("aws")
            .args([
                "configure",
                "export-credentials",
                "--profile",
                aws_profile,
                "--format",
                "process",
            ])
            .env(AWS_CLI_AUTO_PROMPT, "off")
            .env(AWS_PAGER, "")
            .output()?;

        if !creds_out.status.success() {
            eprintln!(
                "\n [X] Could not export credentials for '{}'.\n     {}",
                hlt(aws_profile),
                String::from_utf8_lossy(&creds_out.stderr).trim()
            );
            eprintln!(
                "     Requires AWS CLI v2.15+. Run {} to check.",
                hlt("aws --version")
            );
            return Ok(1);
        }

        let creds: ExportedCredentials = match serde_json::from_slice(&creds_out.stdout) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("\n [X] Could not parse exported credentials: {e}");
                return Ok(1);
            }
        };

        // Build the federation session payload. Console federation requires
        // temporary credentials (which always include a session token); long-
        // term IAM creds without one would fail later with a confusing error.
        // Bail early with a clear message instead.
        let session_token = match creds.session_token {
            Some(t) if !t.is_empty() => t,
            _ => {
                eprintln!(
                    "\n [X] Profile '{}' did not return a session token.",
                    hlt(aws_profile)
                );
                eprintln!("     Console federation requires temporary credentials (SSO/STS).");
                return Ok(1);
            }
        };
        let session_json = format!(
            r#"{{"sessionId":"{}","sessionKey":"{}","sessionToken":"{}"}}"#,
            creds.access_key_id, creds.secret_access_key, session_token
        );

        let signin_token = match get_signin_token(&session_json) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("\n [X] Could not get a signin token from AWS: {e}");
                return Ok(1);
            }
        };

        let console_url = console_url_for_region(&profile_info.sso_region);
        let issuer = build_issuer();
        let login_url = format!(
            "https://signin.aws.amazon.com/federation?Action=login&Issuer={}&Destination={}&SigninToken={}",
            url_encode(&issuer),
            url_encode(&console_url),
            url_encode(&signin_token),
        );

        eprintln!(" Opening AWS console for {}...", hlt(aws_profile));
        if let Err(e) = open_in_browser(&login_url) {
            eprintln!("\n [!] Could not open browser automatically: {e}");
            eprintln!(" Open this URL manually:\n {login_url}");
            return Ok(1);
        }

        Ok(0)
    }

    /// Refresh a profile's SSO session in place by running `aws sso login`.
    /// Resolves the profile from the explicit arg, then the active session,
    /// then the interactive picker. Because the sub-shell shares `~/.aws`, the
    /// freshly minted token is picked up by the running session immediately,
    /// so there's no need to exit and re-enter.
    fn refresh_command(&self, profile_name: Option<&str>) -> io::Result<i32> {
        // Resolve the profile to refresh: arg → active session → picker.
        let resolved: Option<String> = match profile_name {
            Some(n) => Some(n.to_string()),
            None => match env::var(KEE_CURRENT_PROFILE) {
                Ok(n) if !n.is_empty() => Some(n),
                _ => self.pick_profile("Refresh which profile?")?,
            },
        };

        let name = match resolved {
            Some(n) => n,
            None => return Ok(0),
        };

        // Map to the AWS profile name via kee config when we can. When the name
        // came from the active session, KEE_CURRENT_PROFILE already holds the
        // AWS profile name, so falling back to it as-is is correct.
        let config = self.load_config();
        let aws_profile = config
            .profiles
            .get(&name)
            .map(|p| p.profile_name.clone())
            .unwrap_or(name);

        println!("\n Refreshing SSO session for {}...", hlt(&aws_profile));
        if self.sso_login(&aws_profile)? {
            println!(" [✓] Session refreshed for {}.", hlt(&aws_profile));
            Ok(0)
        } else {
            eprintln!(
                " [X] Failed to refresh. Try {} manually.",
                hlt(&format!("aws sso login --profile {aws_profile}"))
            );
            Ok(1)
        }
    }

    fn current_profile(&self) {
        // Check if in active session
        if let Ok(current) = env::var(KEE_CURRENT_PROFILE) {
            println!("\n Current profile: {}", hlt(&current));
            println!(" Type {} to return to your main shell.", hlt("exit"));
        } else {
            let config = self.load_config();
            match config.current_profile {
                Some(current) => println!("\n Current profile: {}", hlt(&current)),
                None => println!("\n [!] No profile is currently active."),
            }
        }
    }

    /// Resolve a profile and ensure its SSO session is usable for non-interactive
    /// commands (run, aws, console). Returns the profile info on success.
    ///
    /// Returns `Ok(None)` if the profile doesn't exist or authentication failed —
    /// in that case the caller has already had a user-facing message printed and
    /// should just return the appropriate error code.
    ///
    /// Status messages go to stderr so they don't contaminate the wrapped
    /// command's stdout.
    fn ensure_session(&self, profile_name: &str) -> io::Result<Option<ProfileInfo>> {
        let config = self.load_config();
        let profile_info = match config.profiles.get(profile_name) {
            Some(p) => p.clone(),
            None => {
                eprintln!("\n [!] Profile '{}' not found.", hlt(profile_name));
                eprintln!(" Run {} to see available profiles.", hlt("kee ls"));
                return Ok(None);
            }
        };
        let aws_profile = &profile_info.profile_name;

        // Validate, falling back to interactive sso login. We don't rotate
        // the SSO token ourselves; the AWS CLI/SDK refresh it on demand.
        if !self.check_credentials(aws_profile) {
            eprintln!(
                " Kee: session expired for '{}', running 'aws sso login'...",
                hlt(aws_profile)
            );
            if !self.sso_login(aws_profile)? || !self.check_credentials(aws_profile) {
                eprintln!(
                    " [X] Failed to authenticate. Please run {} manually.",
                    hlt("aws sso login")
                );
                return Ok(None);
            }
        }

        if profile_info.production {
            println!("\n \x1b[1;31m⚠️  PRODUCTION ACCOUNT\x1b[0m");
        }

        Ok(Some(profile_info))
    }

    fn check_credentials(&self, profile_name: &str) -> bool {
        let mut cmd = Command::new("aws");
        cmd.args(["sts", "get-caller-identity", "--profile", profile_name])
            .env(AWS_CLI_AUTO_PROMPT, "off")
            .env(AWS_PAGER, "");

        // In verbose mode, capture stderr so we can show it on failure.
        // Otherwise discard it so the AWS CLI's auth errors don't pollute
        // the terminal during normal operation.
        if aws::is_verbose() {
            match cmd
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .output()
            {
                Ok(out) => {
                    if !out.status.success() {
                        let stderr = String::from_utf8_lossy(&out.stderr);
                        let trimmed = stderr.trim();
                        if !trimmed.is_empty() {
                            aws::vlog!("aws sts get-caller-identity: {trimmed}");
                        }
                    }
                    out.status.success()
                }
                Err(e) => {
                    aws::vlog!("spawn aws sts: {e}");
                    false
                }
            }
        } else {
            match cmd
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
            {
                Ok(status) => status.success(),
                Err(_) => false,
            }
        }
    }

    fn sso_login(&self, profile_name: &str) -> io::Result<bool> {
        let status = Command::new("aws")
            .args(["sso", "login", "--profile", profile_name])
            .status()?;

        Ok(status.success())
    }

    fn start_subshell(&self, profile_info: &ProfileInfo) -> io::Result<()> {
        let profile_name = &profile_info.profile_name;

        // Get current shell
        let shell = if cfg!(windows) {
            env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string())
        } else {
            env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string())
        };

        // Show banner. Production warning sits between the profile name
        // and the start message so the warning is visually anchored to the
        // profile it applies to.
        println!("\n Profile: {}", hlt(profile_name));
        if profile_info.production {
            println!(" \x1b[1;31m⚠️  PRODUCTION ACCOUNT\x1b[0m");
        }
        println!(" {} is starting a sub-shell...", hlt("Kee"));
        println!(" Type {} to return to your main shell.", hlt("exit"));

        // Start subshell with environment
        let mut cmd = Command::new(&shell);
        cmd.env(AWS_PROFILE, profile_name);
        cmd.env(KEE_CURRENT_PROFILE, profile_name);
        cmd.env(KEE_ACTIVE_PROFILE, "1");

        // Update PS1 for Unix-like systems
        if !cfg!(windows) {
            if let Ok(ps1) = env::var("PS1") {
                cmd.env("PS1", format!("aws:{profile_name} {ps1}"));
            } else {
                cmd.env("PS1", format!("aws:{profile_name} $ "));
            }
        }

        if let Err(e) = cmd.status() {
            eprintln!("\n [X] Failed to start sub-shell ({shell}): {e}");
            eprintln!("     Check your $SHELL environment variable.");
        }

        println!("\n {} — Session ended.", hlt(profile_name));
        Ok(())
    }
}

/// Output shape of `aws configure export-credentials --format process`.
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ExportedCredentials {
    access_key_id: String,
    secret_access_key: String,
    session_token: Option<String>,
}

/// Response from the federation endpoint when requesting a signin token.
#[derive(Deserialize)]
struct SigninTokenResponse {
    #[serde(rename = "SigninToken")]
    signin_token: String,
}

/// Get the account alias for a profile. Returns None if the call fails or
/// there's no alias set. This is a standalone function so it can be called
/// from a spawned thread without borrowing KeeManager.
fn get_account_alias(profile_name: &str) -> Option<String> {
    let output = Command::new("aws")
        .args([
            "iam",
            "list-account-aliases",
            "--profile",
            profile_name,
            "--output",
            "json",
        ])
        .env("AWS_CLI_AUTO_PROMPT", "off")
        .env("AWS_PAGER", "")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())?;

    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    parsed["AccountAliases"]
        .as_array()?
        .first()?
        .as_str()
        .map(|s| s.to_string())
}

/// Group profiles so that every profile sharing one cached SSO token lands in
/// the same bucket. The AWS CLI caches one SSO token per `sso-session` (keyed
/// by start URL), and that token's refresh token is single-use, so profiles in
/// the same bucket must be queried serially to avoid racing the refresh.
/// Profiles with no SSO start URL have no shared cache entry, so each gets its
/// own bucket. Bucket order follows the first appearance of each key in the
/// input, so a caller that feeds sorted input gets stable output.
fn group_profiles_by_session(
    profiles: Vec<(String, ProfileInfo)>,
) -> Vec<Vec<(String, ProfileInfo)>> {
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, Vec<(String, ProfileInfo)>> = HashMap::new();

    for (name, info) in profiles {
        // A profile with no SSO start URL can't share a cached token; give it
        // a unique key (prefixed with a control char that can't appear in a
        // URL) so it runs on its own instead of being lumped in with others.
        let key = if info.sso_start_url.is_empty() {
            format!("\u{1}{name}")
        } else {
            info.sso_start_url.clone()
        };

        if !groups.contains_key(&key) {
            order.push(key.clone());
        }
        groups.entry(key).or_default().push((name, info));
    }

    order
        .into_iter()
        .map(|key| groups.remove(&key).expect("key was inserted above"))
        .collect()
}

/// Render the per-profile status line shown by `kee status`. Pure function:
/// takes the session health and the current time as anchor, returns the
/// coloured status string.
fn format_status_line(health: SessionHealth) -> String {
    format_status_line_at(health, chrono::Utc::now())
}

/// Same as `format_status_line` but with an injectable anchor for testing.
fn format_status_line_at(health: SessionHealth, now: chrono::DateTime<chrono::Utc>) -> String {
    match health {
        SessionHealth::Active(exp) => {
            let remaining = exp - now;
            if remaining.num_seconds() <= 0 {
                // Clock skew, or the token lapsed between the read and here.
                "\x1b[0;31m●\x1b[0m Expired".to_string()
            } else if remaining.num_hours() > 0 {
                let h = remaining.num_hours();
                let m = remaining.num_minutes() % 60;
                format!("\x1b[0;32m●\x1b[0m Active ({h} hours, {m} minutes remaining)")
            } else {
                let m = remaining.num_minutes();
                format!("\x1b[0;32m●\x1b[0m Active ({m} minutes remaining)")
            }
        }
        // Access token lapsed but the session still auto-refreshes: amber, and
        // explicitly not "Expired" so a working session doesn't look dead.
        SessionHealth::Refreshable => "\x1b[0;33m●\x1b[0m Active (auto-refresh)".to_string(),
        SessionHealth::Expired => "\x1b[0;31m●\x1b[0m Expired".to_string(),
    }
}

/// Build the Issuer string AWS shows in the federation audit log.
/// Format: {hostname}/{user}/kee, with fallbacks if either piece is missing.
fn build_issuer() -> String {
    let host = hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown-host".to_string());

    let user = env::var("USER")
        .or_else(|_| env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown-user".to_string());

    format!("{host}/{user}/kee")
}

/// Hit the AWS federation endpoint to exchange temp credentials for a
/// short-lived signin token.
fn get_signin_token(session_json: &str) -> Result<String, String> {
    let url = format!(
        "https://signin.aws.amazon.com/federation?Action=getSigninToken&Session={}",
        url_encode(session_json)
    );

    let resp: SigninTokenResponse = ureq::get(&url)
        .timeout(std::time::Duration::from_secs(10))
        .call()
        .map_err(|e| e.to_string())?
        .into_json()
        .map_err(|e| e.to_string())?;

    Ok(resp.signin_token)
}

/// Pick the regional console URL. The federation endpoint itself is global,
/// but the destination URL determines which region the console lands on.
fn console_url_for_region(region: &str) -> String {
    if region.is_empty() {
        "https://console.aws.amazon.com/".to_string()
    } else {
        format!("https://{region}.console.aws.amazon.com/")
    }
}

/// Minimal RFC 3986 percent-encoding for query-string values. Encodes anything
/// that isn't an unreserved character.
fn url_encode(s: &str) -> String {
    const UNRESERVED: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_.~";
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if UNRESERVED.contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Open a URL in the user's default browser. Uses platform-native commands so
/// we don't take a dependency on the `open` crate.
fn open_in_browser(url: &str) -> io::Result<()> {
    let status = if cfg!(target_os = "macos") {
        Command::new("open").arg(url).status()?
    } else if cfg!(target_os = "windows") {
        Command::new("cmd")
            .args(["/C", "start", "", url])
            .status()?
    } else {
        Command::new("xdg-open").arg(url).status()?
    };

    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "browser opener exited with status {status}"
        )))
    }
}

/// Per-shell snippet that adds dynamic completion of profile names by
/// shelling out to `kee ls --names`. Appended to the base script that
/// clap_complete generates so the user gets profile suggestions for the
/// commands that take a profile name.
fn dynamic_completion_snippet(shell: Shell) -> &'static str {
    match shell {
        Shell::Zsh => {
            r#"
# Kee — dynamic profile name completion
_kee_profiles() {
    local -a profiles
    profiles=("${(@f)$(kee ls --names 2>/dev/null)}")
    if (( ${#profiles[@]} > 0 )) && [[ -n "${profiles[1]}" ]]; then
        _values 'profile' "${profiles[@]}"
    fi
}
"#
        }
        Shell::Bash => {
            r#"
# Kee — dynamic profile name completion
# Wraps clap's _kee completer so existing-profile arguments suggest
# configured profile names. Falls through to clap's completion otherwise.
_kee_with_profiles() {
    local cur prev
    cur="${COMP_WORDS[COMP_CWORD]}"
    prev="${COMP_WORDS[COMP_CWORD-1]}"
    case "$prev" in
        use|rm|set|run|aws|console|refresh)
            local profiles
            profiles=$(kee ls --names 2>/dev/null)
            COMPREPLY=( $(compgen -W "$profiles" -- "$cur") )
            return 0
            ;;
    esac
    # Defer to the clap-generated completer for everything else.
    _kee
}
complete -F _kee_with_profiles -o bashdefault -o default kee
"#
        }
        Shell::Fish => {
            r#"
# Kee — dynamic profile name completion
function __kee_profiles
    kee ls --names 2>/dev/null
end
complete -c kee -n "__fish_seen_subcommand_from use rm set run aws console refresh" -f -a "(__kee_profiles)"
"#
        }
        // PowerShell and Elvish: clap_complete output covers subcommands
        // and flags. Profile name completion isn't wired up; users on
        // those shells can still type names manually.
        _ => "",
    }
}

/// Rewrite clap_complete's zsh output so `profile_name` arguments use our
/// custom `_kee_profiles` function instead of the no-op `_default`.
///
/// `kee add` takes a *new* profile name, so we don't suggest existing ones.
/// Other subcommands' `profile_name` should complete from configured
/// profiles. We also leave the trailing-args completers (`cmd`, `args`) on
/// `kee run` and `kee aws` alone, since those are arbitrary commands.
fn patch_zsh_profile_completion(script: String) -> String {
    // Strategy: swap `:_default` to `:_kee_profiles` only on lines whose
    // help text starts with "Name of the AWS profile". Use a sentinel
    // marker so we can do a targeted replacement without a full parser.
    let sentinel = "\u{0001}KEE_SWAP\u{0001}";

    script
        // Mark exactly the lines we want to swap.
        .replace(
            "profile_name -- Name of the AWS profile",
            &format!("profile_name -- {sentinel}Name of the AWS profile"),
        )
        // For each marked line, swap `:_default` for `:_kee_profiles`.
        // The marker still lives on the line, so this is line-local.
        .lines()
        .map(|line| {
            if line.contains(sentinel) {
                line.replacen(":_default", ":_kee_profiles", 1)
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        // Drop the sentinel.
        .replace(sentinel, "")
        // Preserve the trailing newline that `lines()` strips.
        + "\n"
}

/// Print the completion script for the given shell to stdout.
/// Generate the completion script for the given shell and return it.
fn build_completions(shell: Shell) -> String {
    let mut cmd = Cli::command();
    let bin_name = "kee";
    let mut buf: Vec<u8> = Vec::new();
    generate(shell, &mut cmd, bin_name, &mut buf);
    let mut script = String::from_utf8_lossy(&buf).into_owned();

    // Zsh needs profile_name completers swapped in.
    if matches!(shell, Shell::Zsh) {
        script = patch_zsh_profile_completion(script);
    }

    script.push_str(dynamic_completion_snippet(shell));
    script
}

/// Detect the user's current shell. Looks at $SHELL first, then falls back
/// to checking which rc files exist.
fn detect_current_shell() -> Option<Shell> {
    if let Ok(shell_path) = env::var("SHELL") {
        if let Some(name) = std::path::Path::new(&shell_path)
            .file_name()
            .and_then(|n| n.to_str())
        {
            match name {
                "bash" => return Some(Shell::Bash),
                "zsh" => return Some(Shell::Zsh),
                "fish" => return Some(Shell::Fish),
                _ => {}
            }
        }
    }

    let home = home_dir()?;
    if home.join(".zshrc").exists() {
        Some(Shell::Zsh)
    } else if home.join(".bashrc").exists() || home.join(".bash_profile").exists() {
        Some(Shell::Bash)
    } else if home.join(".config/fish/config.fish").exists() {
        Some(Shell::Fish)
    } else {
        None
    }
}

/// Where the completion script lives on disk for a given shell, plus the
/// rc-file edit (if any) needed to load it.
struct InstallTarget {
    completion_path: PathBuf,
    /// rc file to edit, and the line(s) to ensure are present. Pairs with a
    /// marker substring used to detect prior installs idempotently.
    rc_edit: Option<RcEdit>,
}

struct RcEdit {
    rc_path: PathBuf,
    marker: &'static str,
    block: String,
}

fn install_target(shell: Shell, home: &std::path::Path) -> Option<InstallTarget> {
    match shell {
        Shell::Zsh => {
            let completion_path = home.join(".kee/completions/_kee");
            let rc_path = home.join(".zshrc");
            let block = "\n# Kee completion\nfpath=(~/.kee/completions $fpath)\nautoload -Uz compinit && compinit\n".to_string();
            Some(InstallTarget {
                completion_path,
                rc_edit: Some(RcEdit {
                    rc_path,
                    marker: "~/.kee/completions",
                    block,
                }),
            })
        }
        Shell::Bash => {
            let completion_path = home.join(".kee/.kee_completion.bash");
            // Prefer .bashrc; fall back to .bash_profile if it's the only one.
            let bashrc = home.join(".bashrc");
            let bash_profile = home.join(".bash_profile");
            let rc_path = if bashrc.exists() || !bash_profile.exists() {
                bashrc
            } else {
                bash_profile
            };
            let block = "\n# Kee completion\nsource ~/.kee/.kee_completion.bash\n".to_string();
            Some(InstallTarget {
                completion_path,
                rc_edit: Some(RcEdit {
                    rc_path,
                    marker: ".kee_completion.bash",
                    block,
                }),
            })
        }
        Shell::Fish => {
            // Fish auto-loads anything in this directory; no rc edit needed.
            let completion_path = home.join(".config/fish/completions/kee.fish");
            Some(InstallTarget {
                completion_path,
                rc_edit: None,
            })
        }
        // PowerShell and Elvish: no installer, but the script can still be
        // generated via `kee completions print <shell>`.
        _ => None,
    }
}

/// Append `block` to `rc_path` if `marker` is not already present.
/// Returns true if the rc file was modified.
fn ensure_rc_edit(edit: &RcEdit) -> io::Result<bool> {
    let already_present = if edit.rc_path.exists() {
        let existing = fs::read_to_string(&edit.rc_path)?;
        existing.contains(edit.marker)
    } else {
        false
    };

    if already_present {
        return Ok(false);
    }

    if let Some(parent) = edit.rc_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut content = if edit.rc_path.exists() {
        fs::read_to_string(&edit.rc_path)?
    } else {
        String::new()
    };
    content.push_str(&edit.block);
    fs::write(&edit.rc_path, content)?;
    Ok(true)
}

/// Remove the rc-file block by stripping any contiguous run of lines that
/// includes `marker`. Best-effort: trims trailing whitespace lines too.
fn remove_rc_edit(edit: &RcEdit) -> io::Result<bool> {
    if !edit.rc_path.exists() {
        return Ok(false);
    }
    let content = fs::read_to_string(&edit.rc_path)?;
    if !content.contains(edit.marker) {
        return Ok(false);
    }

    // Drop the marker line and any "# Kee completion" comment immediately
    // above or related lines below in the same block. We use the whole
    // injected block as the search target for a clean removal when it
    // matches verbatim, and a line-filter fallback otherwise.
    let new_content = if content.contains(edit.block.trim_start()) {
        content.replacen(edit.block.trim_start(), "", 1)
    } else {
        content
            .lines()
            .filter(|l| !l.contains(edit.marker) && l.trim() != "# Kee completion")
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    };

    fs::write(&edit.rc_path, new_content)?;
    Ok(true)
}

/// Install completions: generate the script, write it to the right place,
/// and edit the user's rc file to load it.
fn install_completions(shell: Option<Shell>) -> io::Result<()> {
    let shell = match shell.or_else(detect_current_shell) {
        Some(s) => s,
        None => {
            eprintln!(
                "\n [X] Could not detect your shell. Pass {} explicitly.",
                hlt("--shell <SHELL>")
            );
            eprintln!(
                "     Supported: {}, {}, {}",
                hlt("bash"),
                hlt("zsh"),
                hlt("fish")
            );
            return Ok(());
        }
    };

    let home = home_dir()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Could not find home directory"))?;

    let target = match install_target(shell, &home) {
        Some(t) => t,
        None => {
            eprintln!(
                "\n [!] Automatic install isn't supported for {shell:?}.\n     Run {} and follow your shell's documentation.",
                hlt(&format!("kee completions print {shell}"))
            );
            return Ok(());
        }
    };

    if let Some(parent) = target.completion_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let script = build_completions(shell);
    fs::write(&target.completion_path, script)?;
    println!(
        "\n [✓] Wrote completions to {}",
        hlt(&target.completion_path.display().to_string())
    );

    if let Some(ref edit) = target.rc_edit {
        if ensure_rc_edit(edit)? {
            println!(" [✓] Updated {}", hlt(&edit.rc_path.display().to_string()));
        } else {
            println!(
                " [✓] {} already configured",
                hlt(&edit.rc_path.display().to_string())
            );
        }

        let reload = match shell {
            Shell::Zsh => "source ~/.zshrc",
            Shell::Bash => "source ~/.bashrc",
            _ => "restart your terminal",
        };
        println!("\n Restart your terminal or run: {}", hlt(reload));
    } else {
        println!(" Restart your terminal for completions to take effect.");
    }

    Ok(())
}

/// Uninstall completions: remove the file and undo the rc edit.
fn uninstall_completions(shell: Option<Shell>) -> io::Result<()> {
    let shell = match shell.or_else(detect_current_shell) {
        Some(s) => s,
        None => {
            eprintln!(
                "\n [X] Could not detect your shell. Pass {} explicitly.",
                hlt("--shell <SHELL>")
            );
            return Ok(());
        }
    };

    let home = home_dir()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Could not find home directory"))?;

    let target = match install_target(shell, &home) {
        Some(t) => t,
        None => {
            eprintln!("\n [!] No installer for {shell:?}; nothing to uninstall.");
            return Ok(());
        }
    };

    if target.completion_path.exists() {
        fs::remove_file(&target.completion_path)?;
        println!(
            "\n [✓] Removed {}",
            hlt(&target.completion_path.display().to_string())
        );
    } else {
        println!(
            "\n [!] {} did not exist",
            hlt(&target.completion_path.display().to_string())
        );
    }

    if let Some(ref edit) = target.rc_edit {
        if remove_rc_edit(edit)? {
            println!(
                " [✓] Cleaned up {}",
                hlt(&edit.rc_path.display().to_string())
            );
        }
    }

    Ok(())
}

/// Dispatch the `completions` subcommand.
fn run_completions_action(action: CompletionsAction) -> io::Result<()> {
    match action {
        CompletionsAction::Print { shell } => {
            print!("{}", build_completions(shell));
            Ok(())
        }
        CompletionsAction::Install { shell } => install_completions(shell),
        CompletionsAction::Uninstall { shell } => uninstall_completions(shell),
    }
}

fn main() -> io::Result<()> {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => {
            // Customise the missing-arg error for `kee add` only; `use` and `rm`
            // accept an optional name and fall through to a picker.
            if err.kind() == clap::error::ErrorKind::MissingRequiredArgument {
                let error_msg = err.to_string();
                if error_msg.contains("<PROFILE_NAME>") && error_msg.contains("kee add") {
                    eprintln!("\n [X] Please specify a name for the new profile");
                    eprintln!(" Usage: {}", hlt("kee add PROFILE_NAME"));
                    std::process::exit(2);
                }
            }
            // For all other errors, use clap's default handling
            err.exit();
        }
    };

    let kee = KeeManager::new()?;

    // Wire up the global verbose flag so AwsManager can react without
    // needing the flag threaded through every call.
    if cli.verbose {
        aws::VERBOSE.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    // No subcommand: show the current session if active, otherwise print help.
    // Only KEE_ACTIVE_PROFILE counts as "in a session" — the config field can
    // go stale if a sub-shell exits abnormally.
    let command = match cli.command {
        Some(c) => c,
        None => {
            if env::var(KEE_ACTIVE_PROFILE).is_ok() {
                kee.current_profile();
                return Ok(());
            }
            // Re-parse with --help so clap prints and exits.
            let _ = Cli::try_parse_from(["kee", "--help"])
                .err()
                .map(|e| e.exit());
            return Ok(());
        }
    };

    // `completions` doesn't need any kee state; handle it before the rest
    // of the dispatch so it works in any environment.
    if let Commands::Completions { action } = command {
        return run_completions_action(action);
    }

    match command {
        Commands::Add { profile_name } => {
            kee.add_profile(&profile_name)?;
        }
        Commands::Use { profile_name } => {
            let name = match profile_name {
                Some(n) => n,
                None => match kee.pick_profile("Which profile?")? {
                    Some(n) => n,
                    None => return Ok(()),
                },
            };
            kee.use_profile(&name)?;
        }
        Commands::Ls { names } => {
            kee.list_profiles(names);
        }
        Commands::Current => {
            kee.current_profile();
        }
        Commands::Rm { profile_name } => {
            let name = match profile_name {
                Some(n) => n,
                None => match kee.pick_profile("Which profile?")? {
                    Some(n) => n,
                    None => return Ok(()),
                },
            };
            kee.remove_profile(&name)?;
        }
        Commands::Set {
            profile_name,
            production,
            no_production,
        } => {
            kee.set_profile(&profile_name, production, no_production)?;
        }
        Commands::Run { profile_name, cmd } => {
            let code = kee.run_command(&profile_name, &cmd)?;
            std::process::exit(code);
        }
        Commands::Aws { profile_name, args } => {
            // Sugar: prepend "aws" and dispatch through run_command.
            let mut cmd = Vec::with_capacity(args.len() + 1);
            cmd.push("aws".to_string());
            cmd.extend(args);
            let code = kee.run_command(&profile_name, &cmd)?;
            std::process::exit(code);
        }
        Commands::Console { profile_name } => {
            let code = kee.console_command(profile_name.as_deref())?;
            std::process::exit(code);
        }
        Commands::Refresh { profile_name } => {
            let code = kee.refresh_command(profile_name.as_deref())?;
            std::process::exit(code);
        }
        Commands::Status => {
            kee.status_command()?;
        }
        Commands::Completions { .. } => {
            // Handled above. clap's exhaustiveness check still requires the arm.
            unreachable!()
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- group_profiles_by_session --------------------------------------------

    fn profile_on(name: &str, start_url: &str) -> (String, ProfileInfo) {
        (
            name.to_string(),
            ProfileInfo {
                profile_name: name.to_string(),
                sso_start_url: start_url.to_string(),
                sso_region: "ap-southeast-2".to_string(),
                sso_account_id: "123456789012".to_string(),
                sso_role_name: "Role".to_string(),
                session_name: "sess".to_string(),
                production: false,
            },
        )
    }

    #[test]
    fn group_collapses_shared_start_url_into_one_bucket() {
        // Two profiles on the same SSO session share one cached token, so they
        // must land in the same bucket to be queried serially.
        let groups = group_profiles_by_session(vec![
            profile_on("acme.dev", "https://acme.awsapps.com/start"),
            profile_on("acme.prod", "https://acme.awsapps.com/start"),
        ]);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].len(), 2);
    }

    #[test]
    fn group_keeps_distinct_start_urls_separate() {
        let groups = group_profiles_by_session(vec![
            profile_on("acme", "https://acme.awsapps.com/start"),
            profile_on("other", "https://other.awsapps.com/start"),
        ]);
        assert_eq!(groups.len(), 2);
        assert!(groups.iter().all(|g| g.len() == 1));
    }

    #[test]
    fn group_does_not_lump_empty_url_profiles_together() {
        // Legacy profiles without a start URL don't share a cache entry, so
        // each must get its own bucket rather than being serialised together
        // as if they shared a token.
        let groups = group_profiles_by_session(vec![profile_on("a", ""), profile_on("b", "")]);
        assert_eq!(groups.len(), 2);
    }

    #[test]
    fn group_preserves_first_seen_order() {
        let groups = group_profiles_by_session(vec![
            profile_on("b", "https://b.example/start"),
            profile_on("a", "https://a.example/start"),
            profile_on("b2", "https://b.example/start"),
        ]);
        assert_eq!(groups.len(), 2);
        // First key seen is b's URL; b2 joins that bucket. a's URL is second.
        assert_eq!(groups[0][0].0, "b");
        assert_eq!(groups[0][1].0, "b2");
        assert_eq!(groups[1][0].0, "a");
    }

    // -- format_status_line ---------------------------------------------------

    #[test]
    fn status_line_expired_state_renders_expired() {
        let s = format_status_line_at(SessionHealth::Expired, chrono::Utc::now());
        assert!(s.contains("Expired"));
        assert!(!s.contains("Active"));
    }

    #[test]
    fn status_line_refreshable_reads_active_not_expired() {
        // A lapsed-but-refreshable session must not look dead: it is the case
        // where `aws ...` still works while the access token has expired.
        let s = format_status_line_at(SessionHealth::Refreshable, chrono::Utc::now());
        assert!(s.contains("Active"));
        assert!(s.contains("auto-refresh"));
        assert!(!s.contains("Expired"));
    }

    #[test]
    fn status_line_active_in_past_renders_expired() {
        let now = chrono::Utc::now();
        let past = now - chrono::Duration::minutes(5);
        let s = format_status_line_at(SessionHealth::Active(past), now);
        assert!(s.contains("Expired"));
    }

    #[test]
    fn status_line_active_at_exact_zero_boundary_renders_expired() {
        // num_seconds() <= 0 should land on Expired, not Active.
        let now = chrono::Utc::now();
        let s = format_status_line_at(SessionHealth::Active(now), now);
        assert!(s.contains("Expired"));
    }

    #[test]
    fn status_line_active_minutes_only_under_one_hour() {
        let now = chrono::Utc::now();
        let exp = now + chrono::Duration::minutes(45);
        let s = format_status_line_at(SessionHealth::Active(exp), now);
        assert!(s.contains("Active"));
        // No "hours" string when remaining is under an hour.
        assert!(!s.contains("hours"));
        assert!(s.contains("45 minutes remaining"));
    }

    #[test]
    fn status_line_active_includes_hours_and_minutes() {
        let now = chrono::Utc::now();
        let exp = now + chrono::Duration::hours(2) + chrono::Duration::minutes(15);
        let s = format_status_line_at(SessionHealth::Active(exp), now);
        assert!(s.contains("2 hours"));
        assert!(s.contains("15 minutes"));
    }

    #[test]
    fn status_line_active_minutes_modulo_when_hours_present() {
        // 90 minutes -> 1 hour, 30 minutes (not 1 hour, 90 minutes).
        let now = chrono::Utc::now();
        let exp = now + chrono::Duration::minutes(90);
        let s = format_status_line_at(SessionHealth::Active(exp), now);
        assert!(s.contains("1 hours"));
        assert!(s.contains("30 minutes"));
    }

    // -- url_encode -----------------------------------------------------------

    #[test]
    fn url_encode_empty() {
        assert_eq!(url_encode(""), "");
    }

    #[test]
    fn url_encode_unreserved_passes_through() {
        assert_eq!(
            url_encode("ABCabc012-_.~"),
            "ABCabc012-_.~",
            "RFC 3986 unreserved characters should not be encoded"
        );
    }

    #[test]
    fn url_encode_reserved_is_percent_encoded() {
        assert_eq!(url_encode("="), "%3D");
        assert_eq!(url_encode("&"), "%26");
        assert_eq!(url_encode("?"), "%3F");
        assert_eq!(url_encode("/"), "%2F");
        assert_eq!(url_encode(" "), "%20");
    }

    #[test]
    fn url_encode_multi_byte_utf8() {
        // "é" is 0xC3 0xA9 in UTF-8.
        assert_eq!(url_encode("é"), "%C3%A9");
    }

    #[test]
    fn url_encode_realistic_session_payload() {
        // The federation flow encodes a JSON blob; verify a representative
        // sample round-trips into the expected shape (key chars encoded,
        // structure preserved).
        let input = r#"{"sessionId":"AKIA","sessionKey":"secret"}"#;
        let out = url_encode(input);
        assert!(out.starts_with("%7B%22sessionId%22"));
        assert!(out.ends_with("%22secret%22%7D"));
    }

    // -- console_url_for_region -----------------------------------------------

    #[test]
    fn console_url_default_for_empty_region() {
        assert_eq!(
            console_url_for_region(""),
            "https://console.aws.amazon.com/"
        );
    }

    #[test]
    fn console_url_includes_region() {
        assert_eq!(
            console_url_for_region("ap-southeast-2"),
            "https://ap-southeast-2.console.aws.amazon.com/"
        );
    }

    #[test]
    fn console_url_passes_region_through() {
        // No validation on the region string; the input is whatever's in
        // the AWS config.
        assert_eq!(
            console_url_for_region("us-gov-west-1"),
            "https://us-gov-west-1.console.aws.amazon.com/"
        );
    }

    // -- dynamic_completion_snippet -------------------------------------------

    #[test]
    fn snippet_zsh_has_helper_function() {
        let s = dynamic_completion_snippet(Shell::Zsh);
        assert!(s.contains("_kee_profiles"));
        assert!(s.contains("kee ls --names"));
    }

    #[test]
    fn snippet_bash_wraps_clap_completer() {
        let s = dynamic_completion_snippet(Shell::Bash);
        assert!(s.contains("_kee_with_profiles"));
        assert!(s.contains("complete -F _kee_with_profiles"));
        // Falls through to the clap-generated `_kee` for non-profile args.
        assert!(s.contains("_kee\n"));
    }

    #[test]
    fn snippet_fish_uses_subcommand_filter() {
        let s = dynamic_completion_snippet(Shell::Fish);
        assert!(s.contains("__kee_profiles"));
        assert!(s.contains("__fish_seen_subcommand_from use rm set run aws console"));
    }

    #[test]
    fn snippet_powershell_is_empty() {
        // No installer for PowerShell/Elvish; clap_complete output stands alone.
        assert_eq!(dynamic_completion_snippet(Shell::PowerShell), "");
        assert_eq!(dynamic_completion_snippet(Shell::Elvish), "");
    }

    // -- patch_zsh_profile_completion -----------------------------------------

    #[test]
    fn patch_swaps_existing_profile_arguments() {
        let input = "':profile_name -- Name of the AWS profile to use:_default'\n".to_string();
        let out = patch_zsh_profile_completion(input);
        assert!(out.contains(":_kee_profiles"));
        assert!(!out.contains(":_default"));
    }

    #[test]
    fn patch_leaves_kee_add_alone() {
        // `kee add` takes a *new* name; we don't suggest existing profiles.
        let input = "':profile_name -- Name for the new AWS profile:_default'\n".to_string();
        let out = patch_zsh_profile_completion(input);
        assert!(out.contains(":_default"));
        assert!(!out.contains(":_kee_profiles"));
    }

    #[test]
    fn patch_leaves_trailing_args_completer_alone() {
        // `kee run` and `kee aws` take arbitrary trailing args; their
        // `cmd`/`args` completers must stay on `:_default`.
        let input =
            "'*::cmd -- Command and arguments to run (use -- before flags):_default'\n".to_string();
        let out = patch_zsh_profile_completion(input);
        assert!(out.contains(":_default"));
        assert!(!out.contains(":_kee_profiles"));
    }

    #[test]
    fn patch_handles_mixed_input() {
        // The realistic case: clap emits all three kinds in one script.
        let input = "\
':profile_name -- Name for the new AWS profile:_default' \\
'::profile_name -- Name of the AWS profile to use (interactive picker if omitted):_default' \\
'*::cmd -- Command and arguments to run (use -- before flags):_default' \\
':profile_name -- Name of the AWS profile to update:_default' \\
"
        .to_string();
        let out = patch_zsh_profile_completion(input);

        // kee add line: still :_default.
        assert!(out.contains("Name for the new AWS profile:_default"));
        // kee use line: now :_kee_profiles.
        assert!(out.contains(
            "Name of the AWS profile to use (interactive picker if omitted):_kee_profiles"
        ));
        // cmd/args line: still :_default.
        assert!(out.contains("Command and arguments to run (use -- before flags):_default"));
        // kee set line: now :_kee_profiles.
        assert!(out.contains("Name of the AWS profile to update:_kee_profiles"));
    }

    // -- build_issuer ---------------------------------------------------------

    #[test]
    fn build_issuer_has_kee_suffix_and_slash_structure() {
        let issuer = build_issuer();
        // Format: {host}/{user}/kee — three parts separated by `/`.
        let parts: Vec<&str> = issuer.split('/').collect();
        assert_eq!(
            parts.len(),
            3,
            "issuer should have three slash-separated parts: {issuer}"
        );
        assert_eq!(parts[2], "kee");
        // Host and user portions must be non-empty (fallbacks kick in if needed).
        assert!(!parts[0].is_empty());
        assert!(!parts[1].is_empty());
    }

    // -- BackgroundThread -----------------------------------------------------

    #[test]
    fn background_thread_runs_to_completion_when_signalled_to_stop() {
        use std::sync::atomic::AtomicUsize;
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = Arc::clone(&counter);

        let mut bg = BackgroundThread::spawn(move |running| {
            while running.load(Ordering::Relaxed) {
                counter_clone.fetch_add(1, Ordering::Relaxed);
                thread::sleep(Duration::from_millis(5));
            }
        });

        // Let it tick a few times, then stop.
        thread::sleep(Duration::from_millis(50));
        bg.stop();

        // The counter incremented at least once and the thread joined cleanly.
        assert!(counter.load(Ordering::Relaxed) > 0);
    }

    #[test]
    fn background_thread_drop_signals_and_joins() {
        // If we don't call stop() explicitly, drop() should do it for us.
        // Otherwise the test would hang.
        use std::sync::atomic::AtomicBool;
        let exited = Arc::new(AtomicBool::new(false));
        let exited_clone = Arc::clone(&exited);

        {
            let _bg = BackgroundThread::spawn(move |running| {
                while running.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_millis(5));
                }
                exited_clone.store(true, Ordering::Relaxed);
            });
            thread::sleep(Duration::from_millis(20));
        } // _bg dropped here; should signal stop and join.

        assert!(
            exited.load(Ordering::Relaxed),
            "thread should have observed stop and exited"
        );
    }

    #[test]
    fn background_thread_stop_is_idempotent() {
        let mut bg = BackgroundThread::spawn(|running| {
            while running.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(5));
            }
        });

        bg.stop();
        bg.stop(); // Should not panic; second call is a no-op.
    }

    // -- install_target -------------------------------------------------------

    #[test]
    fn install_target_zsh_uses_kee_completions_dir() {
        let home = std::path::PathBuf::from("/tmp/fake-home");
        let target = install_target(Shell::Zsh, &home).unwrap();
        assert_eq!(target.completion_path, home.join(".kee/completions/_kee"));
        let edit = target.rc_edit.unwrap();
        assert_eq!(edit.rc_path, home.join(".zshrc"));
        assert!(edit.block.contains("fpath=(~/.kee/completions"));
        assert!(edit.block.contains("compinit"));
    }

    #[test]
    fn install_target_bash_prefers_bashrc_when_present() {
        let tmp = tempfile::TempDir::new().unwrap();
        let home = tmp.path();
        std::fs::write(home.join(".bashrc"), "").unwrap();

        let target = install_target(Shell::Bash, home).unwrap();
        assert_eq!(target.rc_edit.unwrap().rc_path, home.join(".bashrc"));
    }

    #[test]
    fn install_target_bash_falls_back_to_bash_profile() {
        let tmp = tempfile::TempDir::new().unwrap();
        let home = tmp.path();
        // No .bashrc present.
        std::fs::write(home.join(".bash_profile"), "").unwrap();

        let target = install_target(Shell::Bash, home).unwrap();
        assert_eq!(target.rc_edit.unwrap().rc_path, home.join(".bash_profile"));
    }

    #[test]
    fn install_target_fish_skips_rc_edit() {
        let home = std::path::PathBuf::from("/tmp/fake-home");
        let target = install_target(Shell::Fish, &home).unwrap();
        assert_eq!(
            target.completion_path,
            home.join(".config/fish/completions/kee.fish")
        );
        // Fish auto-loads from the completions directory; no rc edit needed.
        assert!(target.rc_edit.is_none());
    }

    #[test]
    fn install_target_unsupported_shells_return_none() {
        let home = std::path::PathBuf::from("/tmp/fake-home");
        assert!(install_target(Shell::PowerShell, &home).is_none());
        assert!(install_target(Shell::Elvish, &home).is_none());
    }

    // -- ensure_rc_edit -------------------------------------------------------

    fn make_edit(rc_path: std::path::PathBuf, marker: &'static str, block: &str) -> RcEdit {
        RcEdit {
            rc_path,
            marker,
            block: block.to_string(),
        }
    }

    #[test]
    fn ensure_rc_edit_appends_when_marker_absent() {
        let tmp = tempfile::TempDir::new().unwrap();
        let rc = tmp.path().join(".zshrc");
        std::fs::write(&rc, "# existing line\n").unwrap();
        let edit = make_edit(rc.clone(), "kee marker", "\n# kee marker\nappended\n");

        let modified = ensure_rc_edit(&edit).unwrap();
        assert!(modified);
        let body = std::fs::read_to_string(&rc).unwrap();
        assert!(body.contains("# existing line"));
        assert!(body.contains("# kee marker"));
        assert!(body.contains("appended"));
    }

    #[test]
    fn ensure_rc_edit_is_idempotent() {
        let tmp = tempfile::TempDir::new().unwrap();
        let rc = tmp.path().join(".zshrc");
        std::fs::write(&rc, "preamble\n# kee marker\nalready here\n").unwrap();
        let edit = make_edit(rc.clone(), "kee marker", "\n# kee marker\nalready here\n");

        let modified = ensure_rc_edit(&edit).unwrap();
        assert!(!modified, "second run should detect marker and skip");
        let body = std::fs::read_to_string(&rc).unwrap();
        // Should still contain the marker only once.
        assert_eq!(body.matches("# kee marker").count(), 1);
    }

    #[test]
    fn ensure_rc_edit_creates_rc_file_when_missing() {
        let tmp = tempfile::TempDir::new().unwrap();
        let rc = tmp.path().join(".zshrc");
        // rc file doesn't exist yet.
        let edit = make_edit(rc.clone(), "kee marker", "# kee marker\nbody\n");

        let modified = ensure_rc_edit(&edit).unwrap();
        assert!(modified);
        assert!(rc.exists());
        let body = std::fs::read_to_string(&rc).unwrap();
        assert!(body.contains("# kee marker"));
    }

    #[test]
    fn ensure_rc_edit_creates_parent_dir() {
        let tmp = tempfile::TempDir::new().unwrap();
        let rc = tmp.path().join("nested/dir/.config");
        let edit = make_edit(rc.clone(), "kee marker", "# kee marker\nbody\n");

        let modified = ensure_rc_edit(&edit).unwrap();
        assert!(modified);
        assert!(rc.exists());
    }

    // -- remove_rc_edit -------------------------------------------------------

    #[test]
    fn remove_rc_edit_strips_verbatim_block() {
        let tmp = tempfile::TempDir::new().unwrap();
        let rc = tmp.path().join(".zshrc");
        let block = "\n# kee marker\nbody\n";
        std::fs::write(&rc, format!("preamble\n{block}suffix\n")).unwrap();
        let edit = make_edit(rc.clone(), "kee marker", block);

        let modified = remove_rc_edit(&edit).unwrap();
        assert!(modified);
        let body = std::fs::read_to_string(&rc).unwrap();
        assert!(body.contains("preamble"));
        assert!(body.contains("suffix"));
        assert!(!body.contains("kee marker"));
    }

    #[test]
    fn remove_rc_edit_falls_back_to_line_filter() {
        let tmp = tempfile::TempDir::new().unwrap();
        let rc = tmp.path().join(".zshrc");
        // Marker line is present but the surrounding whitespace differs from
        // the canonical block, so the verbatim replacement won't match.
        std::fs::write(
            &rc,
            "preamble\n# Kee completion\nfpath=(kee marker stuff)\nsuffix\n",
        )
        .unwrap();
        let edit = make_edit(
            rc.clone(),
            "kee marker",
            "\n# Kee completion\nfpath=(kee marker stuff)\n",
        );

        let modified = remove_rc_edit(&edit).unwrap();
        assert!(modified);
        let body = std::fs::read_to_string(&rc).unwrap();
        assert!(body.contains("preamble"));
        assert!(body.contains("suffix"));
        assert!(!body.contains("kee marker"));
        assert!(!body.contains("# Kee completion"));
    }

    #[test]
    fn remove_rc_edit_no_op_when_marker_absent() {
        let tmp = tempfile::TempDir::new().unwrap();
        let rc = tmp.path().join(".zshrc");
        std::fs::write(&rc, "just preamble\n").unwrap();
        let edit = make_edit(rc.clone(), "kee marker", "# kee marker\nbody\n");

        let modified = remove_rc_edit(&edit).unwrap();
        assert!(!modified);
        let body = std::fs::read_to_string(&rc).unwrap();
        assert_eq!(body, "just preamble\n");
    }

    #[test]
    fn remove_rc_edit_no_op_when_rc_missing() {
        let tmp = tempfile::TempDir::new().unwrap();
        let rc = tmp.path().join("does-not-exist");
        let edit = make_edit(rc, "kee marker", "# kee marker\nbody\n");

        let modified = remove_rc_edit(&edit).unwrap();
        assert!(!modified);
    }

    // -- AWS CLI mocking via PATH shim ----------------------------------------
    //
    // These tests cover the binary's direct shell-outs to `aws`. The real
    // binary on PATH is replaced with a small shell script that branches on
    // its arguments and a KEE_TEST_AWS_MODE env var. PATH manipulation is
    // process-global, so the tests run serially via #[serial_test::serial].

    #[cfg(unix)]
    mod aws_cli_shim {
        use super::*;
        use serial_test::serial;
        use std::os::unix::fs::PermissionsExt;

        /// Guard that restores the previous PATH when dropped.
        struct PathGuard {
            previous: Option<String>,
        }

        impl Drop for PathGuard {
            fn drop(&mut self) {
                match &self.previous {
                    Some(p) => env::set_var("PATH", p),
                    None => env::remove_var("PATH"),
                }
            }
        }

        /// Write an executable shell stub that simulates `aws`. Returns a
        /// PathGuard that restores PATH when dropped, plus the tempdir kept
        /// alive for the test's duration.
        fn install_aws_stub(stub_body: &str) -> (PathGuard, tempfile::TempDir) {
            let tmp = tempfile::TempDir::new().unwrap();
            let stub_path = tmp.path().join("aws");
            std::fs::write(&stub_path, stub_body).unwrap();
            std::fs::set_permissions(&stub_path, std::fs::Permissions::from_mode(0o755)).unwrap();

            let previous = env::var("PATH").ok();
            let new_path = match &previous {
                Some(p) => format!("{}:{}", tmp.path().display(), p),
                None => tmp.path().display().to_string(),
            };
            env::set_var("PATH", &new_path);

            (PathGuard { previous }, tmp)
        }

        fn manager() -> KeeManager {
            // The KeeManager we build is driven only through the methods that
            // shell out to `aws`. Config file path and AwsManager paths can
            // point at junk; nothing reads from them in these tests.
            let tmp = tempfile::TempDir::new().unwrap();
            let aws_manager = AwsManager::new_with_paths(
                tmp.path().join("aws-config"),
                tmp.path().join("sso-cache"),
            );
            // We leak the tempdir intentionally; the OS reclaims it after
            // the test process exits. Otherwise dropping it here would race
            // with later filesystem access.
            std::mem::forget(tmp);
            KeeManager::new_with_paths(std::path::PathBuf::from("/nonexistent"), aws_manager)
        }

        // -- check_credentials ------------------------------------------------

        #[test]
        #[serial]
        fn check_credentials_returns_true_on_zero_exit() {
            let (_guard, _tmp) = install_aws_stub("#!/bin/sh\nexit 0\n");
            let mgr = manager();
            assert!(mgr.check_credentials("any-profile"));
        }

        #[test]
        #[serial]
        fn check_credentials_returns_false_on_nonzero_exit() {
            let (_guard, _tmp) = install_aws_stub("#!/bin/sh\nexit 1\n");
            let mgr = manager();
            assert!(!mgr.check_credentials("any-profile"));
        }

        #[test]
        #[serial]
        fn check_credentials_returns_false_when_aws_missing() {
            // PATH points only at an empty tempdir, so any attempt to exec
            // `aws` fails with ENOENT.
            let tmp = tempfile::TempDir::new().unwrap();
            let previous = env::var("PATH").ok();
            env::set_var("PATH", tmp.path());
            let _guard = PathGuard { previous };

            let mgr = manager();
            assert!(!mgr.check_credentials("any-profile"));
        }

        // -- sso_login --------------------------------------------------------

        #[test]
        #[serial]
        fn sso_login_succeeds_when_aws_exits_zero() {
            let (_guard, _tmp) = install_aws_stub("#!/bin/sh\nexit 0\n");
            let mgr = manager();
            assert!(mgr.sso_login("any-profile").unwrap());
        }

        #[test]
        #[serial]
        fn sso_login_reports_failure_on_nonzero_exit() {
            let (_guard, _tmp) = install_aws_stub("#!/bin/sh\nexit 7\n");
            let mgr = manager();
            assert!(!mgr.sso_login("any-profile").unwrap());
        }
    }
}
