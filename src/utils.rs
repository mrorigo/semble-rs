use crate::types::{Chunk, SearchResult};

const GIT_URL_SCHEMES: [&str; 6] = [
    "https://",
    "http://",
    "ssh://",
    "git://",
    "git+ssh://",
    "file://",
];

pub fn is_git_url(path: &str) -> bool {
    GIT_URL_SCHEMES
        .iter()
        .any(|scheme| path.starts_with(scheme))
        || path.contains('@') && path.contains(':') && !path.starts_with('/')
}

/// Normalizes a user-provided file path into the root-relative form used by
/// indexed [`Chunk`]s.
///
/// Absolute paths are canonicalized and stripped of the index root prefix.
/// Relative paths are interpreted against `root` when provided. If the path
/// cannot be resolved (e.g., the file does not exist on disk), the input is
/// returned unchanged so downstream chunk matching can still attempt an exact
/// string comparison.
///
/// # Arguments
///
/// * `root` - The canonical index root directory, if known.
/// * `file_path` - An absolute or root-relative file path.
///
/// # Returns
///
/// A root-relative path string, or the original input if normalization fails.
pub fn normalize_file_path(root: Option<&std::path::Path>, file_path: &str) -> String {
    use std::path::PathBuf;

    let raw = PathBuf::from(file_path);
    let joined = match root {
        Some(root) if raw.is_relative() => root.join(raw),
        _ => raw,
    };
    let canonical = joined.canonicalize().unwrap_or(joined);
    for base in [root, root.and_then(|r| r.canonicalize().ok()).as_deref()] {
        let Some(base) = base else { continue };
        if let Ok(rel) = canonical.strip_prefix(base) {
            return rel.to_string_lossy().into_owned();
        }
    }
    file_path.to_string()
}

pub fn resolve_chunk(chunks: &[Chunk], file_path: &str, line: usize) -> Option<Chunk> {
    let mut fallback = None;
    for chunk in chunks {
        if chunk.file_path == file_path && chunk.start_line <= line && line <= chunk.end_line {
            if line < chunk.end_line {
                return Some(chunk.clone());
            }
            if fallback.is_none() {
                fallback = Some(chunk.clone());
            }
        }
    }
    fallback
}

pub fn trace(message: impl AsRef<str>) {
    if std::env::var_os("SEMBLE_TRACE").is_some() {
        eprintln!("[semble] {}", message.as_ref());
    }
}

pub fn format_results(header: &str, results: &[SearchResult]) -> String {
    let mut out = String::new();
    out.push_str(header);
    out.push('\n');
    out.push('\n');
    for (i, r) in results.iter().enumerate() {
        out.push_str(&format!(
            "## {}. {}  [score={:.3}]\n",
            i + 1,
            r.chunk.location(),
            r.score
        ));
        out.push_str("```\n");
        out.push_str(r.chunk.content.trim());
        out.push_str("\n```\n\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::normalize_file_path;

    #[test]
    fn resolves_absolute_path_inside_root() {
        let root = tempfile::tempdir().expect("tempdir");
        let file = root.path().join("src").join("main.rs");
        std::fs::create_dir_all(file.parent().expect("parent")).expect("mkdir");
        std::fs::write(&file, "fn main() {}").expect("write");
        assert_eq!(
            normalize_file_path(Some(root.path()), &file.to_string_lossy()),
            "src/main.rs"
        );
    }

    #[test]
    fn resolves_relative_path_against_root() {
        let root = tempfile::tempdir().expect("tempdir");
        let file = root.path().join("lib.rs");
        std::fs::write(&file, "").expect("write");
        assert_eq!(normalize_file_path(Some(root.path()), "lib.rs"), "lib.rs");
    }

    #[test]
    fn returns_original_for_nonexistent_absolute_path() {
        assert_eq!(
            normalize_file_path(Some(std::path::Path::new("/nonexistent-root")), "/a/b/c.rs"),
            "/a/b/c.rs"
        );
    }

    #[test]
    fn returns_original_outside_root() {
        let file = tempfile::tempdir().expect("tempdir");
        let inner = file.path().join("other.rs");
        std::fs::write(&inner, "").expect("write");
        assert_eq!(
            normalize_file_path(
                Some(std::path::Path::new("/definitely/not/the/root")),
                &inner.to_string_lossy()
            ),
            inner.to_string_lossy()
        );
    }
}
