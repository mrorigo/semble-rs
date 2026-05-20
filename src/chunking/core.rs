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
        .filter_map(|boundary| {
            let start = previous_char_boundary(source, boundary.start.min(source.len()));
            let end = next_char_boundary(source, boundary.end.min(source.len()));
            if start >= end {
                return None;
            }
            let text = source[start..end].to_string();
            let start_line = source[..start].chars().filter(|&c| c == '\n').count() + 1;
            let end_line = source[..end].chars().filter(|&c| c == '\n').count() + 1;
            Some(Chunk {
                content: text,
                file_path: file_path.to_string(),
                start_line,
                end_line,
                language: language.clone(),
            })
        })
        .collect()
}

fn previous_char_boundary(source: &str, mut index: usize) -> usize {
    while index > 0 && !source.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn next_char_boundary(source: &str, mut index: usize) -> usize {
    while index < source.len() && !source.is_char_boundary(index) {
        index += 1;
    }
    index.min(source.len())
}

#[cfg(test)]
mod tests {
    use super::{ChunkBoundary, boundaries_to_chunks};

    #[test]
    fn clamps_boundaries_to_utf8_char_edges() {
        let source = "aé\nb";
        let chunks = boundaries_to_chunks(
            source,
            "sample.rs",
            Some("rust".to_string()),
            vec![ChunkBoundary { start: 2, end: 4 }],
        );

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].content, "é\n");
        assert_eq!(chunks[0].start_line, 1);
        assert_eq!(chunks[0].end_line, 2);
        assert_eq!(chunks[0].file_path, "sample.rs");
        assert_eq!(chunks[0].language.as_deref(), Some("rust"));
    }

    #[test]
    fn drops_empty_boundaries_after_clamping() {
        let source = "é";
        let chunks = boundaries_to_chunks(
            source,
            "sample.rs",
            None,
            vec![ChunkBoundary { start: 3, end: 3 }],
        );

        assert!(chunks.is_empty());
    }
}
