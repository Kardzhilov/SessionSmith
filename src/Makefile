# ─────────────────────────────────────────────────────────────
#  SessionSmith — developer targets
#  Run from project root:  make -C src <target>
#  Or from inside src/:    make <target>
# ─────────────────────────────────────────────────────────────

ROOT   := $(abspath $(dir $(abspath $(lastword $(MAKEFILE_LIST))))/..)
BINARY := $(ROOT)/target/release/sessionsmith

# Ensure cargo is on PATH even if the shell didn't source ~/.cargo/env
export PATH := $(HOME)/.cargo/bin:$(PATH)

.PHONY: all build release check test test-all clean install fmt clippy doctor wizard help

all: build

## Build in debug mode
build:
	cd $(ROOT) && cargo build

## Build optimised release binary
release:
	cd $(ROOT) && cargo build --release
	@echo "→ $(BINARY)  ($$(du -sh $(BINARY) | cut -f1))"

## Type-check without linking
check:
	cd $(ROOT) && cargo check

## Run library unit tests
test:
	cd $(ROOT) && cargo test --lib

## Run all tests (lib + integration)
test-all:
	cd $(ROOT) && cargo test

## Delete build artefacts
clean:
	cd $(ROOT) && cargo clean

## Install binary to ~/.cargo/bin
install:
	cd $(ROOT) && cargo install --path .

## Format source code
fmt:
	cd $(ROOT) && cargo fmt

## Clippy (deny warnings)
clippy:
	cd $(ROOT) && cargo clippy -- -D warnings

## Build release then run `sessionsmith doctor`
doctor: release
	$(BINARY) doctor

## Build release then run the interactive init wizard
wizard: release
	$(BINARY) init

## Print this help
help:
	@awk '/^## /{h=substr($$0,4)} /^[a-z][a-z-]*:/{if(h) printf "  \033[36m%-14s\033[0m %s\n", substr($$1,1,length($$1)-1), h; h=""}' $(lastword $(MAKEFILE_LIST))
