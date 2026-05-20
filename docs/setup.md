# Setup & Installation

## Build from source

SessionSmith is a single static binary written in Rust.

```bash
# Install Rust (if not already present)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Clone and build
git clone <repo-url> && cd SessionSmith
cargo build --release

# Binary is at:
./target/release/sessionsmith
```

You can copy the binary anywhere on your `$PATH` or run it in-place.

---

## Dependencies

### Required

| Tool | Purpose | Install |
|---|---|---|
| `ffmpeg` | Audio decoding (any format → wav for ASR) | `sudo apt install ffmpeg` / `brew install ffmpeg` |
| `ffprobe` | Duration detection for the audio picker | Ships with ffmpeg |

### ASR engine (one of)

| Engine | Notes |
|---|---|
| **whisperx** (recommended) | GPU-accelerated, word-level timestamps, speaker-aware. Install in a project `.venv/` and SessionSmith finds it automatically. |
| whisper-cli (whisper.cpp) | CPU-friendly alternative. Lighter but no word timestamps. |

#### Installing whisperx

```bash
cd SessionSmith
python3 -m venv .venv
source .venv/bin/activate
pip install whisperx
```

SessionSmith checks `.venv/bin/whisperx` first, then `$PATH`.

### LLM backend (one of)

| Backend | When to use |
|---|---|
| **Ollama** (default) | Local inference, no API key, full privacy. Needs ≥16 GB VRAM for 27B+ models. |
| OpenAI-compatible | OpenAI, OpenRouter, LM Studio, vLLM — fast, no local GPU needed. |
| Anthropic | Claude models via the Anthropic API. |

---

## Hardware recommendations

SessionSmith auto-detects your hardware at init time and recommends
appropriate models. Here's what to expect:

| GPU VRAM | ASR model | LLM model | Notes |
|---|---|---|---|
| 24 GB (RTX 3090/4090) | large-v3-turbo | 27B–32B (e.g. qwen3.5:27b) | Best local experience |
| 16 GB (RTX 4080, etc.) | large-v3 | 14B (e.g. qwen2.5:14b) | Good quality, may need to unload ASR before LLM |
| 8 GB | medium | 7B | Serviceable; consider an API backend for LLM |
| CPU only | base or small | API backend | Transcription will be slow (~0.5× realtime) |

**Tip:** whisperx and Ollama share VRAM. SessionSmith runs ASR first and
releases VRAM before starting LLM inference. With 24 GB you can keep both
loaded simultaneously.

---

## First run

```bash
./target/release/sessionsmith init
```

The wizard will:

1. Detect your GPU and available VRAM
2. Recommend ASR and LLM models for your hardware
3. Offer to pull/download recommended models
4. Create `~/.config/sessionsmith/config.toml` (global settings)
5. Scaffold a `campaigns/<name>.toml` for your first campaign

After init, drop audio files in the `audio/` directory and run:

```bash
./target/release/sessionsmith
```

---

## Verifying your setup

```bash
./target/release/sessionsmith doctor
```

This checks:
- ffmpeg/ffprobe availability
- ASR engine (whisperx or whisper-cli) and model presence
- LLM backend connectivity and configured model
- CUDA/GPU detection

Fix anything marked ✗ before running the pipeline.

---

## Directory structure

After setup, your workspace looks like:

```
SessionSmith/
├── audio/                    ← drop session recordings here
├── campaigns/
│   └── MyGame.toml           ← campaign config
├── output/
│   └── mygame/               ← auto-created, named from campaign
│       ├── transcripts/
│       │   ├── session1.txt
│       │   └── session1.srt
│       └── notes/
│           ├── session1/
│           │   ├── bullets.md
│           │   ├── dm-notes.md
│           │   └── ...
│           └── _campaign-log.md
├── presets/                  ← game-system presets (bundled at build time)
└── target/release/sessionsmith
```

Each campaign gets its own subdirectory under `output/` so multiple
campaigns never interfere with each other.
