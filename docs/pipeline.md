# Pipeline & Architecture

## Overview

SessionSmith processes audio through a two-phase pipeline:

```
┌─────────────────────────────────────────────────────────┐
│  Phase 1: Transcription                                 │
│                                                         │
│  audio file(s) ──► ffmpeg decode ──► whisperx/whisper   │
│                                          │              │
│                            ┌─────────────┼──────────┐   │
│                            ▼             ▼          ▼   │
│                         .txt          .srt        .vtt  │
└─────────────────────────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────┐
│  Phase 2: LLM Pipeline                                  │
│                                                         │
│  transcript.txt ──► [Pass A] Bullets                    │
│                          │                              │
│            ┌─────────────┼─────────────────────┐        │
│            ▼             ▼         ▼           ▼        │
│       [Pass B]      [Pass C]  [Pass D]    [Pass E]      │
│       dm-notes       recap    summary      story        │
│                                               │         │
│                                          [Pass F]       │
│                                           quotes        │
│                                                         │
│  summary.md ──► [Campaign Log Merge]                    │
└─────────────────────────────────────────────────────────┘
```

## Phase 1: Transcription

### Engine selection

SessionSmith checks for ASR engines in this order:

1. `.venv/bin/whisperx` — project-local virtualenv (preferred)
2. `whisperx` on `$PATH`
3. Configured `asr.binary` path (whisper-cli)

### GPU handling

- Queries NVIDIA VRAM via `nvidia-smi`
- Uses CUDA when ≥4096 MB free VRAM
- Falls back to CPU automatically (with a warning)
- On CUDA OOM during transcription, retries on CPU

### Output

For each audio file, three outputs are written:
- `<stem>.txt` — plain text transcript
- `<stem>.srt` — SubRip subtitle format with timestamps
- `<stem>.vtt` — WebVTT format

### Multi-file sessions

When multiple audio files are selected for one session (e.g. a recording
split across SD cards), SessionSmith concatenates them with ffmpeg before
transcription. Files can be ordered chronologically (auto-detected by the
LLM or manually specified).

---

## Phase 2: LLM Note Generation

### Pass architecture

The pipeline uses a **bullets-first** design:

1. **Bullets** (Pass A) — the full transcript is sent to the LLM with a
   system-aware prompt. Output: a dense chronological outline of everything
   that happened.

2. **Derived artifacts** (Passes B–F) — each artifact receives the bullet
   outline (not the raw transcript) as input. This is intentional:
   - Keeps context windows manageable
   - Each pass gets a focused, pre-filtered input
   - Quality is higher than sending raw transcript to each

### Artifact types

| ID | What it produces |
|---|---|
| `bullets` | Chronological event outline — the "source of truth" for derived passes |
| `dm-notes` | GM prep document: where you left off, active NPCs, loot, hooks, consequences, open threads |
| `recap` | Short, spoiler-safe player handout suitable for reading aloud at next session |
| `summary` | Quick-reference bullet summary (shorter than full bullets) |
| `story` | Narrative chapter in prose — reads like fantasy fiction |
| `quotes` | Memorable in-character dialogue and funny table moments |

### Prompt structure

Each LLM call includes:

```
System prompt:
  ├── Role definition (artifact-specific)
  ├── Campaign context (name, setting, GM, notes)
  ├── Player/character roster
  ├── Game system terminology (from preset)
  ├── Capture priorities (from preset)
  ├── Extra sections (from preset)
  ├── Forbidden phrases list
  └── System overrides (from campaign config)

User prompt:
  └── The transcript (for bullets) or bullet outline (for derived)
```

### Concurrency

- **Ollama (local):** Derived passes run serially. Ollama serializes
  requests to the same model anyway, so parallelism adds no benefit.
- **API backends:** When `parallel_passes = true`, derived passes run
  concurrently via tokio tasks. Each spawns its own backend client.

### Thinking models

Models like Qwen3 have an internal chain-of-thought ("thinking") mode.
By default, SessionSmith sends `think: false` to disable this, because:

- Extraction tasks don't benefit meaningfully from reasoning
- A 27B thinking model can generate 20,000+ reasoning tokens before
  producing any output — adding 30+ minutes of latency per artifact

Set `[runtime] think = true` in global config if you want reasoning
enabled (e.g. for particularly complex narrative synthesis).

When thinking is enabled, the spinner shows progress:
```
⠴ thinking · ~4820 tokens (reasoning…)
```

### Error handling

- **Per-artifact resilience:** If one artifact fails (timeout, model
  error), the others still complete. Failures show a warning.
- **Campaign log soft-fail:** If the log merge fails, a warning is shown
  with instructions to run `sessionsmith log rebuild` later.
- **Resume support:** `--resume` skips any artifact whose output file
  already exists. Safe to re-run after a partial failure.

---

## Campaign Log

The campaign log (`_campaign-log.md`) is a living document that grows
with each session. After notes are generated, the session's summary is
merged into the existing log via a dedicated LLM call.

The merge prompt instructs the model to:
- Append the new session as a dated entry
- Preserve all existing entries unchanged
- Track NPCs, locations, and ongoing threads across sessions

### Rebuilding

If the log becomes corrupted or you want to regenerate it from scratch:

```bash
sessionsmith log rebuild
```

This reads every `summary.md` in the campaign's notes directory (ordered
by file modification time) and merges them one by one into a fresh log.

---

## File flow summary

```
audio/session3.wav
  │
  ├──► output/<campaign>/transcripts/session3.txt
  ├──► output/<campaign>/transcripts/session3.srt
  └──► output/<campaign>/transcripts/session3.vtt
         │
         └──► output/<campaign>/notes/session3/
                ├── bullets.md
                ├── dm-notes.md
                ├── recap.md
                ├── summary.md
                ├── story.md
                └── quotes.md
                      │
                      └──► output/<campaign>/notes/_campaign-log.md (merged)
```

---

## Streaming & progress

All LLM calls use streaming (NDJSON for Ollama, SSE for OpenAI/Anthropic).
The spinner updates in real-time:

```
⠴ Bullets: qwen3.5:27b via ollama
⠴ streaming · ~312 tokens
```

This ensures:
- No total-request timeout kills long generations
- The user always sees that progress is being made
- Ctrl+C can interrupt at any point (ASR child process is also killed)
