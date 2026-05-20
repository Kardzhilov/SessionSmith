//! Audio file scanner: lists candidate inputs newest-first with metadata.

use anyhow::Result;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;
use walkdir::WalkDir;

const AUDIO_EXTS: &[&str] = &["wav", "mp3", "m4a", "flac", "ogg", "opus", "aac", "wma", "webm"];

#[derive(Debug, Clone)]
pub struct AudioFile {
    pub path: PathBuf,
    pub mtime: SystemTime,
    pub duration_secs: Option<f64>,
    pub size_bytes: u64,
    pub already_transcribed: bool,
}

impl AudioFile {
    pub fn stem(&self) -> String {
        self.path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
    }
}

pub fn scan(dir: &Path, transcripts_dir: &Path) -> Result<Vec<AudioFile>> {
    let mut files = Vec::new();
    if !dir.exists() {
        return Ok(files);
    }
    for entry in WalkDir::new(dir).follow_links(true).into_iter().flatten() {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        if !AUDIO_EXTS.iter().any(|&e| e == ext) {
            continue;
        }
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let transcript_path = transcripts_dir.join(format!("{stem}.txt"));
        files.push(AudioFile {
            path: path.to_path_buf(),
            mtime,
            duration_secs: None,
            size_bytes: meta.len(),
            already_transcribed: transcript_path.exists(),
        });
    }
    files.sort_by(|a, b| b.mtime.cmp(&a.mtime));
    Ok(files)
}

/// Probe duration via ffprobe. Returns None if ffprobe is unavailable or fails.
pub fn probe_duration(path: &Path) -> Option<f64> {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "format=duration",
               "-of", "default=noprint_wrappers=1:nokey=1"])
        .arg(path)
        .output().ok()?;
    if !out.status.success() { return None; }
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

pub fn enrich_durations(files: &mut [AudioFile]) {
    // Sequential to avoid spawning many ffprobe processes; usually fast.
    for f in files.iter_mut() {
        if f.duration_secs.is_none() {
            f.duration_secs = probe_duration(&f.path);
        }
    }
}

pub fn human_duration(secs: Option<f64>) -> String {
    match secs {
        None => "?".into(),
        Some(s) => {
            let total = s as u64;
            let h = total / 3600;
            let m = (total % 3600) / 60;
            let sec = total % 60;
            if h > 0 { format!("{h:02}:{m:02}:{sec:02}") }
            else { format!("{m:02}:{sec:02}") }
        }
    }
}

pub fn human_age(t: SystemTime) -> String {
    match SystemTime::now().duration_since(t) {
        Ok(d) => {
            let s = d.as_secs();
            if s < 60 { format!("{s}s ago") }
            else if s < 3600 { format!("{}m ago", s / 60) }
            else if s < 86400 { format!("{}h ago", s / 3600) }
            else { format!("{}d ago", s / 86400) }
        }
        Err(_) => "future".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sort_newest_first() {
        let tmp = tempfile::tempdir().unwrap();
        let audio_dir = tmp.path().join("audio");
        let tx_dir = tmp.path().join("transcripts");
        std::fs::create_dir_all(&audio_dir).unwrap();
        std::fs::create_dir_all(&tx_dir).unwrap();

        // Create with controlled mtimes.
        for (i, n) in ["old.wav", "new.wav"].iter().enumerate() {
            let p = audio_dir.join(n);
            std::fs::write(&p, b"x").unwrap();
            let t = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000 + i as u64 * 1000);
            if let Ok(f) = std::fs::File::options().write(true).open(&p) {
                let _ = f.set_modified(t);
            }
        }

        let files = scan(&audio_dir, &tx_dir).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path.file_name().unwrap(), "new.wav");
    }
}
