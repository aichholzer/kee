# Fish completion for kee

# Commands
complete -c kee -n "__fish_use_subcommand" -a "add" -d "Add a new AWS profile"
complete -c kee -n "__fish_use_subcommand" -a "use" -d "Use a profile (interactive picker if no name given)"
complete -c kee -n "__fish_use_subcommand" -a "ls" -d "List all configured profiles"
complete -c kee -n "__fish_use_subcommand" -a "current" -d "Show current active profile"
complete -c kee -n "__fish_use_subcommand" -a "rm" -d "Remove a profile (interactive picker if no name given)"
complete -c kee -n "__fish_use_subcommand" -a "set" -d "Update profile settings"
complete -c kee -n "__fish_use_subcommand" -a "run" -d "Run a command with a profile (no sub-shell)"
complete -c kee -n "__fish_use_subcommand" -a "aws" -d "Run an AWS CLI command with a profile"
complete -c kee -n "__fish_use_subcommand" -a "console" -d "Open the AWS Management Console for a profile"

# Profile names for use, rm, set, run, aws and console commands
complete -c kee -n "__fish_seen_subcommand_from use rm set run aws console" -a "(kee ls --names 2>/dev/null)"

# Flags for ls command
complete -c kee -n "__fish_seen_subcommand_from ls" -l names -d "Only show profile names"
complete -c kee -n "__fish_seen_subcommand_from ls" -l help -d "Show help information"

# Flags for set command
complete -c kee -n "__fish_seen_subcommand_from set" -l production -d "Mark as a production account"
complete -c kee -n "__fish_seen_subcommand_from set" -l no-production -d "Unmark as a production account"
