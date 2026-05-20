# ─────────────────────────────────────────────────────────────
#  SessionSmith — launcher
#  Runs sessionsmith subcommands; builds the release binary
#  automatically when it is missing or outdated.
#
#  Developer targets (build, test, fmt, clippy…) live in src/Makefile.
#  Run them with:  make dev TARGET=<target>
# ─────────────────────────────────────────────────────────────

BINARY := target/release/sessionsmith
SOURCES := $(shell find src -name '*.rs') Cargo.toml Cargo.lock

# Ensure cargo is on PATH even if the shell didn't source ~/.cargo/env
export PATH := $(HOME)/.cargo/bin:$(PATH)

.PHONY: all run init transcribe notes log doctor systems models dev help

# Default: full interactive pipeline
all: run

# Build the release binary only when sources are newer than the binary.
$(BINARY): $(SOURCES)
	@$(MAKE) -C src release

## Start the full interactive pipeline (transcribe → notes)
run: $(BINARY)
	$(BINARY)

## Interactive campaign initialisation wizard
init: $(BINARY)
	$(BINARY) init

## Transcribe audio files in audio/
transcribe: $(BINARY)
	$(BINARY) transcribe

## Generate notes from an existing transcript
notes: $(BINARY)
	$(BINARY) notes

## Show or update the campaign log
log: $(BINARY)
	$(BINARY) log

## Check dependencies and configuration
doctor: $(BINARY)
	$(BINARY) doctor

## List available game-system presets
systems: $(BINARY)
	$(BINARY) systems list

## Manage Whisper / Ollama models
models: $(BINARY)
	$(BINARY) models list

## Delegate to src/Makefile — usage: make dev TARGET=test
dev:
	@$(MAKE) -C src $(TARGET)

## Print this help
help:
	@awk '/^## /{h=substr($$0,4)} /^[a-z][a-z-]*:/{if(h) printf "  \033[36m%-14s\033[0m %s\n", substr($$1,1,length($$1)-1), h; h=""}' Makefile
