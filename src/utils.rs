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

/// The outcome of a chunk lookup for a file path and line number.
#[derive(Debug, Clone, PartialEq)]
pub struct ChunkResolution {
    /// The resolved chunk, either containing the line exactly or the nearest
    /// chunk in the same file when no exact match exists.
    pub chunk: Chunk,
    /// Whether the line falls strictly inside the resolved chunk's range.
    pub exact: bool,
}

/// Resolves a chunk for a file path and line, with a nearest-chunk fallback.
///
/// First attempts an exact resolution via [`resolve_chunk`]. If the file
/// contains indexed chunks but none contain `line`, the chunk whose range is
/// closest to `line` is returned instead (preferring chunks that start at or
/// after the line, since callers typically anchor on declarations). Returns
/// `None` only if the file has no chunks in the index at all.
pub fn resolve_chunk_detailed(
    chunks: &[Chunk],
    file_path: &str,
    line: usize,
) -> Option<ChunkResolution> {
    if let Some(chunk) = resolve_chunk(chunks, file_path, line) {
        let exact = chunk.start_line <= line && line < chunk.end_line;
        return Some(ChunkResolution { chunk, exact });
    }
    chunks
        .iter()
        .filter(|c| c.file_path == file_path)
        .map(|c| {
            let distance = if line < c.start_line {
                c.start_line - line
            } else {
                line.saturating_sub(c.end_line)
            };
            // Prefer later chunks on ties so anchors near a declaration
            // resolve to the following definition.
            (distance, std::cmp::Reverse(c.start_line), c)
        })
        .min_by_key(|(distance, start, _)| (*distance, *start))
        .map(|(_, _, c)| ChunkResolution {
            chunk: c.clone(),
            exact: false,
        })
}

/// Lists the indexed chunk line ranges for a file path.
///
/// Used to produce actionable error output when a lookup cannot be resolved.
pub fn file_chunk_ranges(chunks: &[Chunk], file_path: &str) -> Vec<(usize, usize)> {
    let mut ranges: Vec<(usize, usize)> = chunks
        .iter()
        .filter(|c| c.file_path == file_path)
        .map(|c| (c.start_line, c.end_line))
        .collect();
    ranges.sort_unstable();
    ranges.dedup();
    ranges
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
    use super::{file_chunk_ranges, normalize_file_path, resolve_chunk, resolve_chunk_detailed};
    use crate::types::Chunk;

    fn chunk(file_path: &str, start_line: usize, end_line: usize) -> Chunk {
        Chunk {
            content: String::new(),
            file_path: file_path.to_string(),
            start_line,
            end_line,
            language: None,
        }
    }

    #[test]
    fn detailed_resolution_is_exact_inside_chunk() {
        let chunks = vec![chunk("a.rs", 10, 20)];
        let r = resolve_chunk_detailed(&chunks, "a.rs", 15).expect("resolved");
        assert!(r.exact);
        assert_eq!(r.chunk.start_line, 10);
        // Plain resolve_chunk agrees.
        assert_eq!(resolve_chunk(&chunks, "a.rs", 15), Some(r.chunk));
    }

    #[test]
    fn falls_back_to_nearest_chunk_in_gap() {
        // Equidistant from both chunks; ties prefer the later chunk so
        // declaration anchors resolve forward.
        let chunks = vec![chunk("a.rs", 1, 10), chunk("a.rs", 40, 50)];
        let r = resolve_chunk_detailed(&chunks, "a.rs", 25).expect("resolved");
        assert!(!r.exact);
        assert_eq!(r.chunk.start_line, 40);
    }

    #[test]
    fn falls_back_to_nearest_chunk_past_end() {
        let chunks = vec![chunk("a.rs", 1, 10), chunk("a.rs", 40, 50)];
        let r = resolve_chunk_detailed(&chunks, "a.rs", 60).expect("resolved");
        assert!(!r.exact);
        assert_eq!(r.chunk.start_line, 40);
    }

    #[test]
    fn returns_none_when_file_has_no_chunks() {
        let chunks = vec![chunk("a.rs", 1, 10)];
        assert_eq!(resolve_chunk_detailed(&chunks, "missing.rs", 5), None);
    }

    #[test]
    fn lists_file_chunk_ranges_sorted() {
        let chunks = vec![
            chunk("a.rs", 40, 50),
            chunk("b.rs", 1, 2),
            chunk("a.rs", 1, 10),
            chunk("a.rs", 1, 10),
        ];
        assert_eq!(file_chunk_ranges(&chunks, "a.rs"), vec![(1, 10), (40, 50)]);
        assert!(file_chunk_ranges(&chunks, "nope.rs").is_empty());
    }

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
