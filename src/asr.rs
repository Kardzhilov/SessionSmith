//! ASR (transcription) model catalog and engine routing.
//!
//! SessionSmith supports several transcription engines. Whisper `ggml` models
//! run in-process via `whisper-rs` (or the `whisper-cli` binary). The newer,
//! more accurate models (NVIDIA Parakeet / Canary, Mistral Voxtral, and the
//! CTranslate2 `faster-whisper` builds) run through a small Python bridge that
//! is executed with [`uv`](https://docs.astral.sh/uv/), so their dependencies
//! and weights are fetched lazily on first use — no manual `pip install`.
//!
//! The active transcription model is selected purely by its `id` (the
//! `[asr] model` config value or `--asr-model` flag); the catalog maps that id
//! to the engine that should run it.

/// The runtime that backs a given ASR model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsrEngine {
    /// Whisper `ggml` model, run in-process (`whisper-rs`) or via `whisper-cli`.
    WhisperCpp,
    /// CTranslate2 `faster-whisper` (Python bridge, CPU or CUDA).
    FasterWhisper,
    /// NVIDIA NeMo FastConformer-TDT `parakeet` models (Python bridge, CUDA).
    Parakeet,
    /// NVIDIA NeMo `canary-qwen` speech LLM (Python bridge, CUDA).
    CanaryQwen,
    /// Mistral Voxtral audio LLM (Python bridge, CUDA).
    Voxtral,
}

impl AsrEngine {
    /// Whether this engine runs through the `uv` Python bridge.
    pub fn is_bridge(&self) -> bool {
        !matches!(self, AsrEngine::WhisperCpp)
    }

    /// Short human label for the model manager / doctor.
    pub fn label(&self) -> &'static str {
        match self {
            AsrEngine::WhisperCpp => "whisper.cpp",
            AsrEngine::FasterWhisper => "faster-whisper",
            AsrEngine::Parakeet => "NeMo Parakeet",
            AsrEngine::CanaryQwen => "NeMo Canary-Qwen",
            AsrEngine::Voxtral => "Voxtral",
        }
    }
}

/// A selectable transcription model.
pub struct AsrModelSpec {
    /// Stable id used in config / CLI (`[asr] model = "..."`).
    pub id: &'static str,
    /// Display name for menus.
    pub display: &'static str,
    /// The engine that runs it.
    pub engine: AsrEngine,
    /// Backend model reference: a whisper size (`ggml`), a `faster-whisper`
    /// name, or a Hugging Face / NeMo model id for the bridge engines.
    pub model_ref: &'static str,
    /// Approximate download size in bytes (weights), for the manager.
    pub size: u64,
    /// Approximate public release date (`YYYY-MM`).
    pub released: &'static str,
    /// Language coverage, short form.
    pub langs: &'static str,
    /// License, short form.
    pub license: &'static str,
    /// One-line note shown in the manager.
    pub note: &'static str,
}

