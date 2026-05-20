//! Hardware detection + ASR / LLM model recommendation tiers.

use serde::{Deserialize, Serialize};
use std::process::Command;
use sysinfo::System;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareProfile {
    pub os: String,
    pub cpu_cores: usize,
    pub ram_gb: u64,
    pub gpu: Option<Gpu>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gpu {
    pub vendor: String,
    pub name: String,
    pub vram_gb: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Recommendation {
    pub whisper_model: &'static str,
    pub llm_model: &'static str,
    pub llm_context_hint: u32,
    pub reason: String,
}

pub fn detect() -> HardwareProfile {
    let mut sys = System::new_all();
    sys.refresh_memory();
    let ram_gb = sys.total_memory() / 1024 / 1024 / 1024;
    let cpu_cores = num_cpus();
    let os = format!("{} {}", std::env::consts::OS, std::env::consts::ARCH);
    let gpu = detect_gpu();
    HardwareProfile { os, cpu_cores, ram_gb, gpu }
}

fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

fn detect_gpu() -> Option<Gpu> {
    // NVIDIA first
    if let Some(gpu) = detect_nvidia() {
        return Some(gpu);
    }
    // Apple Silicon
    if cfg!(target_os = "macos") && std::env::consts::ARCH == "aarch64" {
        return Some(Gpu {
            vendor: "Apple".into(),
            name: "Apple Silicon (unified memory)".into(),
            vram_gb: detect_apple_unified_gb().unwrap_or(0),
        });
    }
    // AMD via rocm-smi best-effort
    detect_amd()
}

fn detect_nvidia() -> Option<Gpu> {
    let out = Command::new("nvidia-smi")
        .args(["--query-gpu=name,memory.total", "--format=csv,noheader,nounits"])
        .output().ok()?;
    if !out.status.success() { return None; }
    let line = String::from_utf8_lossy(&out.stdout);
    let first = line.lines().next()?;
    let mut parts = first.splitn(2, ',').map(|s| s.trim());
    let name = parts.next()?.to_string();
    let mib: u64 = parts.next()?.parse().ok()?;
    Some(Gpu { vendor: "NVIDIA".into(), name, vram_gb: mib / 1024 })
}

fn detect_amd() -> Option<Gpu> {
    let out = Command::new("rocm-smi")
        .args(["--showmeminfo", "vram", "--json"])
        .output().ok()?;
    if !out.status.success() { return None; }
    // Best-effort: just record presence.
    Some(Gpu { vendor: "AMD".into(), name: "AMD GPU (rocm)".into(), vram_gb: 0 })
}

fn detect_apple_unified_gb() -> Option<u64> {
    let out = Command::new("sysctl").args(["-n", "hw.memsize"]).output().ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    let bytes: u64 = s.trim().parse().ok()?;
    Some(bytes / 1024 / 1024 / 1024)
}

/// Pick recommended ASR + LLM models given the detected hardware.
pub fn recommend(hw: &HardwareProfile) -> Recommendation {
    let vram = hw.gpu.as_ref().map(|g| g.vram_gb).unwrap_or(0);
    let unified = matches!(hw.gpu.as_ref().map(|g| g.vendor.as_str()), Some("Apple"));
    let effective_vram = if unified { hw.ram_gb } else { vram };

    if effective_vram >= 24 {
        Recommendation {
            whisper_model: "large-v3",
            llm_model: "qwen2.5:32b",
            llm_context_hint: 32_768,
            reason: format!(
                "{} effective VRAM ≥ 24 GB → top-tier ASR + large local LLM",
                effective_vram
            ),
        }
    } else if effective_vram >= 12 {
        Recommendation {
            whisper_model: "large-v3-turbo",
            llm_model: "qwen2.5:14b",
            llm_context_hint: 16_384,
            reason: format!("{} GB VRAM → turbo ASR + mid LLM", effective_vram),
        }
    } else if effective_vram >= 6 {
        Recommendation {
            whisper_model: "medium",
            llm_model: "qwen2.5:7b",
            llm_context_hint: 8_192,
            reason: format!("{} GB VRAM → medium ASR + 7B LLM", effective_vram),
        }
    } else if hw.ram_gb >= 16 {
        Recommendation {
            whisper_model: "small",
            llm_model: "qwen2.5:7b",
            llm_context_hint: 8_192,
            reason: format!("CPU only, {} GB RAM → small ASR + 7B LLM (slow)", hw.ram_gb),
        }
    } else {
        Recommendation {
            whisper_model: "base",
            llm_model: "qwen2.5:3b",
            llm_context_hint: 8_192,
            reason: format!("Low RAM ({} GB) → base ASR + 3B LLM", hw.ram_gb),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hw(vram: u64, ram: u64, vendor: &str) -> HardwareProfile {
        HardwareProfile {
            os: "linux".into(),
            cpu_cores: 8,
            ram_gb: ram,
            gpu: if vram > 0 || vendor == "Apple" {
                Some(Gpu { vendor: vendor.into(), name: "x".into(), vram_gb: vram })
            } else { None },
        }
    }

    #[test]
    fn tiers() {
        assert_eq!(recommend(&hw(24, 64, "NVIDIA")).whisper_model, "large-v3");
        assert_eq!(recommend(&hw(16, 32, "NVIDIA")).whisper_model, "large-v3-turbo");
        assert_eq!(recommend(&hw(8, 16, "NVIDIA")).whisper_model, "medium");
        assert_eq!(recommend(&hw(0, 32, "")).whisper_model, "small");
        assert_eq!(recommend(&hw(0, 8, "")).whisper_model, "base");
        // Apple unified
        assert_eq!(recommend(&hw(0, 32, "Apple")).whisper_model, "large-v3");
    }
}
