.DEFAULT_GOAL := help
.PHONY: help build check test format lint clean
help:
	@printf '%s\n' 'build  Build the release workspace.' 'check  Check formatting, lint and tests.' 'test   Run workspace tests.'
build:
	cargo build --release --workspace --locked
test:
	cargo test --workspace --locked
format:
	cargo fmt --all $(if $(CHECK),--check)
lint:
	cargo clippy --workspace --all-targets --locked -- -D warnings
	cargo deny check
check:
	$(MAKE) format CHECK=1
	$(MAKE) lint
	$(MAKE) test

clean:
	cargo clean
