# Setup & Installation

## Build from source

SessionSmith is a single self-contained binary written in Rust. By default it
builds an **in-process transcription engine** (whisper.cpp via `whisper-rs`), so
no external speech-to-text tool is required at runtime.

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

### Build prerequisites

The default build compiles whisper.cpp from source, which needs a small native
toolchain **at build time** (not at runtime — the produced binary is
self-contained):

| Tool | Purpose | Install (Debian/Ubuntu/Mint) |
|---|---|---|
| C/C++ compiler | Compile whisper.cpp | `sudo apt install build-essential` |
| `cmake` | whisper.cpp build system | `sudo apt install cmake` |
| `clang` + `libclang` | `bindgen` header parsing | `sudo apt install clang libclang-dev` |

On macOS these come with the Xcode command-line tools (`xcode-tools install`)
plus `brew install cmake`.

If you would rather **not** build the in-process engine (and instead use an
external `whisper-cli`/`whisperx`), build without default features:

```bash
cargo build --release --no-default-features
```

### GPU acceleration (build-time)

The in-process engine runs on CPU by default. To build with GPU support, enable
the matching feature (each needs its SDK/driver installed):

```bash
cargo build --release --features cuda     # NVIDIA (CUDA toolkit)
cargo build --release --features vulkan   # AMD/Intel/NVIDIA (Vulkan SDK)
cargo build --release --features metal    # Apple Silicon (macOS)
```

`sessionsmith doctor` reports which backend the binary was built with.

---

## Dependencies

### Required at runtime

| Tool | Purpose | Install |
|---|---|---|
| `ffmpeg` | Audio decoding (any format → 16 kHz mono PCM for ASR) | `sudo apt install ffmpeg` / `brew install ffmpeg` |
| `ffprobe` | Duration detection for the audio picker | Ships with ffmpeg |

### ASR engine

| Engine | Notes |
|---|---|
| **in-process (whisper-rs)** — default | Built into the binary; no external tool. Uses the same ggml models as whisper.cpp (auto-downloaded). GPU via build features above. |
| whisper-cli (whisper.cpp) | External binary; used when the build excludes `local-whisper`, or when `[asr] engine = "whisper-cli"`. |
| **whisperx** | Required for **speaker diarization**. Install in a project `.venv/` and SessionSmith finds it automatically. Set `[asr] engine = "whisperx"` or `diarize = true`. |

Select the engine explicitly with `[asr] engine` (`local` / `whisper-cli` /
`whisperx`); the default (`auto`) prefers the in-process engine when compiled in.

#### Installing whisperx (only needed for diarization)

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
