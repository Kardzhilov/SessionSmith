//! Small cross-platform filesystem helpers.

use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{self, ErrorKind, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

/// Derive a filesystem-safe identifier, falling back when no alphanumeric
/// characters remain.
pub fn slugify(value: &str) -> String {
    let normalized: String = value
        .to_lowercase()
        .chars()
        .map(|character| {
            if character.is_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect();
    let slug = normalized
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        "campaign".into()
    } else {
        slug
    }
}

/// Stable content revision for an optimistic write contract. The caller sends
/// this value back after reading a document, rather than exposing a path or
/// trusting filesystem timestamp precision across platforms.
pub fn content_revision(contents: &[u8]) -> String {
    hex::encode(Sha256::digest(contents))
}

/// Replace a regular file only when its current contents still match
/// `expected_revision`. The new content is written to a same-directory temp
/// file, then installed through a rollback-aware rename sequence.
pub fn atomic_replace_if_revision(
    path: &Path,
    expected_revision: &str,
    contents: &[u8],
) -> io::Result<()> {
    let current = fs::read(path)?;
    if content_revision(&current) != expected_revision {
        return Err(io::Error::new(
            ErrorKind::WouldBlock,
            "document changed since it was read",
        ));
    }
    if !path.is_file() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "document path is not a regular file",
        ));
    }

    let temporary = unique_sibling_path(path, "write")?;
    let backup = unique_sibling_path(path, "backup")?;
    let permissions = fs::metadata(path)?.permissions();
    let write_result = (|| -> io::Result<()> {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        file.write_all(contents)?;
        file.sync_all()?;
        fs::set_permissions(&temporary, permissions)?;

        if content_revision(&fs::read(path)?) != expected_revision {
            return Err(io::Error::new(
                ErrorKind::WouldBlock,
                "document changed before replacement",
            ));
        }

        fs::rename(path, &backup)?;
        if let Err(error) = fs::rename(&temporary, path) {
            let restore = fs::rename(&backup, path);
            if let Err(restore_error) = restore {
                return Err(io::Error::new(
                    error.kind(),
                    format!(
                        "replacing document failed and restoration also failed: {restore_error}"
                    ),
                ));
            }
            return Err(error);
        }
        let _ = fs::remove_file(&backup);
        Ok(())
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    write_result
}

fn unique_sibling_path(path: &Path, purpose: &str) -> io::Result<PathBuf> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            ErrorKind::InvalidInput,
            "document path has no parent directory",
        )
    })?;
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            io::Error::new(
                ErrorKind::InvalidInput,
                "document path has no UTF-8 filename",
            )
        })?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    for counter in 0..1_000 {
        let candidate = parent.join(format!(
            ".{filename}.sessionsmith-{purpose}-{}-{timestamp}-{counter}",
            std::process::id(),
        ));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(io::Error::new(
        ErrorKind::AlreadyExists,
        "could not reserve a temporary document path",
    ))
}

/// Find an executable using the process PATH, honoring `PATHEXT` on Windows.
pub fn find_in_path(program: &str) -> Option<PathBuf> {
    let candidate = Path::new(program);
    if candidate.components().count() > 1 {
        return executable_path(candidate);
    }
    let extensions = executable_extensions();
    for directory in std::env::split_paths(&std::env::var_os("PATH")?) {
        for extension in &extensions {
            let path = if extension.is_empty() {
                directory.join(program)
            } else {
                directory.join(format!("{program}{extension}"))
            };
            if let Some(path) = executable_path(&path) {
                return Some(path);
            }
        }
    }
    None
}

fn executable_extensions() -> Vec<String> {
    #[cfg(windows)]
    {
        let mut extensions: Vec<_> = std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
            .split(';')
            .filter(|extension| !extension.is_empty())
            .map(|extension| extension.to_ascii_lowercase())
            .collect();
        extensions.insert(0, String::new());
        extensions
    }
    #[cfg(not(windows))]
    {
        vec![String::new()]
    }
}

fn executable_path(path: &Path) -> Option<PathBuf> {
    let metadata = path.metadata().ok()?;
    if !metadata.is_file() {
        return None;
    }
    #[cfg(unix)]
    if std::os::unix::fs::PermissionsExt::mode(&metadata.permissions()) & 0o111 == 0 {
        return None;
    }
    Some(path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_binary_is_not_found() {
        assert!(find_in_path("sessionsmith-definitely-not-installed").is_none());
    }

    #[test]
    fn slugify_normalizes_names_and_uses_a_fallback() {
        assert_eq!(slugify("Curse of Strahd"), "curse-of-strahd");
        assert_eq!(slugify("  My___Game!!  "), "my-game");
        assert_eq!(slugify("---"), "campaign");
    }

    #[test]
    fn atomic_replacement_updates_matching_content_without_leaving_siblings() {
        let directory = tempfile::tempdir().unwrap();
        let document = directory.path().join("summary.md");
        std::fs::write(&document, "current").unwrap();

        atomic_replace_if_revision(&document, &content_revision(b"current"), b"edited").unwrap();

        assert_eq!(std::fs::read_to_string(&document).unwrap(), "edited");
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn atomic_replacement_rejects_a_stale_revision_without_changing_the_document() {
        let directory = tempfile::tempdir().unwrap();
        let document = directory.path().join("summary.md");
        std::fs::write(&document, "newer").unwrap();

        let error = atomic_replace_if_revision(&document, &content_revision(b"older"), b"edited")
            .unwrap_err();

        assert_eq!(error.kind(), ErrorKind::WouldBlock);
        assert_eq!(std::fs::read_to_string(&document).unwrap(), "newer");
    }
}
