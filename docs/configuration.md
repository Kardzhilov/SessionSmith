# Configuration Reference

SessionSmith uses two levels of configuration:

1. **Global config** — backend, ASR, and runtime settings shared across all campaigns
2. **Campaign config** — per-campaign: name, players, game system, output preferences

---

## Global config

**Location:** `~/.config/sessionsmith/config.toml`  
**Created by:** `sessionsmith init`

```toml
[backend]
kind     = "ollama"                    # "ollama" | "openai" | "anthropic"
base_url = "http://localhost:11434"    # API endpoint
# api_key = "${OPENAI_API_KEY}"       # env var interpolation with ${VAR}
model    = "qwen3.5:27b"              # default model for all LLM calls

[asr]
model     = "large-v3-turbo"           # ASR model id
# engine  = "auto"                     # auto | local | whisper-cli | whisperx
# binary  = "/usr/local/bin/whisper-cli"  # override ASR binary path
# model_dir = "~/.cache/whisper"       # ggml model cache (local/whisper-cli)
# threads = 8                          # CPU threads (local/whisper-cli)
# device  = "cuda"                     # cuda | cpu (whisperX device override)
diarize   = false                      # speaker labels (whisperX). Off by default.
# hf_token = "${HF_TOKEN}"             # Hugging Face token for diarization models
vad       = false                      # ffmpeg silence-removal pre-pass before ASR

[runtime]
parallel_passes = false                # run derived artifacts concurrently
timeout_secs    = 1800                 # per-LLM-call timeout (30 min default)
think           = false                # allow reasoning/thinking models to use CoT
# num_ctx       = 16384                # LLM context window (else derived from hardware)
chunk           = true                 # split long transcripts into overlapping windows
chunk_overlap_chars = 1000             # overlap between transcript chunks
structured      = false                # also emit machine-readable dm-notes.json
index           = true                 # maintain SQLite search index

[paths]
audio_dir  = "audio"                    # where input recordings are read from
output_dir = "output"                   # where transcripts & notes are written
```

### `[backend]` section

| Key | Type | Default | Description |
|---|---|---|---|
| `kind` | string | `"ollama"` | Backend type: `ollama`, `openai`, or `anthropic` |
| `base_url` | string | varies | API base URL. Ollama: `http://localhost:11434`, OpenAI: `https://api.openai.com` |
| `api_key` | string | — | API key. Supports `${ENV_VAR}` expansion. Not needed for Ollama. |
| `model` | string | — | Default model ID (e.g. `qwen3.5:27b`, `gpt-4o`, `claude-sonnet-4-20250514`) |

#### Using different providers

**Ollama (local, default):**
```toml
[backend]
kind     = "ollama"
base_url = "http://localhost:11434"
model    = "qwen3.5:27b"
```

**OpenAI:**
```toml
[backend]
kind    = "openai"
api_key = "${OPENAI_API_KEY}"
model   = "gpt-4o"
```

**OpenRouter / LM Studio / vLLM (OpenAI-compatible):**
```toml
[backend]
kind     = "openai"
base_url = "https://openrouter.ai/api"
api_key  = "${OPENROUTER_API_KEY}"
model    = "meta-llama/llama-3.1-70b-instruct"
```

**Groq (OpenAI-compatible):**
```toml
[backend]
kind     = "openai"
base_url = "https://api.groq.com/openai"
api_key  = "${GROQ_API_KEY}"
model    = "llama-3.3-70b-versatile"
```

**Anthropic:**
```toml
[backend]
kind    = "anthropic"
api_key = "${ANTHROPIC_API_KEY}"
model   = "claude-sonnet-4-20250514"
```

### `[asr]` section

| Key | Type | Default | Description |
|---|---|---|---|
| `model` | string | auto-detected | ASR model id. Built-in examples: `large-v3-turbo`, `faster-large-v3-turbo`, `parakeet-v3`, `voxtral-mini`, `cohere-transcribe-03-2026`. |
| `engine` | string | `auto` | ASR engine: `local` (in-process whisper-rs), `whisper-cli`, `whisperx`, or `auto`. `auto` prefers the in-process engine when compiled in, else an external binary. |
| `binary` | path | auto-detected | Path to ASR binary. Usually not needed. |
| `model_dir` | path | platform default | Where to store downloaded ggml models (whisper-cli only) |
| `threads` | int | system cores | CPU threads for whisper-cli |
| `diarize` | bool | `false` | Speaker diarization (whisperX only). **Off by default** — current local models often confuse the GM with players, causing more harm than help. Kept as an opt-in for future, better models. |
| `hf_token` | string | — | Hugging Face token for the pyannote diarization models. Supports `${ENV_VAR}`. Pre-download the models to stay fully offline. |
| `vad` | bool | `false` | Run an ffmpeg silence-removal pre-pass before ASR to skip long gaps in the recording. |
| `device` | string | auto | Force the ASR compute device where supported: `cuda`, `vulkan`, `metal`, or `cpu`. Unset auto-detects when the selected engine supports it. |

`cohere-transcribe-03-2026` is a local GGUF model. Preparing it downloads the
Q5_K_M GGUF and fetches/builds the local `transcribe.cpp` runtime.

For Hugging Face-hosted GGML/GGUF model files, repeated model checks use remote
metadata headers and skip the download when the local file is already current.

### `[runtime]` section

