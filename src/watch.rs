//! Shared stable-file detection for polling audio watchers.

use crate::audio::{self, AudioFile};
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::time::SystemTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileSnapshot {
    size_bytes: u64,
    mtime: SystemTime,
}

#[derive(Debug, Default)]
pub struct StableFileDetector {
    previous: BTreeMap<PathBuf, FileSnapshot>,
    handled: HashSet<PathBuf>,
}

impl StableFileDetector {
    pub fn observe(&mut self, files: &[AudioFile]) -> Vec<PathBuf> {
        let mut current = BTreeMap::new();
        let mut ready = Vec::new();

        for file in files {
            if !audio::is_supported_audio_path(&file.path) {
                continue;
            }
            let snapshot = FileSnapshot {
                size_bytes: file.size_bytes,
                mtime: file.mtime,
            };
            if !file.already_transcribed
                && !self.handled.contains(&file.path)
                && self.previous.get(&file.path) == Some(&snapshot)
            {
                ready.push(file.path.clone());
            }
            current.insert(file.path.clone(), snapshot);
        }

        self.previous = current;
        ready
    }

    pub fn mark_handled(&mut self, path: PathBuf) {
        self.handled.insert(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn audio_file(path: &str, size_bytes: u64) -> AudioFile {
        AudioFile {
            path: PathBuf::from(path),
            mtime: SystemTime::UNIX_EPOCH,
            duration_secs: None,
            size_bytes,
            already_transcribed: false,
        }
    }

    #[test]
    fn requires_two_identical_scans_before_reporting_a_file() {
        let mut detector = StableFileDetector::default();
        let file = audio_file("session.wav", 10);

        assert!(detector.observe(std::slice::from_ref(&file)).is_empty());
        assert_eq!(
            detector.observe(&[file]),
            vec![PathBuf::from("session.wav")]
        );
    }

    #[test]
    fn changed_and_handled_files_are_not_reported() {
        let mut detector = StableFileDetector::default();
        let path = PathBuf::from("session.wav");

        assert!(detector
            .observe(&[audio_file("session.wav", 10)])
            .is_empty());
        assert!(detector
            .observe(&[audio_file("session.wav", 20)])
            .is_empty());
        assert_eq!(
            detector.observe(&[audio_file("session.wav", 20)]),
            vec![path.clone()]
        );
        detector.mark_handled(path);
        assert!(detector
            .observe(&[audio_file("session.wav", 20)])
            .is_empty());
    }

    #[test]
    fn unsupported_files_are_ignored() {
        let mut detector = StableFileDetector::default();
        let file = audio_file("notes.txt", 10);

        assert!(detector.observe(std::slice::from_ref(&file)).is_empty());
        assert!(detector.observe(&[file]).is_empty());
    }
}
