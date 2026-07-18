BIN := ua-tui

.PHONY: build build-release deb release update clean help

build: ## debug build of ua-tui
	cargo build --bin $(BIN)

build-release: ## optimized build of ua-tui
	cargo build --release --bin $(BIN)

deb: ## build a .deb of ua-tui (needs: cargo install cargo-deb)
	@command -v cargo-deb >/dev/null || { echo "cargo-deb not found: run 'cargo install cargo-deb'"; exit 1; }
	cargo deb
	@echo "built: $$(ls -1 target/debian/*.deb | tail -1)"

release: ## bump version (prompts; or BUMP=patch|minor|major|X.Y.Z), tag, push, cargo publish
	@set -e; \
	cargo set-version --help >/dev/null 2>&1 || { echo "cargo-edit not found: run 'cargo install cargo-edit'" >&2; exit 1; }; \
	test -z "$$(git status --porcelain)" || { echo "working tree not clean; commit or stash first" >&2; exit 1; }; \
	LEVEL="$(BUMP)"; \
	if [ -z "$$LEVEL" ]; then \
	  CUR=$$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"(.*)".*/\1/'); \
	  printf "current version %s — bump [patch/minor/major] or enter explicit version: " "$$CUR"; \
	  read LEVEL; \
	fi; \
	case "$$LEVEL" in \
	  patch|minor|major) cargo set-version --bump "$$LEVEL";; \
	  "") echo "no version given" >&2; exit 1;; \
	  *) cargo set-version "$$LEVEL";; \
	esac; \
	VERSION=$$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"(.*)".*/\1/'); \
	echo "releasing v$$VERSION"; \
	cargo publish --dry-run --allow-dirty; \
	git commit -am "release v$$VERSION"; \
	git tag -a "v$$VERSION" -m "v$$VERSION"; \
	git push origin HEAD --follow-tags; \
	cargo publish

update: ## update all dependencies in Cargo.lock to latest semver-compatible versions
	cargo update
	@echo "note: for major (semver-incompatible) bumps, run 'cargo upgrade --incompatible' (needs cargo-edit)"

clean: ## remove build artifacts
	cargo clean

help: ## list available targets
	@grep -hE '^[a-z][a-z-]*:.*##' $(MAKEFILE_LIST) | sed 's/:.*##/\t-/'
