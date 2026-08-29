use std::collections::HashMap;

use crate::index::SembleIndex;
use crate::index::engine::SymbolReport;
use crate::types::{Chunk, SearchResult, SymbolKind};

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

/// Maximum number of characters of chunk content emitted per result before
/// truncation. Long enough for most functions; short enough to keep tool
/// output token-friendly.
const MAX_CONTENT_CHARS: usize = 2000;

/// Maps a file extension to a fenced-code-block language tag.
///
/// Returns `None` for unrecognized extensions so no tag is emitted.
fn fence_language(file_path: &str) -> Option<&'static str> {
    let ext = std::path::Path::new(file_path)
        .extension()
        .and_then(|e| e.to_str())?;
    let lang = match ext.to_ascii_lowercase().as_str() {
        "rs" => "rust",
        "py" => "python",
        "js" | "mjs" | "cjs" => "javascript",
        "jsx" => "jsx",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "tsx",
        "go" => "go",
        "java" => "java",
        "c" | "h" => "c",
        "cc" | "cpp" | "cxx" | "hpp" | "hh" => "cpp",
        "cs" => "csharp",
        "rb" => "ruby",
        "php" => "php",
        "swift" => "swift",
        "kt" | "kts" => "kotlin",
        "sh" | "bash" | "zsh" => "shell",
        "md" | "markdown" => "markdown",
        "json" => "json",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        "html" | "htm" => "html",
        "css" | "scss" | "sass" => "css",
        "sql" => "sql",
        _ => return None,
    };
    Some(lang)
}

/// Truncates chunk content to [`MAX_CONTENT_CHARS`] on a char boundary,
/// appending an explicit marker so consumers know more text exists.
fn truncate_content(content: &str) -> String {
    if content.chars().count() <= MAX_CONTENT_CHARS {
        return content.to_string();
    }
    let truncated: String = content.chars().take(MAX_CONTENT_CHARS).collect();
    format!(
        "{}\n... [truncated, showing {} of {} chars]",
        truncated.trim_end(),
        MAX_CONTENT_CHARS,
        content.chars().count()
    )
}

pub fn trace(message: impl AsRef<str>) {
    if std::env::var_os("SEMBLE_TRACE").is_some() {
        eprintln!("[semble] {}", message.as_ref());
    }
}

pub fn format_results(
    header: &str,
    results: &[SearchResult],
    expanded: Option<&HashMap<String, String>>,
) -> String {
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
        match fence_language(&r.chunk.file_path) {
            Some(lang) => out.push_str(&format!("```{}\n", lang)),
            None => out.push_str("```\n"),
        }
        let content = match expanded.and_then(|map| map.get(&r.chunk.location())) {
            Some(snippet) => snippet.clone(),
            None => truncate_content(r.chunk.content.trim()),
        };
        out.push_str(content.as_str());
        out.push_str("\n```\n\n");
    }
    out
}

/// Builds expanded source context for search results when `context_lines` is
/// requested.
///
/// For each result, reads the source window around its chunk from disk via
/// [`SembleIndex::read_snippet_context`] and returns a map keyed by the chunk's
/// `location()` string, ready to pass as the `expanded` argument of
/// [`format_results`]. Results whose files cannot be read from disk are
/// omitted so they fall back to chunk-only rendering.
///
/// # Arguments
///
/// * `index` - The index whose root resolves the source files on disk.
/// * `results` - The search results to expand.
/// * `context_lines` - Number of surrounding lines to include per chunk.
///
/// # Returns
///
/// A map of chunk location to expanded snippet text.
pub fn build_expanded_context(
    index: &SembleIndex,
    results: &[SearchResult],
    context_lines: usize,
) -> HashMap<String, String> {
    results
        .iter()
        .filter_map(|r| {
            let chunk = &r.chunk;
            index
                .read_snippet_context(
                    &chunk.file_path,
                    chunk.start_line,
                    chunk.end_line,
                    context_lines,
                )
                .map(|text| (chunk.location(), text))
        })
        .collect()
}

/// Renders one or more symbol reports into an agent-facing text block.
///
/// Each report shows the symbol name and kind, its definition location(s), and
/// the chunks that reference it, with language-fenced snippets.
pub fn format_symbol_reports(reports: &[SymbolReport]) -> String {
    let mut out = String::new();
    for (i, report) in reports.iter().enumerate() {
        out.push_str(&format!(
            "## Symbol {}: {} ({})\n",
            i + 1,
            report.name,
            match report.definitions.first().map(|d| d.symbol.kind) {
                Some(kind) => kind_name(kind),
                None => "symbol",
            }
        ));
        if !report.definitions.is_empty() {
            out.push_str("\nDefined at:\n");
            for (j, d) in report.definitions.iter().enumerate() {
                out.push_str(&format_code_block(j, d.chunk.location(), &d.chunk));
            }
        }
        if !report.usages.is_empty() {
            out.push_str("Referenced at:\n");
            for (j, u) in report.usages.iter().enumerate() {
                out.push_str(&format_code_block(j, u.chunk.location(), &u.chunk));
            }
        }
        out.push('\n');
    }
    out
}

