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
use aws::{AwsManager, ProfileInfo};

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
#[command(long_about = format!("{KEE_ART}\n V. {}\n\nExamples:\n  kee                        Show current profile or this help\n  kee add myprofile          Add a new AWS profile\n  kee use                    Pick a profile interactively (starts sub-shell)\n  kee use myprofile          Use a specific profile (starts sub-shell)\n  kee aws myprofile s3 ls    Run an AWS CLI command with a profile\n  kee run myprofile -- CMD   Run any command with a profile (no sub-shell)\n  kee console                Open the AWS console (current session or picker)\n  kee console myprofile      Open the AWS console for a specific profile\n  kee status                 Show all profiles with session status and expiry\n  kee ls                     List all available profiles\n  kee current                Show current, active profile\n  kee rm                     Pick a profile to remove interactively\n  kee rm myprofile           Remove a specific profile", env!("CARGO_PKG_VERSION")))]
struct Cli {
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

struct Spinner {
    running: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl Spinner {
    fn start(message: &str) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = Arc::clone(&running);
        let message = message.to_string();

        let handle = thread::spawn(move || {
            let mut i = 0;
            while running_clone.load(Ordering::Relaxed) {
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

        Self {
            running,
            handle: Some(handle),
        }
    }

    fn stop(mut self, result: &str) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        // Clear the spinner line and print the result
        print!("\r\x1b[2K");
        let _ = io::stdout().flush();
        println!("{result}");
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        print!("\r\x1b[2K");
        let _ = io::stdout().flush();
    }
}

/// Buffer before token expiry at which we refresh, in seconds.
const REFRESH_BUFFER_SECS: i64 = 300;
/// Fallback sleep duration if expiry can't be read, in seconds.
const REFRESH_FALLBACK_SECS: u64 = 1800;

/// Background thread that keeps the SSO session fresh while a sub-shell is active.
struct SessionRefresher {
    running: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl SessionRefresher {
    fn start(aws_manager: AwsManager, profile_info: ProfileInfo) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = Arc::clone(&running);

        let handle = thread::spawn(move || {
            let mut last_ok = true;
            while running_clone.load(Ordering::Relaxed) {
                // Decide how long to sleep based on the current token expiry.
                let sleep_secs = match aws_manager.read_token_expiry(&profile_info) {
                    Some(expires_at) => {
                        let remaining =
                            (expires_at - chrono::Utc::now()).num_seconds() - REFRESH_BUFFER_SECS;
                        // If we're already within the buffer, refresh immediately.
                        remaining.max(0) as u64
                    }
                    None => REFRESH_FALLBACK_SECS,
                };

                // Sleep in short ticks so we exit promptly when signalled.
                let mut slept = 0u64;
                while slept < sleep_secs && running_clone.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_secs(1));
                    slept += 1;
                }

                if !running_clone.load(Ordering::Relaxed) {
                    break;
                }

                // Refresh. Surface state transitions to the user via stderr so a
                // silently-dying session is at least visible in the sub-shell.
                let ok = aws_manager.try_refresh_token(&profile_info);
                match (last_ok, ok) {
                    (true, false) => {
                        eprintln!(
                            "\n Kee: background session refresh failed for '{}'.\n      Run 'aws sso login --profile {}' if AWS calls start failing.",
                            profile_info.profile_name, profile_info.profile_name
                        );
                    }
                    (false, true) => {
                        eprintln!(
                            "\n Kee: background session refresh recovered for '{}'.",
                            profile_info.profile_name
                        );
                    }
                    _ => {}
                }
                last_ok = ok;
            }
        });

        Self {
            running,
            handle: Some(handle),
        }
    }
}

impl Drop for SessionRefresher {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl KeeManager {
    fn prompt_user(&self, message: &str) -> io::Result<bool> {
        print!("{message}");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        Ok(input.trim().to_lowercase() == "y")
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
        let home_dir = dirs::home_dir().ok_or_else(|| {
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
        let production = self.prompt_user(&format!(
            "\n Is {} a production account? (y/N): ",
            hlt(profile_name)
        ))?;
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
            "\n [!] Are you sure you want to remove profile '{}'? (y/N): ",
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
            if self.prompt_user(" Would you like to add now? (y/N): ")? {
                if self.add_profile(profile_name)? {
                    if self.prompt_user(&format!(
                        " Would you like to use profile '{hlt_profile}' now? (y/N): "
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

        // Always attempt a token refresh to maximise session duration
        println!();
        let spinner = Spinner::start("Refreshing session...");
        let refreshed = self.aws_manager.try_refresh_token(&profile_info)
            && self.check_credentials(&profile_name);

        if refreshed {
            spinner.stop(" [✓] Session refreshed.");
        } else if self.check_credentials(&profile_name) {
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

        // Collect profile info for parallel processing.
        let profiles: Vec<(String, ProfileInfo)> = config
            .profiles
            .iter()
            .map(|(name, info)| (name.clone(), info.clone()))
            .collect();

        let current = env::var(KEE_CURRENT_PROFILE).ok();

        // Spawn a thread per profile to check status concurrently.
        let handles: Vec<_> = profiles
            .into_iter()
            .map(|(name, info)| {
                let aws_manager = self.aws_manager.clone();
                let profile_name = info.profile_name.clone();
                thread::spawn(move || {
                    let expiry = aws_manager.read_token_expiry(&info);
                    let alias = get_account_alias(&profile_name);
                    (name, info, expiry, alias)
                })
            })
            .collect();

        println!();
        for handle in handles {
            let (name, info, expiry, alias) = handle.join().unwrap();

            let is_current = current.as_deref() == Some(&name);
            let marker = if is_current { " *" } else { "" };

            // Single status line combining health and expiry.
            let status_line = match expiry {
                Some(exp) => {
                    let remaining = exp - chrono::Utc::now();
                    if remaining.num_seconds() <= 0 {
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
                None => "\x1b[0;31m●\x1b[0m Expired".to_string(),
            };

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

        // Build the federation session payload.
        let session_token = creds.session_token.unwrap_or_default();
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

        // Try a refresh, then validate, then fall back to interactive sso login.
        let _ = self.aws_manager.try_refresh_token(&profile_info);
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
        match Command::new("aws")
            .args(["sts", "get-caller-identity", "--profile", profile_name])
            .env(AWS_CLI_AUTO_PROMPT, "off")
            .env(AWS_PAGER, "")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
        {
            Ok(status) => status.success(),
            Err(_) => false,
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

        // Show banner
        if profile_info.production {
            println!("\n \x1b[1;31m⚠️  PRODUCTION ACCOUNT\x1b[0m");
        }
        println!("\n Profile: {}", hlt(profile_name));
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

        // Keep the session refreshed in the background while the sub-shell is alive.
        // Dropped automatically when this function returns.
        let _refresher = SessionRefresher::start(self.aws_manager.clone(), profile_info.clone());

        let _ = cmd.status();

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

/// Build the Issuer string AWS shows in the federation audit log.
/// Format: {hostname}/{user}/kee, with fallbacks if either piece is missing.
fn build_issuer() -> String {
    let host = Command::new("hostname")
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout).ok()
            } else {
                None
            }
        })
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
        use|rm|set|run|aws|console)
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
complete -c kee -n "__fish_seen_subcommand_from use rm set run aws console" -f -a "(__kee_profiles)"
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

    let home = dirs::home_dir()?;
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

    let home = dirs::home_dir()
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

    let home = dirs::home_dir()
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
