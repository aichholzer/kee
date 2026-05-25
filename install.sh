#!/bin/bash
# Kee — Installation script

set -e

BOLD_WHITE="\033[1;37m"
RESET="\033[0m"
CARGO_BIN="$HOME/.cargo/bin"

echo ""
echo -e " Installing ${BOLD_WHITE}Kee${RESET}..."

# Check prerequisites
if ! command -v cargo >/dev/null 2>&1; then
    echo ""
    echo " [X] cargo not found. Install Rust first: https://rustup.rs"
    exit 1
fi

# Build and install
cargo install --path . --quiet

# Ensure ~/.cargo/bin is in PATH for the user's shell
ensure_path() {
    local config_file="$1"
    local line="$2"

    [[ -f "$config_file" ]] || return 1

    if grep -q "\.cargo/bin" "$config_file" 2>/dev/null; then
        return 0
    fi

    echo "$line" >> "$config_file"
    echo -e " [✓] Added ~/.cargo/bin to PATH in ${BOLD_WHITE}$(basename "$config_file")${RESET}"
}

NEEDS_RELOAD=false

if [[ ":$PATH:" != *":$CARGO_BIN:"* ]]; then
    case "$SHELL" in
        */zsh)
            ensure_path "$HOME/.zshrc" 'export PATH="$HOME/.cargo/bin:$PATH"'
            NEEDS_RELOAD=true
            ;;
        */bash)
            ensure_path "$HOME/.bashrc" 'export PATH="$HOME/.cargo/bin:$PATH"' ||
            ensure_path "$HOME/.bash_profile" 'export PATH="$HOME/.cargo/bin:$PATH"'
            NEEDS_RELOAD=true
            ;;
        */fish)
            fish_config="$HOME/.config/fish/config.fish"
            mkdir -p "$(dirname "$fish_config")"
            ensure_path "$fish_config" 'set -gx PATH $HOME/.cargo/bin $PATH'
            NEEDS_RELOAD=true
            ;;
        *)
            echo " [!] Unrecognised shell: $SHELL"
            echo "     Please add ~/.cargo/bin to your PATH manually."
            ;;
    esac
fi

# Install shell auto-completion via the binary itself.
if command -v kee >/dev/null 2>&1; then
    kee completions install
elif [[ -x "$CARGO_BIN/kee" ]]; then
    "$CARGO_BIN/kee" completions install
fi

echo ""
echo -e " [✓] ${BOLD_WHITE}Kee${RESET} installed successfully!"

if command -v kee >/dev/null 2>&1; then
    echo -e "     Ready to go. Run: ${BOLD_WHITE}kee add <account>${RESET}"
elif [[ "$NEEDS_RELOAD" == true ]]; then
    case "$SHELL" in
        */zsh)  SOURCE="source ~/.zshrc" ;;
        */bash) SOURCE="source ~/.bashrc" ;;
        */fish) SOURCE="source ~/.config/fish/config.fish" ;;
        *)      SOURCE="restart your terminal" ;;
    esac
    echo -e "     Run: ${BOLD_WHITE}${SOURCE}${RESET}, then: ${BOLD_WHITE}kee add <account>${RESET}"
fi
