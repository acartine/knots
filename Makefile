
COVERAGE_FILE := .ci/coverage-threshold.txt
COVERAGE_MIN ?= $(shell tr -d '[:space:]' < $(COVERAGE_FILE))
ARTIFACT_MAX_AGE_HOURS ?= 24
SANITY_TARGET_DIR ?= target/sanity
SANITY_COVERAGE_TARGET_DIR ?= target/sanity-coverage

.PHONY: fmt lint test coverage sanity reap-artifacts install-hooks check-threshold loom-bundle \
	event-log-fixture event-log-inventory github-knots-v2-policy demo demo-gif

fmt:
	cargo fmt --all -- --check

lint: reap-artifacts
	npm run check-changesets
	CARGO_TARGET_DIR=$(SANITY_TARGET_DIR) cargo clippy --workspace --all-targets --all-features -- -D warnings
	bash scripts/repo/check-file-sizes.sh

test: reap-artifacts
	CARGO_TARGET_DIR=$(SANITY_TARGET_DIR) cargo test --all-targets --all-features
	npm run test-event-log-tools
	npm run test-release

coverage: reap-artifacts
	@if ! cargo tarpaulin --version >/dev/null 2>&1; then \
	  echo "cargo-tarpaulin is required. Install with: cargo install cargo-tarpaulin --locked"; \
	  exit 1; \
	fi
	mkdir -p coverage
	CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=$(SANITY_COVERAGE_TARGET_DIR) \
	  cargo tarpaulin --engine llvm --all-features \
	  --workspace --timeout 120 --out Xml \
	  --output-dir coverage --fail-under "$(COVERAGE_MIN)"

sanity: fmt lint test coverage

reap-artifacts:
	bash scripts/repo/reap-stale-artifacts.sh "$(ARTIFACT_MAX_AGE_HOURS)"

install-hooks:
	bash scripts/repo/install-hooks.sh

check-threshold:
	bash scripts/repo/check-coverage-threshold.sh origin/main

loom-bundle:
	loom build loom/work_sdlc --emit knots-bundle > loom/work_sdlc/dist/bundle.json

event-log-fixture:
	@test -n "$(EVENT_LOG_ROOT)" || { echo "EVENT_LOG_ROOT is required" >&2; exit 1; }
	node scripts/event-log-fixture.mjs --root "$(EVENT_LOG_ROOT)" $(EVENT_LOG_FIXTURE_ARGS)

event-log-inventory:
	@test -n "$(EVENT_LOG_ROOT)" || { echo "EVENT_LOG_ROOT is required" >&2; exit 1; }
	node scripts/event-log-inventory.mjs --root "$(EVENT_LOG_ROOT)" $(EVENT_LOG_INVENTORY_ARGS)

github-knots-v2-policy:
	node scripts/repo/knots-v2-rulesets.mjs verify production

demo:
	@echo 'Run: asciinema rec --overwrite -c "bash scripts/demo.sh" assets/demo.cast'
	@echo 'Then: make demo-gif  (renders assets/demo.gif for inline README playback).'

demo-gif:
	@command -v agg >/dev/null 2>&1 || { \
	  echo "agg not found; install with: brew install agg"; \
	  exit 1; \
	}
	agg assets/demo.cast assets/demo.gif
	@echo "Wrote assets/demo.gif"
