# SessionSmith

**Turn raw session recordings into GM-ready notes in one command.**

SessionSmith is a local-first CLI that takes your TTRPG session audio,
transcribes it with state-of-the-art ASR, then runs a multi-pass LLM
pipeline to produce structured notes — bullet outlines, GM prep docs,
player recaps, narrative prose, and a rolling campaign log that grows
with every session.

It works with any game system. Bundled presets for D&D 5e, Pathfinder 2e,
Call of Cthulhu, Blades in the Dark, Daggerheart, and more teach the LLM
what to look for in *your* system. Or write your own in five minutes.

```
audio/session3.wav
     │
     ▼  whisperx (GPU-accelerated ASR)
output/<campaign>/transcripts/session3.txt
     │
     ▼  multi-pass LLM pipeline
output/<campaign>/notes/session3/
  ├── bullets.md      chronological event outline
  ├── dm-notes.md     where you left off · NPCs · loot · hooks · consequences
  ├── recap.md        short, spoiler-safe player handout
  ├── summary.md      quick-reference bullet summary
  ├── story.md        narrative chapter in fantasy prose
  └── quotes.md       memorable in-character & table quotes

output/<campaign>/notes/_campaign-log.md   ← auto-merged after each session
```

---

## Getting started

```bash
# Build (requires Rust 1.75+)
cargo build --release

# First-run wizard — detects your GPU, recommends models, scaffolds config
./target/release/sessionsmith init

# Drop audio files in audio/ and run the interactive flow
./target/release/sessionsmith
```

That's it. SessionSmith will ask which files to process, transcribe them,
generate all configured artifacts, and update your campaign log.

See [docs/setup.md](docs/setup.md) for detailed installation and
hardware recommendations.

---

## How it works

1. **Transcription** — whisperx (or whisper.cpp) converts audio to
   timestamped text. CUDA is used automatically when VRAM is available;
   falls back to CPU otherwise.

2. **Bullet extraction** — the full transcript is fed to the LLM with a
   system-aware prompt. The output is a dense, chronological event outline.

3. **Derived artifacts** — each remaining artifact (dm-notes, recap,
   summary, story, quotes) is generated from the bullet outline in a
   separate LLM call. This keeps each pass focused and context-efficient.

4. **Campaign log merge** — the session summary is merged into a running
   campaign log via one final LLM call, producing a living document that
   tracks the full arc of your game.

Each campaign's output lives in its own directory under `output/`, so
multiple campaigns never pollute each other.

---

## Commands

| Command | Purpose |
|---|---|
| `sessionsmith` | Interactive home screen — pick an action |
| `sessionsmith run [files...] [--all]` | Full pipeline: audio → transcript → notes |
| `sessionsmith transcribe [files...]` | Transcribe only |
| `sessionsmith notes [transcript]` | Generate notes from an existing transcript |
| `sessionsmith log show \| rebuild` | View or regenerate the campaign log |
| `sessionsmith models` | List, pull, and configure ASR/LLM models |
| `sessionsmith systems list \| show <name>` | Browse game-system presets |
| `sessionsmith doctor` | Health check: deps, hardware, backend connectivity |
| `sessionsmith init` | First-run setup wizard |

Run any command with `--help` for full flag reference.

---

## Configuration

SessionSmith uses two config files:

- **Campaign config** (`campaigns/<name>.toml`) — campaign name, players,
  game system, which artifacts to generate. One per campaign.
- **Global config** (`~/.config/sessionsmith/config.toml`) — backend
  selection, model, ASR settings, runtime options. Shared across all
  campaigns.

See [docs/configuration.md](docs/configuration.md) for the full reference.

---

## Game system presets

Presets tell the LLM what terminology, mechanics, and structure matter for
your game. Bundled:

| Preset | System |
|---|---|
| `dnd5e` | Dungeons & Dragons 5th Edition |
| `pf2e` | Pathfinder 2nd Edition |
| `coc` | Call of Cthulhu |
| `blades` | Blades in the Dark |
| `daggerheart` | Daggerheart |
| `generic` | System-agnostic (any RPG) |
| `wordsmith` | Wordsmith (narrative dice) |

Custom presets are a single TOML file. See [docs/presets.md](docs/presets.md).

---

## Requirements

| Dependency | Role |
|---|---|
| Rust 1.75+ | Build the binary |
| `ffmpeg` + `ffprobe` | Audio decoding & duration detection |
| `whisperx` **or** `whisper-cli` | Speech-to-text engine |
| An LLM backend | Ollama (local), OpenAI-compatible, or Anthropic |

A CUDA-capable GPU with ≥8 GB VRAM is strongly recommended for both ASR
and local LLM inference. SessionSmith works on CPU but will be
significantly slower.

Run `sessionsmith doctor` to verify your setup.

---

## Documentation

| Document | Contents |
|---|---|
| [docs/setup.md](docs/setup.md) | Installation, hardware guidance, first run |
| [docs/configuration.md](docs/configuration.md) | Full config file reference |
| [docs/presets.md](docs/presets.md) | Writing and customizing game-system presets |
| [docs/pipeline.md](docs/pipeline.md) | Architecture, artifact pipeline, prompt design |

---

## Contributing

```bash
cargo test --lib    # unit tests
cargo build         # debug build for iteration
make all            # release build + run
```

To add a game system, drop a TOML in `presets/` following the pattern in
[docs/presets.md](docs/presets.md) and register it in `src/presets.rs`.

---

## Design principles

- **Local-first.** Ollama is the default backend. Your session recordings
  and notes never leave your machine unless you choose a cloud backend.
- **Resume-safe.** `--resume` skips any artifact whose output file already
  exists. Interrupted runs pick up where they left off.
- **Campaign-isolated.** Each campaign gets its own output directory.
  Switch between games freely without cross-contamination.
- **System-aware.** Presets inject game-specific terminology, capture
  priorities, and structure into every prompt — no manual prompt engineering.
- **Fail-soft.** If one artifact fails, the others still complete. The
  campaign log merge is non-fatal. You can always rebuild later.
