DESKTOP_DIR := apps/desktop
NPM := npm --prefix $(DESKTOP_DIR)
NODE_MODULES_LOCK := $(DESKTOP_DIR)/node_modules/.package-lock.json
BINARY := target/release/sessionsmith

export PATH := $(HOME)/.cargo/bin:$(PATH)

.DEFAULT_GOAL := help

.PHONY: app browser binary deps help

app: deps ## Launch the desktop app from the current repository source
	$(NPM) run tauri -- dev

browser: deps ## Open the desktop frontend in the default browser
	$(NPM) run dev -- --open

binary: ## Compile only the optimized SessionSmith command-line binary
	cargo build --release
	@printf '\nBuilt %s\n' "$(BINARY)"

deps: $(NODE_MODULES_LOCK) ## Install frontend dependencies when the lockfile changes

$(NODE_MODULES_LOCK): $(DESKTOP_DIR)/package.json $(DESKTOP_DIR)/package-lock.json
	$(NPM) ci

help: ## Show available commands
	@printf 'SessionSmith desktop commands\n\n'
	@awk 'BEGIN {FS = ":.*## "} /^[a-zA-Z0-9_-]+:.*## / {printf "  %-12s %s\n", $$1, $$2}' $(lastword $(MAKEFILE_LIST))
	@printf '\nExamples:\n  make app\n  make browser\n  make binary\n'
