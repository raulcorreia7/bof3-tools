.DEFAULT_GOAL := help
.PHONY: help build check check-retail test format lint clean

# Release binaries are linked into bin/. CARGO_TARGET_DIR moves the cargo output, not bin/.
BIN_DIR := bin
RELEASE_DIR := $(abspath $(or $(CARGO_TARGET_DIR),target))/release
BINS := bof3 bof3-edit

help:
	@printf '%s\n' 'build  Build the release workspace and link bof3 and bof3-edit into bin/.' 'check  Check formatting, lint and tests.' 'check-retail  Run the workspace tests with the retail data required (BOF3_DISC_DIR, BOF3_DISC_IMAGE).' 'test   Run workspace tests.' 'clean  Remove cargo output and bin/ links.'
build:
	cargo build --release --workspace --locked
	@mkdir -p $(BIN_DIR)
	@for b in $(BINS); do ln -sf "$(RELEASE_DIR)/$$b" "$(BIN_DIR)/$$b"; done
test:
	cargo test --workspace --locked
format:
	cargo fmt --all $(if $(CHECK),--check)
lint:
	@command -v cargo-deny >/dev/null 2>&1 || { echo "cargo-deny missing: cargo install cargo-deny" >&2; exit 1; }
	cargo clippy --workspace --all-targets --locked -- -D warnings
	cargo deny check
check:
	$(MAKE) format CHECK=1
	$(MAKE) lint
	$(MAKE) test

# The retail tests skip when their input is unset; here a skip is a failure.
check-retail:
	@test -n "$$BOF3_DISC_DIR" -a -n "$$BOF3_DISC_IMAGE" || { echo "set BOF3_DISC_DIR (extracted disc) and BOF3_DISC_IMAGE (cue sheet)" >&2; exit 1; }
	BOF3_REQUIRE_RETAIL=1 cargo test --workspace --locked

clean:
	cargo clean
	@for b in $(BINS); do rm -f "$(BIN_DIR)/$$b"; done
	@rmdir $(BIN_DIR) 2>/dev/null || true
