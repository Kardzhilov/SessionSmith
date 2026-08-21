//! Small cross-platform filesystem helpers.

use std::path::{Path, PathBuf};

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
}