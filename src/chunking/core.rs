use crate::types::Chunk;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkBoundary {
    pub start: usize,
    pub end: usize,
}

pub fn is_supported_language(language: &str) -> bool {
    matches!(
        language,
        "python"
            | "rust"
            | "javascript"
            | "typescript"
            | "java"
            | "go"
            | "ruby"
            | "cpp"
            | "c"
            | "json"
            | "markdown"
            | "text"
    )
}

pub fn chunk(source: &str, _language: &str, desired_length: usize) -> Vec<ChunkBoundary> {
    chunk_lines(source, desired_length)
}

pub fn chunk_lines(content: &str, desired_length: usize) -> Vec<ChunkBoundary> {
    if content.trim().is_empty() {
        return vec![];
    }
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut current_len = 0usize;
    for (idx, line) in content.split_inclusive('\n').enumerate() {
        let line_len = line.len();
        if current_len > 0 && current_len + line_len > desired_length {
            out.push(ChunkBoundary {
                start,
                end: start + current_len,
            });
            start += current_len;
            current_len = 0;
        }
        current_len += line_len;
        if idx == content.lines().count().saturating_sub(1) {
            out.push(ChunkBoundary {
                start,
                end: start + current_len,
            });
        }
    }
    if out.is_empty() {
        out.push(ChunkBoundary {
            start: 0,
            end: content.len(),
        });
    }
    out
}

pub fn boundaries_to_chunks(
    source: &str,
    file_path: &str,
    language: Option<String>,
    boundaries: Vec<ChunkBoundary>,
) -> Vec<Chunk> {
    boundaries
        .into_iter()
        .map(|boundary| {
            let end_index = boundary.end.saturating_sub(1).max(boundary.start);
            let text = source[boundary.start..=end_index].to_string();
            let start_line = source[..boundary.start]
                .chars()
                .filter(|&c| c == '\n')
                .count()
                + 1;
            let end_line = source[..end_index].chars().filter(|&c| c == '\n').count() + 1;
            Chunk {
                content: text,
                file_path: file_path.to_string(),
                start_line,
                end_line,
                language: language.clone(),
            }
        })
        .collect()
}
