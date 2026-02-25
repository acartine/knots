
COVERAGE_FILE := .ci/coverage-threshold.txt
COVERAGE_MIN ?= $(shell tr -d '[:space:]' < $(COVERAGE_FILE))

.PHONY: fmt lint test coverage sanity install-hooks check-threshold

fmt:
	cargo fmt --all -- --check

lint:
	cargo clippy --all-targets --all-features -- -D warnings

test:
	cargo test --all-targets --all-features

coverage:
	@if ! cargo tarpaulin --version >/dev/null 2>&1; then \
	  echo "cargo-tarpaulin is required. Install with: cargo install cargo-tarpaulin --locked"; \
	  exit 1; \
	fi
	mkdir -p coverage
	cargo tarpaulin --all-features --workspace --timeout 120 --out Xml \
	  --output-dir coverage --fail-under "$(COVERAGE_MIN)"

sanity: fmt lint test coverage

install-hooks:
	bash scripts/repo/install-hooks.sh

check-threshold:
	bash scripts/repo/check-coverage-threshold.sh origin/main
