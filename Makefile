
COVERAGE_FILE := .ci/coverage-threshold.txt
COVERAGE_MIN ?= $(shell tr -d '[:space:]' < $(COVERAGE_FILE))
SANITY_TARGET_DIR ?= target/sanity
SANITY_COVERAGE_TARGET_DIR ?= target/sanity-coverage

.PHONY: fmt lint test coverage sanity install-hooks check-threshold loom-bundle

fmt:
	cargo fmt --all -- --check

lint:
	CARGO_TARGET_DIR=$(SANITY_TARGET_DIR) cargo clippy --all-targets --all-features -- -D warnings
	bash scripts/repo/check-file-sizes.sh

test:
	CARGO_TARGET_DIR=$(SANITY_TARGET_DIR) cargo test --all-targets --all-features

coverage:
	@if ! cargo tarpaulin --version >/dev/null 2>&1; then \
	  echo "cargo-tarpaulin is required. Install with: cargo install cargo-tarpaulin --locked"; \
	  exit 1; \
	fi
	mkdir -p coverage
	CARGO_TARGET_DIR=$(SANITY_COVERAGE_TARGET_DIR) cargo tarpaulin --engine llvm --all-features \
	  --workspace --timeout 120 --out Xml \
	  --output-dir coverage --fail-under "$(COVERAGE_MIN)"

sanity: fmt lint test coverage

install-hooks:
	bash scripts/repo/install-hooks.sh

check-threshold:
	bash scripts/repo/check-coverage-threshold.sh origin/main

loom-bundle:
	loom build loom/knots_sdlc --emit knots-bundle > loom/knots_sdlc/dist/bundle.json
