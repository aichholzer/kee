#!/bin/bash
# Kee — Auto-completion installation
# Generates completions from the kee binary itself so they always
# reflect the current set of subcommands and flags.

set -e

BOLD_WHITE="\033[1;37m"
RESET="\033[0m"

# Function to detect current shell
detect_shell() {
    if [[ -n "$SHELL" ]]; then
        case "$SHELL" in
            */bash) echo "bash"; return 0 ;;
            */zsh)  echo "zsh";  return 0 ;;
            */fish) echo "fish"; return 0 ;;
        esac
    fi

    # Fallback: check for shell config files.
    if   [[ -f "$HOME/.zshrc" ]];                                    then echo "zsh"
    elif [[ -f "$HOME/.bashrc" || -f "$HOME/.bash_profile" ]];       then echo "bash"
    elif [[ -f "$HOME/.config/fish/config.fish" ]];                  then echo "fish"
    else echo "unknown"; return 1
    fi
}

# Need the kee binary on PATH to generate completions.
KEE_BIN=$(command -v kee || true)
if [[ -z "$KEE_BIN" ]]; then
    echo -e " [X] ${BOLD_WHITE}kee${RESET} not found on PATH; install it first."
    exit 1
fi

install_bash_completion() {
    local completion_file="$HOME/.kee/.kee_completion.bash"
    local source_line="source ~/.kee/.kee_completion.bash"

    mkdir -p "$HOME/.kee"

    if "$KEE_BIN" completions bash > "$completion_file" 2>/dev/null; then
        echo -e " [✓] Bash auto-completion installed to ${BOLD_WHITE}~/.kee/.kee_completion.bash${RESET}"

        local config_files=("$HOME/.bashrc" "$HOME/.bash_profile")
        local added=false
        for config_file in "${config_files[@]}"; do
            if [[ -f "$config_file" ]]; then
                added=true
                if ! grep -q "\.kee_completion\.bash" "$config_file"; then
                    echo "$source_line" >> "$config_file"
                fi
                break
            fi
        done

        if [[ "$added" == false ]]; then
            echo "$source_line" >> "$HOME/.bash_profile"
        fi

        return 0
    else
        echo -e " [X] Failed to generate bash auto-completion."
        return 1
    fi
}

install_zsh_completion() {
    local completion_dir="$HOME/.kee/completions"
    local completion_file="$completion_dir/_kee"
    local config_file="$HOME/.zshrc"

    mkdir -p "$completion_dir"

    if "$KEE_BIN" completions zsh > "$completion_file" 2>/dev/null; then
        local fpath_line="fpath=(~/.kee/completions \$fpath)"
        local compinit_line="autoload -Uz compinit && compinit"

        if [[ -f "$config_file" ]]; then
            if ! grep -q "\.kee/completions" "$config_file"; then
                {
                    echo ""
                    echo "# Kee completion"
                    echo "$fpath_line"
                    echo "$compinit_line"
                } >> "$config_file"
            fi
        else
            {
                echo "# Kee completion"
                echo "$fpath_line"
                echo "$compinit_line"
            } > "$config_file"
        fi

        return 0
    else
        echo -e " [X] Failed to generate zsh auto-completion."
        return 1
    fi
}

install_fish_completion() {
    local completion_dir="$HOME/.config/fish/completions"
    local completion_file="$completion_dir/kee.fish"

    mkdir -p "$completion_dir"

    if "$KEE_BIN" completions fish > "$completion_file" 2>/dev/null; then
        return 0
    else
        echo -e " [X] Failed to generate fish auto-completion."
        return 1
    fi
}

CURRENT_SHELL=$(detect_shell)
case "$CURRENT_SHELL" in
    bash)
        install_bash_completion
        echo -e " [✓] Bash completion installed successfully!"
        echo -e "     Restart your terminal or run: ${BOLD_WHITE}source ~/.bashrc${RESET}"
        ;;
    zsh)
        install_zsh_completion
        echo -e " [✓] Zsh completion installed successfully!"
        echo -e "     Restart your terminal or run: ${BOLD_WHITE}source ~/.zshrc${RESET}"
        ;;
    fish)
        install_fish_completion
        echo -e " [✓] Fish completion installed successfully!"
        echo -e "     Restart your terminal for completions to take effect"
        ;;
    *)
        echo -e " [X] Could not detect your shell type"
        echo -e "     Supported shells: ${BOLD_WHITE}bash${RESET}, ${BOLD_WHITE}zsh${RESET}, ${BOLD_WHITE}fish${RESET}"
        exit 1
        ;;
esac
