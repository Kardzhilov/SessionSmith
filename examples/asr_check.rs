//! Dev-only ASR smoke check: transcribe an audio file with a chosen model and
//! print the transcript. Bypasses campaign setup.
//!
//! Usage: cargo run --example asr_check -- <audio> [model-id]

use sessionsmith::config::GlobalConfig;
use sessionsmith::transcribe::{transcribe, TranscribeOpts};
use std::path::PathBuf;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let audio = PathBuf::from(args.next().expect("usage: asr_check <audio> [model]"));
    let model = args.next().unwrap_or_else(|| "large-v3-turbo".to_string());

    let out = std::env::temp_dir().join("ss_asr_check");
    std::fs::create_dir_all(&out)?;

    let g = GlobalConfig::default();
    let opts = TranscribeOpts {
        model: model.clone(),
        language: "en".to_string(),
        force: true,
        diarize: false,
        vad: false,
    };
    let res = transcribe(&audio, &out, &g, &opts).await?;
    let txt = std::fs::read_to_string(&res.txt)?;
    println!("\n=== [{model}] transcript ===\n{}\n", txt.trim());
    Ok(())
}