fn kind_name(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Function => "function",
        SymbolKind::Method => "method",
        SymbolKind::Class => "class",
        SymbolKind::Struct => "struct",
        SymbolKind::Enum => "enum",
        SymbolKind::Trait => "trait",
        SymbolKind::Interface => "interface",
        SymbolKind::Type => "type",
        SymbolKind::Constant => "constant",
        SymbolKind::Module => "module",
        SymbolKind::Unknown => "symbol",
    }
}

fn format_code_block(ordinal: usize, header: String, chunk: &Chunk) -> String {
    let mut out = String::new();
    out.push_str(&format!("## {}. {}\n", ordinal + 1, header));
    match fence_language(&chunk.file_path) {
        Some(lang) => out.push_str(&format!("```{}\n", lang)),
        None => out.push_str("```\n"),
    }
    out.push_str(truncate_content(chunk.content.trim()).as_str());
    out.push_str("\n```\n\n");
    out
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{
        MAX_CONTENT_CHARS, fence_language, file_chunk_ranges, format_results,
        format_symbol_reports, normalize_file_path, resolve_chunk, resolve_chunk_detailed,
        truncate_content,
    };
    use crate::index::engine::{SymbolRef, SymbolReport};
    use crate::types::{Chunk, SearchMode, SearchResult, Symbol, SymbolKind};

    fn chunk(file_path: &str, start_line: usize, end_line: usize) -> Chunk {
        Chunk {
            content: String::new(),
            file_path: file_path.to_string(),
            start_line,
            end_line,
            language: None,
            symbols: Vec::new(),
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
    fn format_symbol_reports_lists_definitions_and_usages() {
        let mut def = chunk("a.rs", 1, 3);
        def.content = "pub fn foo() {}".to_string();
        let mut use_ = chunk("b.rs", 5, 6);
        use_.content = "foo();".to_string();
        let report = SymbolReport {
            name: "foo".to_string(),
            definitions: vec![SymbolRef {
                chunk: def,
                symbol: Symbol {
                    name: "foo".to_string(),
                    kind: SymbolKind::Function,
                    line: 1,
                },
            }],
            usages: vec![SymbolRef {
                chunk: use_,
                symbol: Symbol {
                    name: "foo".to_string(),
                    kind: SymbolKind::Function,
                    line: 5,
                },
            }],
        };
        let out = format_symbol_reports(&[report]);
        assert!(out.contains("## Symbol 1: foo (function)"), "{out}");
        assert!(out.contains("Defined at:"), "{out}");
        assert!(out.contains("a.rs:1-3"), "{out}");
        assert!(out.contains("Referenced at:"), "{out}");
        assert!(out.contains("b.rs:5-6"), "{out}");
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
    fn maps_extensions_to_fence_languages() {
        assert_eq!(fence_language("src/main.rs"), Some("rust"));
        assert_eq!(fence_language("app.py"), Some("python"));
        assert_eq!(fence_language("a.JS"), Some("javascript"));
        assert_eq!(fence_language("data.weird"), None);
        assert_eq!(fence_language("noext"), None);
    }

    #[test]
    fn truncates_long_content_with_marker() {
        let short = truncate_content("hello");
        assert_eq!(short, "hello");
        let long = "x".repeat(MAX_CONTENT_CHARS + 500);
        let out = truncate_content(&long);
        assert!(out.contains("[truncated, showing 2000 of 2500 chars]"));
        assert!(out.len() < long.len());
    }

    #[test]
    fn formats_results_with_language_fence() {
        let result = SearchResult {
            chunk: Chunk {
                content: "fn main() {}".to_string(),
                file_path: "src/main.rs".to_string(),
                start_line: 1,
                end_line: 1,
                language: None,
                symbols: Vec::new(),
            },
            score: 0.5,
            source: SearchMode::Hybrid,
        };
        let out = format_results("header", &[result], None);
        assert!(out.contains("```rust\nfn main() {}"));
    }

    #[test]
    fn formats_results_with_expanded_context() {
        let result = SearchResult {
            chunk: Chunk {
                content: "fn ignored() {}".to_string(),
                file_path: "src/a.rs".to_string(),
                start_line: 3,
                end_line: 3,
                language: None,
                symbols: Vec::new(),
            },
            score: 0.4,
            source: SearchMode::Hybrid,
        };
        let expanded = [(
            "src/a.rs:3-3".to_string(),
            "fn above() {}\n...\nfn body() {}\n...\nfn below() {}".to_string(),
        )]
        .into_iter()
        .collect::<HashMap<_, _>>();
        let out = format_results("header", &[result], Some(&expanded));
        assert!(out.contains("```rust\nfn above() {}",), "{out}");
        assert!(out.contains("fn body() {}"), "{out}");
        assert!(out.contains("fn below() {}"), "{out}");
        assert!(!out.contains("fn ignored()"), "{out}");
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
