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
model     = "large-v3-turbo"           # whisperx/whisper model name
# binary  = "/usr/local/bin/whisper-cli"  # override ASR binary path
# model_dir = "~/.cache/whisper"       # ggml model cache (whisper-cli only)
# threads = 8                          # CPU threads (whisper-cli only)

[runtime]
parallel_passes = false                # run derived artifacts concurrently
timeout_secs    = 1800                 # per-LLM-call timeout (30 min default)
think           = false                # allow reasoning/thinking models to use CoT
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
| `model` | string | auto-detected | Model name. whisperx: `large-v3-turbo`, `large-v3`, `medium`, etc. |
| `binary` | path | auto-detected | Path to ASR binary. Usually not needed. |
| `model_dir` | path | platform default | Where to store downloaded ggml models (whisper-cli only) |
| `threads` | int | system cores | CPU threads for whisper-cli |

### `[runtime]` section

| Key | Type | Default | Description |
|---|---|---|---|
| `parallel_passes` | bool | `false` | Run derived artifacts concurrently. Safe for API backends; keep false for Ollama. |
| `timeout_secs` | int | `1800` | Per-request timeout in seconds. Raise for very long sessions. |
| `think` | bool | `false` | Allow "thinking" models (Qwen3, etc.) to use chain-of-thought reasoning. Dramatically slower; usually unnecessary for extraction tasks. |

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

## CLI overrides

Most config values can be overridden per-invocation:

```bash
sessionsmith run --backend openai --model gpt-4o --asr-model medium
sessionsmith notes --artifacts bullets,recap --no-log
sessionsmith transcribe --force --language en
```

Priority: CLI flag > campaign config > global config > built-in default.

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

Any `${VAR_NAME}` in the `api_key` field is expanded at runtime.
