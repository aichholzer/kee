#!/bin/bash
# Setup script for Git pre-commit hooks

set -e

# ANSI color codes
WHITE='\033[1;37m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RESET='\033[0m'

echo ""
echo -e " ${WHITE}Setting up Git pre-commit hooks...${RESET}"
echo ""

# Check if we're in a Git repository
if [ ! -d ".git" ]; then
    echo -e " ${YELLOW}Error: Not in a Git repository. Please run this from the project root.${RESET}"
    exit 1
fi

# Check if pre-commit hook already exists
if [ -f ".git/hooks/pre-commit" ]; then
    echo -e " ${YELLOW}Pre-commit hook already exists.${RESET}"
    read -p " Do you want to overwrite it? (y/N): " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        echo ""
        echo -e " ${YELLOW}Setup cancelled.${RESET}"
        exit 0
    fi
fi

# Make sure the hooks directory exists
mkdir -p .git/hooks

# Copy the hook file and make sure it's executable
cp ./utilities/pre-commit .git/hooks/pre-commit
chmod u+x .git/hooks/pre-commit

echo ""
echo -e " ${WHITE}Pre-commit hook setup complete!${RESET}"
echo
echo -e "${YELLOW} The hook will now run these checks before each commit:${RESET}"
echo -e " - ${WHITE}cargo fmt -- --check${RESET} (code formatting)"
echo -e " - ${WHITE}cargo clippy -- -D warnings${RESET} (linting)"
echo -e " - ${WHITE}cargo check${RESET} (compilation)"
echo
echo -e " To test the hook manually, run: ${WHITE}./.git/hooks/pre-commit${RESET}"
echo -e " To skip the hook for a commit, use: ${WHITE}git commit --no-verify${RESET} ${YELLOW}— (Not recommended)${RESET}"
