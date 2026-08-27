use crate::chunking::core::{
    boundaries_to_chunks, chunk_lines as core_chunk_lines, is_supported_language,
};
use crate::chunking::tree_sitter::chunk_with_symbols;
use crate::types::Chunk;

pub fn chunk_lines(
    content: &str,
    desired_length: usize,
) -> Vec<crate::chunking::core::ChunkBoundary> {
    core_chunk_lines(content, desired_length)
}

pub fn chunk_source(source: &str, file_path: &str, language: Option<&str>) -> Vec<Chunk> {
    if source.trim().is_empty() {
        return vec![];
    }
    let (boundaries, symbols) = match language {
        Some(lang) if is_supported_language(lang) => chunk_with_symbols(source, lang, 1500),
        _ => {
            let bounds = core_chunk_lines(source, 1500);
            let n = bounds.len();
            (bounds, vec![Vec::new(); n])
        }
    };
    boundaries_to_chunks(
        source,
        file_path,
        language.map(|s| s.to_string()),
        boundaries,
        &symbols,
    )
}
