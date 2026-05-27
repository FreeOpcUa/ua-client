BIN := ua-tui

.PHONY: build release deb clean help

build: ## debug build of ua-tui
	cargo build --bin $(BIN)

release: ## optimized build of ua-tui
	cargo build --release --bin $(BIN)

deb: ## build a .deb of ua-tui (needs: cargo install cargo-deb)
	@command -v cargo-deb >/dev/null || { echo "cargo-deb not found: run 'cargo install cargo-deb'"; exit 1; }
	cargo deb
	@echo "built: $$(ls -1 target/debian/*.deb | tail -1)"

clean: ## remove build artifacts
	cargo clean

help: ## list available targets
	@grep -hE '^[a-z]+:.*##' $(MAKEFILE_LIST) | sed 's/:.*##/\t-/'
