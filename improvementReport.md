# SessionSmith Improvement Report

> Findings from studying [Meetily](https://github.com/Zackriya-Solutions/meetily)
> (Zackriya Solutions) — a privacy-first, local AI meeting assistant — and what
> SessionSmith can learn from it.
>
> Scope note: Meetily ships a Tauri + Next.js **GUI** desktop app. Per the
> request, GUI concerns are ignored; a TUI is an acceptable target where an
> interface is implied. This report focuses on **engine, pipeline, and data**
> ideas that transfer to SessionSmith's CLI/TUI model.

---

## 1. How Meetily works (relevant parts)

Meetily and SessionSmith are close cousins: both are **local-first, Rust,
Ollama-by-default, whisper-based** tools that turn speech into structured notes.
The differences are where the lessons live.

| Concern | Meetily | SessionSmith (today) |
|---|---|---|
| Transcription | **In-process** via `whisper-rs` / `transcribe-rs`; also NVIDIA **Parakeet** (ONNX, ~4× faster) | Shells out to `whisper-cli` **or** `whisperX` (Python venv) |
| Long transcripts | **Chunked with overlap**, model-specific chunk sizes | Whole transcript sent in one pass |
| Summary format | **Schema-constrained JSON** (Pydantic + Ollama `format`) | Free-form Markdown |
| Persistence | **SQLite** (meetings, transcripts, summaries, keys) | Flat files + one LLM-merged log |
| Audio input | **Live capture** (mic + system) with VAD + mixing | Pre-recorded files only |
| Speaker labels | Diarization (planned/PRO) | None (`--no_align`) |
| GPU | Metal/CoreML, CUDA, Vulkan via build features | `nvidia-smi` gating + CPU fallback |
| Providers | Ollama, Claude, Groq, OpenRouter, OpenAI-compatible | Ollama, OpenAI-compatible, Anthropic |

Meetily's summary engine (in its archived Python backend, still the clearest
statement of its approach) does three things worth copying:

1. **Chunk-and-map.** `process_transcript()` splits the transcript into
   overlapping windows (`step = chunk_size - overlap`) and summarizes each chunk
   independently. Chunk size is tuned per model family (e.g. 10k chars for
   phi4/llama, 30k otherwise; overlap 1k). This is the standard defence against
   context-window overflow.
2. **Schema-constrained output.** A Pydantic `SummaryResponse` model defines
   named sections (`People`, `SessionSummary`, `CriticalDeadlines`,
   `KeyItemsDecisions`, `ImmediateActionItems`, `NextSteps`, `MeetingNotes`).
   For Ollama they pass `format=SummaryResponse.model_json_schema()` so the model
   is *forced* to emit valid JSON, which the app then renders as editable blocks
   (`text` / `bullet` / `heading1` / `heading2`).
3. **Cheap transcript-quality prompt.** Every chunk prompt includes:
   *"Transcription can have spelling mistakes. correct it if required. context is
   important."* — a one-line hedge against ASR errors.

---

## 2. Prioritised improvements for SessionSmith

Ranked by value ÷ effort. Each maps a Meetily idea to a concrete SessionSmith
change with file references.

Every "current limitation" documented in Sessionsmith.md §14 has a proposal here:

| Sessionsmith.md §14 limitation | Addressed by |
|---|---|
| No speaker diarization (`--no_align`) | 2.8 (and 2.7 as enabler) |
| No transcript chunking (context overflow) | 2.4 (unblocked by 2.1) |
| No embeddings / semantic search / cross-session memory | 2.5 + 2.6 |
| SHA-256 verification disabled by default | *(not a Meetily-derived idea; noted below)* |
| `nvidia-smi`-only VRAM gating | 2.9 (via 2.7) |
| No live/streaming capture | 2.11 (+ 2.12) |

> The disabled model checksum verification is a supply-chain hardening item
> orthogonal to Meetily; it is out of scope for this report but worth a separate
> ticket (populate the `sha256` fields in [src/models.rs](src/models.rs)).

### 🔴 High value, low effort

#### 2.1 Use the context-window hint you already compute

SessionSmith's [src/hardware.rs](src/hardware.rs) produces `llm_context_hint`
(8k–32k) per hardware tier, but **nothing ever uses it.** The Ollama backend in
[src/llm/ollama.rs](src/llm/ollama.rs) never sets `num_ctx`, so Ollama falls back
to its small default context (often 4096). For a two-hour session transcript the
model silently sees only the *last* few thousand tokens — the bullets pass is
quietly truncating input and no one is told.

