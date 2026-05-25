.PHONY: test test-verbose test-unit test-integration build build-release install install-completions uninstall-completions clean lint format check help

# Build debug version
build:
	cargo build

# Build release version
build-release:
	cargo build --release

# Install shell completions
install-completions:
	kee completions install

# Uninstall shell completions
uninstall-completions:
	kee completions uninstall

# Install locally (includes completions)
install:
	cargo install --path . --quiet
	kee completions install

# Run all tests
test:
	cargo test

# Run tests with verbose output
test-verbose:
	cargo test -- --nocapture

# Run only unit tests (lib tests)
test-unit:
	cargo test --lib

# Run only integration tests
test-integration:
	cargo test --test integration_tests

# Run integration tests with binary testing
test-integration-full:
	cargo test --test integration_tests --features integration-tests

# Run tests with coverage (requires cargo-tarpaulin)
test-coverage:
	cargo tarpaulin --out Html --output-dir coverage

# Check code without building
check:
	cargo check

# Run linting
lint:
	cargo clippy -- -D warnings

# Format code
format:
	cargo fmt

# Check formatting
format-check:
	cargo fmt -- --check

# Run all checks (format, lint, test)
ci: format-check lint test

# Clean build artifacts
clean:
	cargo clean

# Update dependencies
update:
	cargo update

# Generate documentation
doc:
	cargo doc --open

# Install development tools
install-dev-tools:
	cargo install cargo-tarpaulin
	cargo install cargo-audit
	cargo install cargo-outdated

# Security audit
audit:
	cargo audit

# Check for outdated dependencies
outdated:
	cargo outdated

# Benchmark (if benchmarks exist)
bench:
	cargo bench

# Help
help:
	@echo "Available targets:"
	@echo "  build             - Build debug version"
	@echo "  build-release     - Build release version"
	@echo "  install           - Install locally"
	@echo "  install-completions - Install shell completions"
	@echo "  uninstall-completions - Uninstall shell completions"
	@echo "  test              - Run all tests"
	@echo "  test-verbose      - Run tests with verbose output"
	@echo "  test-unit         - Run only unit tests"
	@echo "  test-integration  - Run only integration tests"
	@echo "  test-coverage     - Run tests with coverage report"
	@echo "  check             - Check code without building"
	@echo "  lint              - Run linting"
	@echo "  format            - Format code"
	@echo "  format-check      - Check code formatting"
	@echo "  ci                - Run all CI checks"
	@echo "  clean             - Clean build artifacts"
	@echo "  doc               - Generate and open documentation"
	@echo "  audit             - Security audit"
	@echo "  outdated          - Check for outdated dependencies"