/// Full catalog of transcription models, best-first within each engine tier.
///
/// The whisper `ggml` entries mirror [`crate::models::WHISPER_MODELS`] (which
/// remains the source of truth for downloads); the bridge entries are the
/// modern high-accuracy models researched in mid-2026.
pub const ASR_CATALOG: &[AsrModelSpec] = &[
    // --- whisper.cpp (ggml, in-process, CPU/GPU) -------------------------
    AsrModelSpec {
        id: "large-v3-turbo",
        display: "Whisper large-v3-turbo",
        engine: AsrEngine::WhisperCpp,
        model_ref: "large-v3-turbo",
        size: 1_620_000_000,
        released: "2024-10",
        langs: "99 langs",
        license: "MIT",
        note: "fast multilingual default; runs on CPU or GPU",
    },
    AsrModelSpec {
        id: "large-v3",
        display: "Whisper large-v3",
        engine: AsrEngine::WhisperCpp,
        model_ref: "large-v3",
        size: 3_100_000_000,
        released: "2023-11",
        langs: "99 langs",
        license: "MIT",
        note: "most accurate whisper; slower than turbo",
    },
    AsrModelSpec {
        id: "medium",
        display: "Whisper medium",
        engine: AsrEngine::WhisperCpp,
        model_ref: "medium",
        size: 1_530_000_000,
        released: "2022-09",
        langs: "99 langs",
        license: "MIT",
        note: "lighter; good on CPU-only machines",
    },
    AsrModelSpec {
        id: "base",
        display: "Whisper base",
        engine: AsrEngine::WhisperCpp,
        model_ref: "base",
        size: 148_000_000,
        released: "2022-09",
        langs: "99 langs",
        license: "MIT",
        note: "tiny + fast; low accuracy, good for smoke tests",
    },
    // --- faster-whisper (CTranslate2, Python bridge) ---------------------
    AsrModelSpec {
        id: "faster-large-v3-turbo",
        display: "faster-whisper large-v3-turbo",
        engine: AsrEngine::FasterWhisper,
        model_ref: "large-v3-turbo",
        size: 1_620_000_000,
        released: "2024-10",
        langs: "99 langs",
        license: "MIT",
        note: "CTranslate2 build; batched, low VRAM, CPU int8 capable",
    },
    AsrModelSpec {
        id: "faster-large-v3",
        display: "faster-whisper large-v3",
        engine: AsrEngine::FasterWhisper,
        model_ref: "large-v3",
        size: 3_100_000_000,
        released: "2023-11",
        langs: "99 langs",
        license: "MIT",
        note: "CTranslate2 large-v3; accurate, GPU-friendly",
    },
    AsrModelSpec {
        id: "distil-large-v3",
        display: "distil-whisper large-v3",
        engine: AsrEngine::FasterWhisper,
        model_ref: "distil-large-v3",
        size: 1_500_000_000,
        released: "2024-03",
        langs: "English",
        license: "MIT",
        note: "distilled, ~2x faster than large-v3, English-focused",
    },
    // --- NVIDIA NeMo Parakeet (FastConformer-TDT, GPU) -------------------
    AsrModelSpec {
        id: "parakeet-v3",
        display: "NVIDIA Parakeet TDT 0.6B v3",
        engine: AsrEngine::Parakeet,
        model_ref: "nvidia/parakeet-tdt-0.6b-v3",
        size: 2_500_000_000,
        released: "2025-08",
        langs: "25 EU langs (auto)",
        license: "CC-BY-4.0",
        note: "very fast (RTFx ~3300), multilingual, word timestamps",
    },
    AsrModelSpec {
        id: "parakeet-v2",
        display: "NVIDIA Parakeet TDT 0.6B v2",
        engine: AsrEngine::Parakeet,
        model_ref: "nvidia/parakeet-tdt-0.6b-v2",
        size: 2_500_000_000,
        released: "2025-05",
        langs: "English",
        license: "CC-BY-4.0",
        note: "fastest English ASR (WER 6.05); NeMo/CUDA",
    },
    // --- NVIDIA NeMo Canary-Qwen (speech LLM, GPU) -----------------------
    AsrModelSpec {
        id: "canary-qwen-2.5b",
        display: "NVIDIA Canary-Qwen 2.5B",
        engine: AsrEngine::CanaryQwen,
        model_ref: "nvidia/canary-qwen-2.5b",
        size: 5_000_000_000,
        released: "2025-07",
        langs: "English",
        license: "CC-BY-4.0",
        note: "best English WER (5.63); FastConformer + Qwen3 LLM; NeMo/CUDA",
    },
    // --- Mistral Voxtral (audio LLM, GPU) --------------------------------
    AsrModelSpec {
        id: "voxtral-mini",
        display: "Mistral Voxtral Mini 3B",
        engine: AsrEngine::Voxtral,
        model_ref: "mistralai/Voxtral-Mini-3B-2507",
        size: 9_500_000_000,
        released: "2025-07",
        langs: "8 langs",
        license: "Apache-2.0",
        note: "audio LLM: transcription + understanding; ~9.5GB VRAM",
    },
];

/// Look up a model spec by its `id`.
pub fn find(id: &str) -> Option<&'static AsrModelSpec> {
    ASR_CATALOG.iter().find(|m| m.id == id)
}

/// The engine that should run model `id` (defaults to whisper.cpp when the id
/// is an unknown/legacy whisper size like `tiny`/`small`).
pub fn engine_of(id: &str) -> AsrEngine {
    find(id).map(|m| m.engine).unwrap_or(AsrEngine::WhisperCpp)
}

// ---------------------------------------------------------------------------
// Diarization catalog
// ---------------------------------------------------------------------------

/// A speaker-diarization model option.
pub struct DiarizeSpec {
    pub id: &'static str,
    pub display: &'static str,
    /// Hugging Face pipeline id used by the pyannote/whisperX path.
    pub model_ref: &'static str,
    pub released: &'static str,
    pub license: &'static str,
    pub note: &'static str,
}

/// Diarization pipelines. `community-1` is the current WhisperX default and a
/// large quality jump over `3.1`; it supports an unlimited number of speakers
/// which suits multi-player tabletop sessions.
pub const DIARIZE_CATALOG: &[DiarizeSpec] = &[
    DiarizeSpec {
        id: "community-1",
        display: "pyannote community-1",
        model_ref: "pyannote/speaker-diarization-community-1",
        released: "2025-09",
        license: "CC-BY-4.0",
        note: "default; unlimited speakers, big DER improvement over 3.1",
    },
    DiarizeSpec {
        id: "3.1",
        display: "pyannote 3.1",
        model_ref: "pyannote/speaker-diarization-3.1",
        released: "2023-11",
        license: "MIT",
        note: "older pipeline; kept for compatibility",
    },
];

/// The default diarization model id.
pub const DEFAULT_DIARIZE: &str = "community-1";

/// Look up a diarization spec by id (falls back to the default).
pub fn diarize_spec(id: &str) -> &'static DiarizeSpec {
    DIARIZE_CATALOG
        .iter()
        .find(|d| d.id == id)
        .unwrap_or(&DIARIZE_CATALOG[0])
}
