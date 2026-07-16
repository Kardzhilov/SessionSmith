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

SessionSmith first checks whether `[asr] model` is a modern bridge model such as
`faster-large-v3-turbo`, `parakeet-v3`, `voxtral-mini`, or
`cohere-transcribe-03-2026`. Python-family models run through the `uv` bridge;
GGUF models run locally through `transcribe.cpp`.

For legacy Whisper models, SessionSmith checks for ASR engines in this order:

1. `.venv/bin/whisperx` — project-local virtualenv (preferred)
2. `whisperx` on `$PATH`
3. Configured `asr.binary` path (whisper-cli)

### GPU handling

- Queries NVIDIA VRAM via `nvidia-smi`
- Uses CUDA when ≥4096 MB free VRAM
- Falls back to CPU automatically (with a warning)
- On CUDA OOM during transcription, retries on CPU
- `[asr] device = "cuda" | "cpu"` overrides the auto-detection (useful on
  non-NVIDIA hardware)

### Speaker diarization (optional, off by default)

With `[asr] diarize = true` (or `--diarize`), whisperX aligns words and runs
pyannote diarization so the transcript carries speaker labels. It is **off by
default**: current local models frequently confuse the GM with players, which
tends to cause more harm than help. It is kept as an opt-in for future, better
models. A Hugging Face token (`[asr] hf_token`) is required for the pyannote
models.

### VAD pre-pass (optional)

With `[asr] vad = true` (or `--vad`), an ffmpeg silence-removal pass trims long
gaps before ASR, so transcription spends time only on speech.

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

   For transcripts that exceed the model's context budget (see `[runtime]
   num_ctx`), the bullets pass switches to **map-reduce**: the transcript is
   split into overlapping windows (`[runtime] chunk`, `chunk_overlap_chars`),
   each window is summarised independently, and the partial outlines are merged
   into one chronological outline. This prevents silent truncation of long
   sessions.

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

### Structured output & search index

- **Structured companion (`[runtime] structured = true`):** in addition to the
  prose `dm-notes.md`, a schema-constrained `dm-notes.json` is emitted (typed
  `npcs`, `loot`, `quests`, `locations`, `cliffhanger`) using the backend's
  structured-output mode (Ollama `format`, OpenAI `response_format`).
- **Search index (`[runtime] index = true`, on by default):** each generated
  artifact is recorded in a per-campaign SQLite database at
  `output/<slug>/index.sqlite`. `sessionsmith search <query>` looks across every
  session's notes.

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
