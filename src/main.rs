use clap::{Parser, Subcommand};
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
#[command(long_about = format!("{KEE_ART}\n V. {}\n\nExamples:\n  kee add myprofile          Add a new AWS profile\n  kee use myprofile          Use an available profile (starts sub-shell)\n  kee ls                     List all available profiles\n  kee current                Show current, active profile\n  kee rm myprofile           Remove a profile configuration", env!("CARGO_PKG_VERSION")))]
struct Cli {
    #[command(subcommand)]
    command: Commands,
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
        #[arg(value_name = "PROFILE_NAME", help = "Name of the AWS profile to use")]
        profile_name: String,
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
            help = "Name of the AWS profile to remove"
        )]
        profile_name: String,
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
                print!("\r {} {}", SPINNER_FRAMES[i % SPINNER_FRAMES.len()], message);
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

impl KeeManager {
    fn prompt_user(&self, message: &str) -> io::Result<bool> {
        print!("{message}");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        Ok(input.trim().to_lowercase() == "y")
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

        let profile_info = config.profiles.get(profile_name).unwrap();
        let profile_name = &profile_info.profile_name;

        // Check credentials
        println!();
        let spinner = Spinner::start("Validating session...");
        let credentials_valid = self.check_credentials(profile_name);
        if credentials_valid {
            spinner.stop(" [✓] Session is valid.");
        } else {
            spinner.stop(" [!] Your session has expired. Refreshing...");

            let spinner = Spinner::start("Refreshing token...");
            if self.aws_manager.try_refresh_token(profile_info)
                && self.check_credentials(profile_name)
            {
                spinner.stop(" [✓] Session refreshed.");
            } else {
                spinner.stop(" [!] Could not refresh the session. Opening SSO login...");
                if !self.sso_login(profile_name)? {
                    println!(
                        " [X] Failed to authenticate. Please run {} manually.",
                        hlt("aws sso login")
                    );
                    return Ok(false);
                }
            }
        }

        // Update current profile
        config.current_profile = Some(profile_name.to_string());
        self.save_config(&config)?;

        // Start subshell
        self.start_subshell(profile_name)?;

        // Clear current profile when subshell exits
        config.current_profile = None;
        self.save_config(&config)?;

        Ok(true)
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

    fn start_subshell(&self, profile_name: &str) -> io::Result<()> {
        // Get current shell
        let shell = if cfg!(windows) {
            env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string())
        } else {
            env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string())
        };

        // Show banner
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

        let _ = cmd.status();

        println!("\n {} — Session ended.", hlt(profile_name));
        Ok(())
    }
}

fn main() -> io::Result<()> {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => {
            // Check if it's a missing argument error and customize the message
            if err.kind() == clap::error::ErrorKind::MissingRequiredArgument {
                let error_msg = err.to_string();
                if error_msg.contains("<PROFILE_NAME>") {
                    if error_msg.contains("kee use") {
                        eprintln!("\n [X] Please specify a profile to use");
                        eprintln!(" Usage: {}", hlt("kee use PROFILE_NAME"));
                        eprintln!("\n Run {} to see your available profiles", hlt("kee ls"));
                        std::process::exit(2);
                    } else if error_msg.contains("kee add") {
                        eprintln!("\n [X] Please specify a name for the new profile");
                        eprintln!(" Usage: {}", hlt("kee add PROFILE_NAME"));
                        std::process::exit(2);
                    } else if error_msg.contains("kee rm") {
                        eprintln!("\n [X] Please specify the profile to remove");
                        eprintln!(" Usage: {}", hlt("kee rm PROFILE_NAME"));
                        eprintln!("\n Run {} to see your available profiles", hlt("kee ls"));
                        std::process::exit(2);
                    }
                }
            }
            // For all other errors, use clap's default handling
            err.exit();
        }
    };

    let kee = KeeManager::new()?;

    match cli.command {
        Commands::Add { profile_name } => {
            kee.add_profile(&profile_name)?;
        }
        Commands::Use { profile_name } => {
            kee.use_profile(&profile_name)?;
        }
        Commands::Ls { names } => {
            kee.list_profiles(names);
        }
        Commands::Current => {
            kee.current_profile();
        }
        Commands::Rm { profile_name } => {
            kee.remove_profile(&profile_name)?;
        }
    }

    Ok(())
}
