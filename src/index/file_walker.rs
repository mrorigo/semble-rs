// Rust guideline compliant 2026-05-18

use std::fs;
use std::path::{Path, PathBuf};

use crate::utils::trace;

const DEFAULT_IGNORED_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    "__pycache__",
    "node_modules",
    ".venv",
    "venv",
    ".tox",
    ".cache",
    ".semble",
    "target",
];

/// Recursively walks the directory tree under `root`, discovering files with matching extensions.
///
/// Automatically respects gitignore-style patterns and standard directory exclusion rules.
///
/// # Arguments
///
/// * `root` - The starting directory path to walk.
/// * `extensions` - List of allowed file extensions to match.
///
/// # Returns
///
/// A sorted vector of absolute paths to matching files.
pub fn walk_files(root: &Path, extensions: &[String]) -> Vec<PathBuf> {
    walk_files_with_meta(root, extensions)
        .into_iter()
        .map(|(path, _)| path)
        .collect()
}

/// A file's change-detection metadata captured during a walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileMeta {
    /// File size in bytes.
    pub size: u64,
    /// Last-modified time in nanoseconds since the Unix epoch.
    pub mtime_nanos: i128,
}

/// Recursively walks `root`, returning matching files together with their
/// change-detection metadata.
///
/// Same discovery rules as [`walk_files`], but also captures each file's size
/// and modification time so callers can detect which files changed between
/// builds without re-reading content.
///
/// # Arguments
///
/// * `root` - The starting directory path to walk.
/// * `extensions` - List of allowed file extensions to match.
///
/// # Returns
///
/// A sorted vector of `(absolute_path, FileMeta)` pairs for matching files.
pub fn walk_files_with_meta(root: &Path, extensions: &[String]) -> Vec<(PathBuf, FileMeta)> {
    trace(format!(
        "walk_files root={} extensions={:?}",
        root.display(),
        extensions
    ));
    let mut out = Vec::new();
    let mut patterns = extensions
        .iter()
        .flat_map(|ext| [format!("!**/*{}", ext), format!("!*{}", ext)])
        .collect::<Vec<_>>();
    patterns.extend(load_patterns(root));
    walk(root, root, extensions, &patterns, &mut out);
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn walk(
    root: &Path,
    directory: &Path,
    extensions: &[String],
    inherited_patterns: &[String],
    out: &mut Vec<(PathBuf, FileMeta)>,
) {
    let mut patterns = inherited_patterns.to_vec();
    patterns.extend(load_patterns(directory));
    let entries = match fs::read_dir(directory) {
        Ok(v) => v,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_symlink() {
            continue;
        }
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if path.is_dir() {
            if DEFAULT_IGNORED_DIRS.contains(&name) || is_ignored(root, &path, true, &patterns) {
                continue;
            }
            trace(format!("descending into {}", path.display()));
            walk(root, &path, extensions, &patterns, out);
        } else if path.is_file() {
            if is_ignored(root, &path, false, &patterns) {
                continue;
            }
            if extensions.iter().any(|ext| {
                path.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| format!(".{}", e) == *ext)
            }) {
                trace(format!("indexing file {}", path.display()));
                if let Some(meta) = file_meta(&path) {
                    out.push((path, meta));
                }
            }
        }
    }
}

fn file_meta(path: &Path) -> Option<FileMeta> {
    let meta = fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    Some(FileMeta {
        size: meta.len(),
        mtime_nanos: mtime.as_secs() as i128 * 1_000_000_000 + mtime.subsec_nanos() as i128,
    })
}

fn load_patterns(directory: &Path) -> Vec<String> {
    [".gitignore", ".sembleignore"]
        .into_iter()
        .filter_map(|name| fs::read_to_string(directory.join(name)).ok())
        .flat_map(|s| s.lines().map(|l| l.trim().to_string()).collect::<Vec<_>>())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect()
}

fn is_ignored(root: &Path, path: &Path, is_dir: bool, patterns: &[String]) -> bool {
    let rel = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    let rel_with_slash = if is_dir {
        format!("{}/", rel)
    } else {
        rel.clone()
    };
    let mut ignored = !is_dir;
    for pat in patterns {
        let negated = pat.starts_with('!');
        let pat = pat.trim_start_matches('!');
        if matches_pattern(&rel_with_slash, pat) || (!is_dir && matches_pattern(&rel, pat)) {
            ignored = !negated;
        }
    }
    ignored
}

fn matches_pattern(path: &str, pattern: &str) -> bool {
    if pattern.ends_with('/') {
        return path.starts_with(pattern.trim_end_matches('/'));
    }
    let anchored = if pattern.contains('/') {
        pattern.to_string()
    } else {
        format!("**/{}", pattern)
    };
    glob_match(&anchored, path)
}

fn glob_match(pattern: &str, text: &str) -> bool {
    let mut regex = if pattern.starts_with("**/") {
        String::from("^(?:.*/)?")
    } else {
        String::from("^")
    };
    let mut chars = if let Some(stripped) = pattern.strip_prefix("**/") {
        stripped.chars().peekable()
    } else {
        pattern.chars().peekable()
    };
    while let Some(ch) = chars.next() {
        match ch {
            '*' => {
                if chars.peek() == Some(&'*') {
                    chars.next();
                    regex.push_str(".*");
                } else {
                    regex.push_str("[^/]*");
                }
            }
            '?' => regex.push('.'),
            '.' | '+' | '(' | ')' | '|' | '^' | '$' | '{' | '}' | '[' | ']' | '\\' => {
                regex.push('\\');
                regex.push(ch);
            }
            c => regex.push(c),
        }
    }
    regex.push('$');
    regex::Regex::new(&regex)
        .map(|re| re.is_match(text))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::walk_files;

    #[test]
    fn walks_root_level_files() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        fs::write(root.join("main.rs"), "fn main() {}\n").expect("write");
        fs::write(root.join("notes.txt"), "ignored\n").expect("write");
        let files = walk_files(root, &[".rs".to_string()]);
        assert_eq!(files, vec![root.join("main.rs")]);
    }

    #[test]
    fn metadata_captures_size_and_positive_mtime() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        fs::write(root.join("a.rs"), "fn a() {}\n").expect("write");
        let pairs = super::walk_files_with_meta(root, &[".rs".to_string()]);
        assert_eq!(pairs.len(), 1);
        let (path, meta) = &pairs[0];
        assert_eq!(path, &root.join("a.rs"));
        assert_eq!(meta.size, 10);
        assert!(meta.mtime_nanos > 0);
    }
}