| Key | Type | Default | Description |
|---|---|---|---|
| `parallel_passes` | bool | `false` | Run derived artifacts concurrently. Safe for API backends; keep false for Ollama. |
| `timeout_secs` | int | `1800` | Per-request timeout in seconds. Raise for very long sessions. |
| `think` | bool | `false` | Allow "thinking" models (Qwen3, etc.) to use chain-of-thought reasoning. Dramatically slower; usually unnecessary for extraction tasks. |
| `num_ctx` | int | derived | LLM context window (Ollama `num_ctx`). When unset, derived from the detected hardware tier. Without it, Ollama silently truncates long transcripts. |
| `chunk` | bool | `true` | Split transcripts that exceed the context budget into overlapping windows for the bullets pass, then merge (map-reduce). |
| `chunk_overlap_chars` | int | `1000` | Character overlap between consecutive transcript chunks. |
| `structured` | bool | `false` | Also emit a machine-readable `dm-notes.json` (typed NPCs/loot/quests) using the backend's structured-output mode. |
| `index` | bool | `true` | Maintain a per-campaign SQLite index (`output/<slug>/index.sqlite`) powering `sessionsmith search`. |

---

## Campaign config

**Location:** `campaigns/<name>.toml`  
**Created by:** `sessionsmith init` or manually

```toml
[campaign]
name    = "Curse of Strahd"
gm      = "Alex"
setting = "Barovia, a mist-shrouded valley ruled by the vampire Strahd."
notes   = "Party entered Death House in session 1."

[[players]]
player    = "Jordan"
character = "Ser Aldric"
ancestry  = "Human"
class     = "Paladin"

[[players]]
player    = "Sam"
character = "Whisper"
ancestry  = "Tiefling"
class     = "Rogue"

[system]
preset    = "dnd5e"
overrides = "We track inspiration as a shared pool of 3 tokens."

[outputs]
default = ["bullets", "dm-notes", "recap", "summary", "story", "quotes"]

[prompts]
# Optional per-artifact prompt overrides (full replacement of the system prompt)
# bullets = """Your custom prompt here..."""
```

### `[campaign]` section

| Key | Required | Description |
|---|---|---|
| `name` | yes | Campaign name. Also determines the output directory slug. |
| `gm` | no | GM name (injected into prompts for context) |
| `setting` | no | World/setting description |
| `notes` | no | Free-form notes injected into every prompt |

### `[[players]]` section (repeatable)

| Key | Required | Description |
|---|---|---|
| `player` | yes | Real name of the player |
| `character` | yes | Character name |
| `ancestry` | no | Race/ancestry/heritage |
| `class` | no | Class/playbook/archetype |

The player list is injected into prompts so the LLM can attribute actions
to the correct characters. Uncertain attributions are marked with `(?)`.

### `[system]` section

| Key | Required | Description |
|---|---|---|
| `preset` | yes | Bundled preset name: `dnd5e`, `pf2e`, `coc`, `blades`, `daggerheart`, `generic`, `wordsmith` |
| `overrides` | no | Free-text appended after the preset block — house rules, custom terminology, etc. |

### `[outputs]` section

| Key | Default | Description |
|---|---|---|
| `default` | all artifacts | Array of artifact IDs to generate by default |

Valid artifact IDs: `bullets`, `dm-notes`, `recap`, `summary`, `story`, `quotes`

### `[prompts]` section

Optional per-artifact prompt overrides. Each key replaces the entire system
prompt for that artifact. Useful for heavily customized output formats.

---

## Input / output directories

By default SessionSmith reads recordings from `audio/` and writes everything
under `output/` (both relative to the current directory, and created
automatically on startup). Override them in the **global** config:

```toml
[paths]
audio_dir  = "~/Recordings/ttrpg"
output_dir = "~/Documents/campaign-notes"
```

| Key | Default | Description |
|---|---|---|
| `audio_dir` | `audio` | Directory scanned for input recordings. |
| `output_dir` | `output` | Root for generated output; each campaign gets an `<output_dir>/<slug>/` subdirectory. |

Relative paths resolve against the current working directory; a leading `~`
expands to your home directory. Both directories are created on startup if
missing.

---

## CLI overrides

Most config values can be overridden per-invocation:

```bash
sessionsmith run --backend openai --model gpt-4o --asr-model medium
sessionsmith notes --artifacts bullets,recap --no-log
sessionsmith transcribe --force --language en
sessionsmith transcribe --diarize --vad          # opt-in speaker labels + silence trim
sessionsmith search "the cursed amulet"          # search indexed notes
sessionsmith record my-session                    # capture live audio into audio/
```

Priority: CLI flag > campaign config > global config > built-in default.

### Re-transcribe / enhance an existing recording

To re-transcribe a recording at a higher-quality model or a different language
(the "import & enhance" workflow), re-run transcription with `--force`:

```bash
sessionsmith transcribe audio/session3.wav --force --asr-model large-v3 --language en
```

---

## Multiple campaigns

Place multiple `.toml` files in `campaigns/`:

```
campaigns/
├── curse-of-strahd.toml
├── blades-campaign.toml
└── oneshot.toml
```

When you run SessionSmith it will show a picker if multiple campaigns
exist. The chosen campaign determines which `output/<slug>/` directory is
used for transcripts and notes.

---

## Environment variables

| Variable | Purpose |
|---|---|
| `SESSIONSMITH_CAMPAIGN` | Path to a campaign config file (skips the picker) |
| `OPENAI_API_KEY` | Referenced via `${OPENAI_API_KEY}` in config |
| `ANTHROPIC_API_KEY` | Referenced via `${ANTHROPIC_API_KEY}` in config |
| `GROQ_API_KEY` | Referenced via `${GROQ_API_KEY}` in config |
| `HF_TOKEN` | Referenced via `${HF_TOKEN}` for diarization models |

Any `${VAR_NAME}` in the `api_key` field is expanded at runtime.
