//! Dev-only ASR smoke check.
//!
//! Transcribe:  cargo run --example asr_check -- <audio> [model-id]
//! Prepare:     cargo run --example asr_check -- prepare:<model-id>

use sessionsmith::config::GlobalConfig;
use sessionsmith::transcribe::{transcribe, TranscribeOpts};
use std::path::PathBuf;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let first = args.next().expect("usage: asr_check <audio|prepare:model> [model]");

    if let Some(id) = first.strip_prefix("prepare:") {
        let spec = sessionsmith::asr::find(id).expect("unknown ASR model id");
        sessionsmith::pybridge::run_asr_prepare(spec.engine, spec.model_ref, "auto")?;
        sessionsmith::asr::mark_prepared(id);
        println!(
            "\nprepared {} ({}) — is_prepared={}\n",
            spec.display,
            spec.engine.label(),
            sessionsmith::asr::is_prepared(id)
        );
        return Ok(());
    }

    let audio = PathBuf::from(first);
    let model = args.next().unwrap_or_else(|| "large-v3-turbo".to_string());

    let out = std::env::temp_dir().join("ss_asr_check");
    std::fs::create_dir_all(&out)?;

    let g = GlobalConfig::default();
    let opts = TranscribeOpts {
        model: model.clone(),
        language: "en".to_string(),
        force: true,
        replacements: std::collections::BTreeMap::new(),
        diarize: false,
        vad: false,
    };
    let res = transcribe(&audio, &out, &g, &opts).await?;
    let txt = std::fs::read_to_string(&res.txt)?;
    println!("\n=== [{model}] transcript ===\n{}\n", txt.trim());
    Ok(())
}
