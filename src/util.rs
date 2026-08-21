//! Small cross-platform filesystem helpers.

use std::path::{Path, PathBuf};

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
}
