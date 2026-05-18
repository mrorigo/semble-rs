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