**Fix:** thread the context hint into `OllamaOptions` as `num_ctx`, defaulting to
the hardware tier's `llm_context_hint` and overridable via a new `[runtime]
num_ctx` key. Sketch:

```rust
// src/llm/ollama.rs
#[derive(Serialize, Default)]
struct OllamaOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    num_predict: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    num_ctx: Option<u32>,   // NEW: from ChatOptions / hardware hint
    num_gpu: i32,
}
```

This is a small change with an outsized correctness payoff, and it is the
prerequisite for trusting any single-pass result. **Caveat:** larger `num_ctx`
costs VRAM; cap it at the tier hint so it never over-commits the GPU that ASR
also shares.

#### 2.2 Add the "correct ASR mistakes" instruction to prompts

TTRPG transcripts are full of invented proper nouns that Whisper mangles. Add a
line like Meetily's to the bullets/base prompts in
[src/prompts.rs](src/prompts.rs): *"The transcript is machine-generated and may
contain spelling or homophone errors, especially for names and places — infer the
intended word from context."* Near-zero cost, meaningful quality gain, and it
composes with the existing preset terminology block.

#### 2.3 Broaden provider docs (Groq / OpenRouter already work)

Meetily lists Groq and OpenRouter as first-class. SessionSmith's
OpenAI-compatible backend ([src/llm/openai.rs](src/llm/openai.rs)) already
supports both via `base_url` — this is a **documentation** gap, not a code gap.
Add ready-to-paste `[backend]` snippets for Groq and OpenRouter to
[docs/configuration.md](docs/configuration.md).

### 🟠 High value, medium effort

#### 2.4 Chunk long transcripts (map-reduce for the bullets pass)

This is the single most impactful architectural gap.
[docs/pipeline.md](docs/pipeline.md) and [src/pipeline.rs](src/pipeline.rs) send
the **entire transcript** to the bullets pass. Beyond the context limit the tail
is lost (see 2.1). Adopt Meetily's approach:

1. Split the transcript into overlapping windows sized to fit `num_ctx` (reserve
   room for the system prompt and output).
2. Run the bullets prompt over each window (**map**).
3. Concatenate/de-duplicate the per-window bullets, preserving chronology
   (**reduce**) — either by simple stitching or a final "merge & dedupe" LLM pass.
4. Feed the merged bullets into the existing derived passes unchanged.

Because SessionSmith is already **bullets-first**, chunking only touches Pass A;
the derived artifacts inherit the benefit for free. This makes multi-hour
sessions reliable instead of silently degraded. **Caveat:** naive chunking can
split a scene mid-beat; the overlap window (and, if needed, a light reduce pass)
mitigates this. Prefer splitting on transcript segment/timestamp boundaries
(available once 2.7 gives programmatic segments) rather than raw character
offsets like Meetily's `text[i:i+chunk_size]`.

#### 2.5 Optional schema-constrained artifacts (Ollama `format` / JSON mode)

Meetily forces valid JSON via Ollama's `format` field. SessionSmith could emit a
machine-readable companion to the prose artifacts — e.g. a structured
`dm-notes.json` with typed `npcs[]`, `loot[]`, `quests[]`, `locations[]`,
`cliffhanger`. Benefits:

- Deterministic, parseable output (no regex-scraping Markdown later).
- Enables cross-session features (2.6): a real NPC/quest index.
- The existing `forbidden_phrases` and formatting rules become largely
  unnecessary for the structured path.

Implementation: add an optional `format` (JSON schema) field to `ChatReq` in
[src/llm/ollama.rs](src/llm/ollama.rs) and a schema in
[src/prompts.rs](src/prompts.rs). Keep prose artifacts as the default; make JSON
opt-in per artifact so nothing regresses. **Caveat:** Ollama structured outputs
require a recent Ollama version and cooperative models; OpenAI/Anthropic use
different JSON-mode mechanisms, so gate the feature per backend and fall back to
prose parsing when unsupported.

#### 2.6 A local index/DB for cross-session memory and search

Meetily stores everything in **SQLite** and can search across meetings.
SessionSmith's only cross-session memory is `_campaign-log.md`, rebuilt by
repeated LLM summarization — lossy and unsearchable. A lightweight
`output/<slug>/index.sqlite` (via `rusqlite`) recording sessions, artifacts, and
(if 2.5 lands) structured NPCs/loot/quests would enable:

- `sessionsmith search "the cursed amulet"` across all sessions.
- A queryable NPC/quest tracker that doesn't drift as the log is re-summarized.
- Faster `log rebuild` from structured rows instead of re-reading every file.

This keeps the local-first ethos; SQLite is a single file and needs no server.

### 🟡 High value, higher effort

#### 2.7 In-process transcription with `whisper-rs` (drop the subprocess)

SessionSmith's biggest operational fragility is [src/transcribe.rs](src/transcribe.rs):
it resolves and shells out to `whisper-cli` **or** a Python `whisperX` venv,
parses stderr, tracks PIDs, and juggles interpreter shebangs. Meetily instead
links **`whisper-rs`** (and `transcribe-rs`) directly. Moving to `whisper-rs`
would:

- Remove the Python/venv dependency and the shebang/PYTHONPATH workarounds.
- Give programmatic access to segments/timestamps (feeds diarization and
  chunking) instead of scraping `.txt`/`.srt`.
- Simplify Ctrl-C handling (no external PID to `kill -TERM`).
- Unlock cross-platform GPU via Cargo features (see 2.9).

This is a larger change; keep the subprocess path as a fallback during migration.

#### 2.8 Speaker diarization — high leverage for TTRPG specifically

SessionSmith explicitly runs whisperX with `--no_align` and no diarization, so
transcripts are one undifferentiated stream and the LLM must *guess* who spoke
(the prompts even add "(?)" markers and a roster-stripping hallucination guard).
For a table of 4–6 players this is the core accuracy problem. Options:

- Enable whisperX diarization (pyannote) when a HF token is present, or
- Add a diarization step if migrating to `whisper-rs`.

Even coarse "Speaker 0/1/2" labels would dramatically improve the bullets pass's
player-vs-character attribution, let recaps quote the right person, and allow the
`(?)` attribution markers and roster-stripping guard in
[src/prompts.rs](src/prompts.rs) to be relaxed. This directly attacks the
limitation called out in Sessionsmith.md §14. **Caveat:** pyannote diarization
pulls models from Hugging Face behind a gated token — pre-download them and cache
locally so the "nothing leaves your machine" guarantee in README/§1 still holds.

#### 2.9 Cross-platform GPU acceleration

Meetily supports Metal/CoreML (macOS), CUDA, and Vulkan (AMD/Intel) selected at
build time. SessionSmith gates ASR on `nvidia-smi` only ([src/transcribe.rs](src/transcribe.rs),
[src/hardware.rs](src/hardware.rs)), so Apple Silicon and AMD users mostly fall
back to CPU. Adopting `whisper-rs` (2.7) with Cargo GPU features would give Metal
and Vulkan paths for free and make the tool genuinely fast on non-NVIDIA
hardware.

### 🟢 Nice to have

#### 2.10 "Import & Enhance" — re-transcribe with a different model/language

Meetily lets users re-run transcription with a different model or language.
SessionSmith has `--force` but no first-class "re-transcribe this file at
`large-v3` in Spanish" flow, nor a way to compare model outputs. A small
`sessionsmith transcribe --re <stem> --asr-model large-v3` UX would help users
upgrade old transcripts.

#### 2.11 Live capture at the table (optional TUI mode)

Meetily's headline feature is real-time capture (mic + system audio, VAD,
mixing via `cpal`). SessionSmith is strictly offline-file-based. A `sessionsmith
record` command using `cpal` to capture the session live (with VAD to trim
silence) would let GMs run it *during* play and get notes moments after the
session ends — a natural TUI-only feature that needs no GUI.

#### 2.12 VAD preprocessing to cut ASR time

Meetily reports VAD reduces Whisper load ~70% by only transcribing speech.
SessionSmith transcribes whole files. A VAD pre-pass (silero/webrtc, or ffmpeg
silence-removal) before ASR would speed up long, gap-heavy recordings.

---

## 3. What SessionSmith already does *better*

Worth stating so improvements don't regress current strengths:

- **Campaign isolation** and **game-system presets** — Meetily has no equivalent
  domain-specialization layer; SessionSmith's preset system is a genuine edge.
- **Multi-artifact pipeline** (bullets → dm-notes/recap/summary/story/quotes +
  rolling log) is richer and more purpose-built than Meetily's single summary.
- **Fail-soft + resume** semantics are explicit and well-tested.
- **Zero-GUI, single-binary CLI** is lighter to deploy than a Tauri app.
- **Deliberate hallucination guards** (roster stripping, recap "say so if no
  gameplay") are more careful than Meetily's generic prompts.

---

## 4. Suggested sequencing

1. **2.1 `num_ctx`** + **2.2 ASR-correction prompt** + **2.3 provider docs** —
   a single small PR; immediate correctness/quality wins.
2. **2.4 chunking** — build on 2.1; makes long sessions trustworthy.
3. **2.6 SQLite index** and **2.5 structured artifacts** — pair them; the DB is
   far more useful with typed data.
4. **2.7 `whisper-rs`** migration — unlocks **2.8 diarization** and **2.9 GPU**.
5. **2.10–2.12** — opportunistic once the engine is in-process.

The theme: **make the existing single-pass pipeline correct first (2.1, 2.4),
then make its memory durable (2.5/2.6), then modernise the transcription engine
(2.7) to unlock accuracy (2.8) and portability (2.9).**
