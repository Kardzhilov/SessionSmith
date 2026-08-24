//! Safe resolution of generated candidate artifacts.

use anyhow::{bail, Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Promote a candidate artifact over its current version without exposing an
/// interval where a failed replacement silently loses the current document.
pub fn promote(candidate: &Path, current: &Path) -> Result<()> {
    if !candidate.is_file() {
        bail!("candidate artifact does not exist: {}", candidate.display());
    }
    if candidate.parent() != current.parent() {
        bail!("candidate and current artifacts must share a directory");
    }
    if !current.exists() {
        return fs::rename(candidate, current).with_context(|| {
            format!(
                "promoting candidate artifact {} to {}",
                candidate.display(),
                current.display()
            )
        });
    }
    if !current.is_file() {
        bail!(
            "current artifact is not a regular file: {}",
            current.display()
        );
    }

    let backup = replacement_backup_path(current)?;
    fs::rename(current, &backup).with_context(|| {
        format!(
            "saving current artifact before candidate promotion: {}",
            current.display()
        )
    })?;
    if let Err(error) = fs::rename(candidate, current) {
        let restore_result = fs::rename(&backup, current);
        if let Err(restore_error) = restore_result {
            return Err(error).with_context(|| {
                format!(
                    "promoting candidate artifact failed and restoring {} also failed: {restore_error}",
                    current.display()
                )
            });
        }
        return Err(error).with_context(|| {
            format!(
                "promoting candidate artifact {} to {}",
                candidate.display(),
                current.display()
            )
        });
    }

    let _ = fs::remove_file(&backup);
    Ok(())
}

/// Discard an unkept candidate artifact.
pub fn discard(candidate: &Path) -> Result<()> {
    if !candidate.is_file() {
        bail!("candidate artifact does not exist: {}", candidate.display());
    }
    fs::remove_file(candidate)
        .with_context(|| format!("discarding candidate artifact: {}", candidate.display()))
}

/// Keep the existing current artifact and move its candidate to a unique
/// sibling such as `summary-alt.md` or `summary-alt-2.md`.
pub fn keep_both(candidate: &Path, current: &Path) -> Result<PathBuf> {
    if !candidate.is_file() {
        bail!("candidate artifact does not exist: {}", candidate.display());
    }
    if candidate.parent() != current.parent() {
        bail!("candidate and current artifacts must share a directory");
    }
    if !current.is_file() {
        bail!(
            "keeping both requires a current artifact: {}",
            current.display()
        );
    }

    let alternate = alternate_path(current)?;
    fs::rename(candidate, &alternate).with_context(|| {
        format!(
            "keeping candidate artifact {} alongside {}",
            candidate.display(),
            current.display()
        )
    })?;
    Ok(alternate)
}

/// Whether `name` is an alternate generated from `current_name` by
/// [`keep_both`]. This deliberately recognizes only SessionSmith's own
/// collision-safe filename format.
pub fn is_alternate_filename(current_name: &str, name: &str) -> bool {
    let Some((stem, extension)) = current_name.rsplit_once('.') else {
        return false;
    };
    let base = format!("{stem}-alt");
    if name == format!("{base}.{extension}") {
        return true;
    }
    let Some(suffix) = name
        .strip_prefix(&format!("{base}-"))
        .and_then(|suffix| suffix.strip_suffix(&format!(".{extension}")))
    else {
        return false;
    };
    suffix.parse::<u32>().is_ok_and(|index| index >= 2)
}

fn replacement_backup_path(current: &Path) -> Result<PathBuf> {
    let parent = current
        .parent()
        .ok_or_else(|| anyhow::anyhow!("current artifact has no parent directory"))?;
    let filename = current
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("current artifact has no UTF-8 file name"))?;
    for index in 0..1_000 {
        let backup = parent.join(format!(".{filename}.sessionsmith-replace-{index}"));
        if !backup.exists() {
            return Ok(backup);
        }
    }
    bail!("could not reserve a temporary replacement path")
}

fn alternate_path(current: &Path) -> Result<PathBuf> {
    let parent = current
        .parent()
        .ok_or_else(|| anyhow::anyhow!("current artifact has no parent directory"))?;
    let filename = current
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("current artifact has no UTF-8 file name"))?;
    let (stem, extension) = filename
        .rsplit_once('.')
        .ok_or_else(|| anyhow::anyhow!("current artifact must have a file extension"))?;

    for index in 1..1_000 {
        let suffix = if index == 1 {
            "-alt".to_string()
        } else {
            format!("-alt-{index}")
        };
        let alternate = parent.join(format!("{stem}{suffix}.{extension}"));
        if !alternate.exists() {
            return Ok(alternate);
        }
    }
    bail!("could not reserve an alternate artifact path")
}

#[cfg(test)]
mod tests {
    use super::{discard, is_alternate_filename, keep_both, promote};

    #[test]
    fn promotion_replaces_current_and_removes_candidate() {
        let temp = tempfile::tempdir().unwrap();
        let current = temp.path().join("summary.md");
        let candidate = temp.path().join("summary.candidate.md");
        std::fs::write(&current, "current").unwrap();
        std::fs::write(&candidate, "candidate").unwrap();

        promote(&candidate, &current).unwrap();

        assert_eq!(std::fs::read_to_string(&current).unwrap(), "candidate");
        assert!(!candidate.exists());
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 1);
    }

    #[test]
    fn promotion_creates_the_current_artifact_when_only_candidate_exists() {
        let temp = tempfile::tempdir().unwrap();
        let current = temp.path().join("summary.md");
        let candidate = temp.path().join("summary.candidate.md");
        std::fs::write(&candidate, "candidate").unwrap();

        promote(&candidate, &current).unwrap();

        assert_eq!(std::fs::read_to_string(&current).unwrap(), "candidate");
        assert!(!candidate.exists());
    }

    #[test]
    fn discard_removes_only_the_candidate() {
        let temp = tempfile::tempdir().unwrap();
        let candidate = temp.path().join("summary.candidate.md");
        std::fs::write(&candidate, "candidate").unwrap();

        discard(&candidate).unwrap();

        assert!(!candidate.exists());
    }

    #[test]
    fn keep_both_preserves_current_and_uses_a_collision_safe_alternate_name() {
        let temp = tempfile::tempdir().unwrap();
        let current = temp.path().join("summary.md");
        let candidate = temp.path().join("summary.candidate.md");
        std::fs::write(&current, "current").unwrap();
        std::fs::write(temp.path().join("summary-alt.md"), "older alternate").unwrap();
        std::fs::write(&candidate, "candidate").unwrap();

        let alternate = keep_both(&candidate, &current).unwrap();

        assert_eq!(alternate.file_name().unwrap(), "summary-alt-2.md");
        assert_eq!(std::fs::read_to_string(&current).unwrap(), "current");
        assert_eq!(std::fs::read_to_string(&alternate).unwrap(), "candidate");
        assert!(!candidate.exists());
    }

    #[test]
    fn alternate_names_are_tied_to_the_current_artifact_name() {
        assert!(is_alternate_filename("summary.md", "summary-alt.md"));
        assert!(is_alternate_filename("summary.md", "summary-alt-2.md"));
        assert!(!is_alternate_filename("summary.md", "summary-alt-1.md"));
        assert!(!is_alternate_filename("summary.md", "recap-alt.md"));
        assert!(!is_alternate_filename("summary.md", "summary.candidate.md"));
    }
}
