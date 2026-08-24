//! Audio file scanner: lists candidate inputs newest-first with metadata.

use anyhow::Result;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::SystemTime;
use walkdir::WalkDir;

use crate::jobs::procs::{configure_command, ChildRegistry};

const AUDIO_EXTS: &[&str] = &[
    "wav", "mp3", "m4a", "flac", "ogg", "opus", "aac", "wma", "webm",
];

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
        self.path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default()
    }
}

/// Whether a path has an audio extension SessionSmith can ingest.
pub fn is_supported_audio_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            AUDIO_EXTS
                .iter()
                .any(|known| known.eq_ignore_ascii_case(extension))
        })
        .unwrap_or(false)
}

pub fn scan(dir: &Path, transcripts_dir: &Path) -> Result<Vec<AudioFile>> {
    let mut files = Vec::new();
    if !dir.exists() {
        return Ok(files);
    }
    for entry in WalkDir::new(dir).follow_links(true).into_iter().flatten() {
        if entry
            .path()
            .components()
            .any(|part| part.as_os_str() == "merged")
        {
            continue;
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if !is_supported_audio_path(path) {
            continue;
        }
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let transcript_path = transcripts_dir.join(format!("{stem}.txt"));
        files.push(AudioFile {
            path: path.to_path_buf(),
            mtime,
            duration_secs: None,
            size_bytes: meta.len(),
            already_transcribed: transcript_path.exists(),
        });
    }
    files.sort_by_key(|f| std::cmp::Reverse(f.mtime));
    Ok(files)
}

/// Find the newest recursively-scanned audio file whose stem matches `stem`.
pub fn find_by_stem(dir: &Path, stem: &str) -> Option<PathBuf> {
    WalkDir::new(dir)
        .follow_links(true)
        .into_iter()
        .flatten()
        .filter(|entry| {
            entry.file_type().is_file()
                && !entry
                    .path()
                    .components()
                    .any(|part| part.as_os_str() == "merged")
                && entry.path().file_stem().and_then(|value| value.to_str()) == Some(stem)
                && entry
                    .path()
                    .extension()
                    .and_then(|value| value.to_str())
                    .map(|value| AUDIO_EXTS.iter().any(|ext| ext.eq_ignore_ascii_case(value)))
                    .unwrap_or(false)
        })
        .max_by_key(|entry| entry.metadata().ok().and_then(|meta| meta.modified().ok()))
        .map(|entry| entry.into_path())
}

/// Probe duration via ffprobe. Returns None if ffprobe is unavailable or fails.
pub fn probe_duration(path: &Path) -> Option<f64> {
    probe_duration_with_children(path, None)
}

/// Probe duration while associating ffprobe with a host-owned job. Existing
/// library and player callers use [`probe_duration`] without a registry.
pub fn probe_duration_with_children(path: &Path, children: Option<&ChildRegistry>) -> Option<f64> {
    let mut command = Command::new("ffprobe");
    command
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(path);
    let out = run_command_output(&mut command, children, "ffprobe audio duration").ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

fn run_command_output(
    command: &mut Command,
    children: Option<&ChildRegistry>,
    label: &str,
) -> std::io::Result<std::process::Output> {
    let Some(children) = children else {
        return command.output();
    };

    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    configure_command(command);
    let child = command.spawn()?;
    let registration = children.register(&child, label);
    let output = child.wait_with_output();
    drop(registration);
    output
}

pub fn enrich_durations(files: &mut [AudioFile]) {
    let pending: Vec<_> = files
        .iter()
        .enumerate()
        .filter(|(_, file)| file.duration_secs.is_none())
        .map(|(index, file)| (index, file.path.clone()))
        .collect();
    if pending.is_empty() {
        return;
    }
    let workers = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1)
        .min(4)
        .min(pending.len());
    let next = AtomicUsize::new(0);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let tx = tx.clone();
            let pending = &pending;
            let next = &next;
            scope.spawn(move || loop {
                let job = next.fetch_add(1, Ordering::Relaxed);
                let Some((index, path)) = pending.get(job) else {
                    break;
                };
                let _ = tx.send((*index, probe_duration(path)));
            });
        }
    });
    drop(tx);
    for (index, duration) in rx {
        files[index].duration_secs = duration;
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
            if h > 0 {
                format!("{h:02}:{m:02}:{sec:02}")
            } else {
                format!("{m:02}:{sec:02}")
            }
        }
    }
}

pub fn human_age(t: SystemTime) -> String {
    match SystemTime::now().duration_since(t) {
        Ok(d) => {
            let s = d.as_secs();
            if s < 60 {
                format!("{s}s ago")
            } else if s < 3600 {
                format!("{}m ago", s / 60)
            } else if s < 86400 {
                format!("{}h ago", s / 3600)
            } else {
                format!("{}d ago", s / 86400)
            }
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
            let t = SystemTime::UNIX_EPOCH
                + std::time::Duration::from_secs(1_000_000 + i as u64 * 1000);
            if let Ok(f) = std::fs::File::options().write(true).open(&p) {
                let _ = f.set_modified(t);
            }
        }

        let files = scan(&audio_dir, &tx_dir).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path.file_name().unwrap(), "new.wav");
    }

    #[test]
    fn find_by_stem_recurses_and_prefers_newest_file() {
        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp.path().join("2026").join("recordings");
        std::fs::create_dir_all(&nested).unwrap();
        let old = tmp.path().join("session.wav");
        let newest = nested.join("session.flac");
        std::fs::write(&old, b"old").unwrap();
        std::fs::write(&newest, b"new").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_modified(SystemTime::UNIX_EPOCH)
            .unwrap();

        assert_eq!(find_by_stem(tmp.path(), "session"), Some(newest));
    }

    #[test]
    fn recognizes_supported_audio_extensions_case_insensitively() {
        assert!(is_supported_audio_path(Path::new("session.WAV")));
        assert!(is_supported_audio_path(Path::new("session.m4a")));
        assert!(!is_supported_audio_path(Path::new("session.txt")));
        assert!(!is_supported_audio_path(Path::new("session")));
    }

    #[cfg(unix)]
    #[test]
    fn registry_aware_duration_probe_command_reaps_and_deregisters() {
        let registry = ChildRegistry::default();
        let mut command = Command::new("sh");
        command.args(["-c", "printf 12.5"]);

        let output = run_command_output(&mut command, Some(&registry), "test duration probe")
            .expect("command should run");
        assert!(output.status.success());
        assert_eq!(output.stdout, b"12.5");
        assert!(registry.active_children().is_empty());
    }
}
