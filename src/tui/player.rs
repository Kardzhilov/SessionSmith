//! A small headless audio player for the TUI.
//!
//! There is no cross-platform Rust audio decoder wired in, so we drive an
//! external player process (`ffplay`, which ships with ffmpeg; `mpv` as a
//! fallback). Seeking and pause are implemented by killing and re-spawning the
//! process at a new offset — good enough for "jump to a quote" and coarse
//! scrubbing. Position is estimated from wall-clock time since the last spawn.

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

pub struct Player {
    /// Audio file being played.
    pub file: PathBuf,
    /// Short display label (e.g. the file name or session stem).
    pub label: String,
    /// Total duration in milliseconds (0 = unknown / still probing). Filled in
    /// on a background thread so a slow probe never blocks the UI.
    duration_ms: Arc<AtomicU64>,
    /// Playback offset the current process was spawned at.
    base_offset: f64,
    /// When the current process was spawned (for position estimation).
    started: Instant,
    /// Whether playback is paused.
    pub paused: bool,
    /// Position captured when paused.
    paused_at: f64,
    /// Playback volume 0–100.
    pub volume: u8,
    child: Option<Child>,
    backend: &'static str,
}

impl Player {
    /// Start playing `file` (labelled `label`) from `offset` seconds at
    /// `volume` (0–100).
    pub fn start(file: &Path, label: &str, offset: f64, volume: u8) -> io::Result<Player> {
        let backend = pick_backend().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "no audio player found — install ffmpeg (for ffplay) or mpv",
            )
        })?;
        let duration_ms = Arc::new(AtomicU64::new(0));
        spawn_duration_probe(file.to_path_buf(), Arc::clone(&duration_ms));
        let mut p = Player {
            file: file.to_path_buf(),
            label: label.to_string(),
            duration_ms,
            base_offset: 0.0,
            started: Instant::now(),
            paused: false,
            paused_at: 0.0,
            volume: volume.min(100),
            child: None,
            backend,
        };
        p.spawn_at(offset.max(0.0))?;
        Ok(p)
    }

    fn spawn_at(&mut self, offset: f64) -> io::Result<()> {
        self.kill();
        self.child = Some(spawn_cmd(self.backend, &self.file, offset, self.volume)?);
        self.base_offset = offset;
        self.started = Instant::now();
        self.paused = false;
        Ok(())
    }

    /// Total duration in seconds (0.0 while unknown / still probing).
    pub fn duration(&self) -> f64 {
        self.duration_ms.load(Ordering::Relaxed) as f64 / 1000.0
    }

    /// Estimated current playback position in seconds.
    pub fn position(&self) -> f64 {
        let dur = self.duration();
        let pos = if self.paused {
            self.paused_at
        } else {
            self.base_offset + self.started.elapsed().as_secs_f64()
        };
        if dur > 0.0 {
            pos.min(dur)
        } else {
            pos.max(0.0)
        }
    }

    /// Pause or resume playback.
    pub fn toggle_pause(&mut self) {
        if self.paused {
            let at = self.paused_at;
            let _ = self.spawn_at(at);
        } else {
            self.paused_at = self.position();
            self.kill();
            self.paused = true;
        }
    }

    /// Seek by `delta` seconds (negative to rewind), restarting playback there.
    pub fn seek(&mut self, delta: f64) {
        let mut target = (self.position() + delta).max(0.0);
        let dur = self.duration();
        if dur > 0.0 {
            target = target.min((dur - 0.2).max(0.0));
        }
        if self.paused {
            self.paused_at = target;
        } else {
            let _ = self.spawn_at(target);
        }
    }

    /// Set the volume (0–100), restarting playback at the current position so
    /// the change takes effect immediately.
    pub fn set_volume(&mut self, volume: u8) {
        self.volume = volume.min(100);
        if !self.paused {
            let at = self.position();
            let _ = self.spawn_at(at);
        }
    }

    /// True once playback has ended on its own (the process exited).
    pub fn finished(&mut self) -> bool {
        if self.paused {
            return false;
        }
        match &mut self.child {
            Some(c) => matches!(c.try_wait(), Ok(Some(_))),
            None => true,
        }
    }

    fn kill(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.kill();
    }
}

