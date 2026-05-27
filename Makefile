BIN := ua-tui
BUMP ?= patch

.PHONY: build build-release deb release clean help

build: ## debug build of ua-tui
	cargo build --bin $(BIN)

build-release: ## optimized build of ua-tui
	cargo build --release --bin $(BIN)

deb: ## build a .deb of ua-tui (needs: cargo install cargo-deb)
	@command -v cargo-deb >/dev/null || { echo "cargo-deb not found: run 'cargo install cargo-deb'"; exit 1; }
	cargo deb
	@echo "built: $$(ls -1 target/debian/*.deb | tail -1)"

release: ## bump version (BUMP=patch|minor|major), tag, push, and cargo publish
	@cargo set-version --help >/dev/null 2>&1 || { echo "cargo-edit not found: run 'cargo install cargo-edit'"; exit 1; }
	@test -z "$$(git status --porcelain)" || { echo "working tree not clean; commit or stash first"; exit 1; }
	cargo set-version --bump $(BUMP)
	@VERSION=$$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"(.*)".*/\1/'); \
	echo "releasing v$$VERSION"; \
	cargo publish --dry-run --allow-dirty && \
	git commit -am "release v$$VERSION" && \
	git tag -a "v$$VERSION" -m "v$$VERSION" && \
	git push origin HEAD --follow-tags && \
	cargo publish

clean: ## remove build artifacts
	cargo clean

help: ## list available targets
	@grep -hE '^[a-z][a-z-]*:.*##' $(MAKEFILE_LIST) | sed 's/:.*##/\t-/'
