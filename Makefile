.PHONY: build build-release install install-completions uninstall-completions \
	test test-verbose test-unit test-cli test-coverage \
	check lint format format-check ci clean update doc audit help

# --- Build --------------------------------------------------------------------

# Build debug version
build:
	cargo build

# Build release version
build-release:
	cargo build --release

# --- Install ------------------------------------------------------------------

# Install locally (includes completions)
install:
	cargo install --path . --quiet
	kee completions install

# Install shell completions for the current shell
install-completions:
	kee completions install

# Uninstall shell completions for the current shell
uninstall-completions:
	kee completions uninstall

# --- Test ---------------------------------------------------------------------

# Run all tests (lib, in-binary, integration)
test:
	cargo test --all-targets

# Run tests with verbose output (test stdout/stderr is captured by default)
test-verbose:
	cargo test --all-targets -- --nocapture

# Run only library tests
test-unit:
	cargo test --lib

# Run only the CLI integration tests
test-cli:
	cargo test --test cli_tests

# Run tests with coverage report (Linux only; requires cargo-tarpaulin)
test-coverage:
	cargo tarpaulin --out Html --output-dir coverage

# --- Quality ------------------------------------------------------------------

# Check code without building
check:
	cargo check

# Run clippy with warnings as errors (matches CI)
lint:
	cargo clippy --all-targets -- -D warnings

# Format code in place
format:
	cargo fmt

# Check formatting without modifying (matches CI)
format-check:
	cargo fmt -- --check

# Run all CI checks (format, lint, test)
ci: format-check lint test

# Security audit (requires cargo-audit)
audit:
	cargo audit

# --- House keeping ------------------------------------------------------------

# Clean build artifacts
clean:
	cargo clean

# Update Cargo.lock to latest compatible versions
update:
	cargo update

# Generate API documentation and open in browser
doc:
	cargo doc --open

# --- Help ---------------------------------------------------------------------

help:
	@echo "Available targets:"
	@echo ""
	@echo " Build:"
	@echo "   build                 Build debug version"
	@echo "   build-release         Build release version"
	@echo ""
	@echo " Install:"
	@echo "   install               Install locally (binary + completions)"
	@echo "   install-completions   Install shell completions for current shell"
	@echo "   uninstall-completions Remove shell completions"
	@echo ""
	@echo " Test:"
	@echo "   test                  Run all tests"
	@echo "   test-verbose          Run all tests with --nocapture"
	@echo "   test-unit             Library tests only"
	@echo "   test-cli              CLI integration tests only"
	@echo "   test-coverage         Tarpaulin HTML coverage report (Linux only)"
	@echo ""
	@echo " Quality:"
	@echo "   check                 cargo check"
	@echo "   lint                  cargo clippy with warnings as errors"
	@echo "   format                Format code in place"
	@echo "   format-check          Check formatting (read-only)"
	@echo "   ci                    Run format-check + lint + test"
	@echo "   audit                 Security audit (requires cargo-audit)"
	@echo ""
	@echo " House keeping:"
	@echo "   clean                 Remove build artifacts"
	@echo "   update                Update Cargo.lock"
	@echo "   doc                   Build and open API docs"
