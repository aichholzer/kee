# Fish completion for kee

# Commands
complete -c kee -n "__fish_use_subcommand" -a "add" -d "Add a new AWS account"
complete -c kee -n "__fish_use_subcommand" -a "use" -d "Use an account"
complete -c kee -n "__fish_use_subcommand" -a "ls" -d "List all configured accounts"
complete -c kee -n "__fish_use_subcommand" -a "current" -d "Show current active account"
complete -c kee -n "__fish_use_subcommand" -a "rm" -d "Remove an account"
complete -c kee -n "__fish_use_subcommand" -a "set" -d "Update profile settings"

# Account names for use, rm, and set commands
complete -c kee -n "__fish_seen_subcommand_from use rm set" -a "(kee ls --names 2>/dev/null)"

# Flags for ls command
complete -c kee -n "__fish_seen_subcommand_from ls" -l names -d "Only show account names"
complete -c kee -n "__fish_seen_subcommand_from ls" -l help -d "Show help information"

# Flags for set command
complete -c kee -n "__fish_seen_subcommand_from set" -l production -d "Mark as a production account"
complete -c kee -n "__fish_seen_subcommand_from set" -l no-production -d "Unmark as a production account"