fn pick_backend() -> Option<&'static str> {
    if bin_exists("ffplay") {
        Some("ffplay")
    } else if bin_exists("mpv") {
        Some("mpv")
    } else {
        None
    }
}

fn bin_exists(bin: &str) -> bool {
    crate::util::find_in_path(bin).is_some()
}

fn spawn_cmd(backend: &str, file: &Path, offset: f64, volume: u8) -> io::Result<Child> {
    let seek = format!("{offset:.3}");
    let vol = volume.min(100);
    let mut cmd = if backend == "mpv" {
        let mut c = Command::new("mpv");
        c.args(["--no-video", "--really-quiet", &format!("--start={seek}"), &format!("--volume={vol}")]);
        c
    } else {
        let mut c = Command::new("ffplay");
        c.args(["-nodisp", "-autoexit", "-loglevel", "quiet", "-ss", &seek, "-volume", &vol.to_string()]);
        c
    };
    cmd.arg(file)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

/// Probe a file's duration (millis) on a background thread and store it into
/// `slot` when done. FLAC and other headerless containers may need a full
/// decode, which can take a couple of seconds for multi-hour files — doing it
/// off-thread keeps the UI responsive.
fn spawn_duration_probe(file: PathBuf, slot: Arc<AtomicU64>) {
    std::thread::spawn(move || {
        let secs = probe_duration(&file);
        if secs > 0.0 {
            slot.store((secs * 1000.0) as u64, Ordering::Relaxed);
        }
    });
}

/// Probe a file's duration in seconds (0.0 if unavailable). Tries the fast
/// ffprobe queries first, then falls back to a full ffmpeg decode for
/// containers (e.g. streamed FLAC) that carry no duration metadata.
fn probe_duration(file: &Path) -> f64 {
    let run = |args: &[&str]| -> f64 {
        match Command::new("ffprobe").args(args).arg(file).output() {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
                .lines()
                .find_map(|l| l.trim().parse::<f64>().ok())
                .filter(|v| v.is_finite() && *v > 0.0)
                .unwrap_or(0.0),
            _ => 0.0,
        }
    };
    let d = run(&["-v", "error", "-show_entries", "format=duration", "-of", "default=nk=1:nw=1"]);
    if d > 0.0 {
        return d;
    }
    let d = run(&[
        "-v", "error",
        "-select_streams", "a:0",
        "-show_entries", "stream=duration",
        "-of", "default=nk=1:nw=1",
    ]);
    if d > 0.0 {
        return d;
    }
    decode_duration(file)
}

/// Last resort: fully decode the file with ffmpeg and parse the final
/// `time=HH:MM:SS.ss` from its progress output. Works for any decodable file.
fn decode_duration(file: &Path) -> f64 {
    let out = Command::new("ffmpeg")
        .arg("-i")
        .arg(file)
        .args(["-f", "null", "-"])
        .stdout(Stdio::null())
        .output();
    let Ok(out) = out else { return 0.0 };
    let stderr = String::from_utf8_lossy(&out.stderr);
    // Progress lines look like `... time=00:12:34.56 bitrate=...`; take the last.
    let mut best = 0.0;
    for token in stderr.split_whitespace() {
        if let Some(hms) = token.strip_prefix("time=") {
            if let Some(secs) = parse_hms(hms) {
                best = secs;
            }
        }
    }
    best
}

/// Parse `HH:MM:SS.ss` into seconds.
fn parse_hms(s: &str) -> Option<f64> {
    let mut parts = s.split(':');
    let h: f64 = parts.next()?.parse().ok()?;
    let m: f64 = parts.next()?.parse().ok()?;
    let sec: f64 = parts.next()?.parse().ok()?;
    let total = h * 3600.0 + m * 60.0 + sec;
    if total.is_finite() && total > 0.0 {
        Some(total)
    } else {
        None
    }
}

/// Format seconds as `M:SS` (or `H:MM:SS`).
pub fn fmt_time(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    let (h, m, sec) = (s / 3600, (s % 3600) / 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{sec:02}")
    } else {
        format!("{m}:{sec:02}")
    }
}
