use crate::chunking::core::{
    boundaries_to_chunks, chunk_lines as core_chunk_lines, is_supported_language,
};
use crate::chunking::tree_sitter::chunk as tree_sitter_chunk;
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
    let boundaries = match language {
        Some(lang) if is_supported_language(lang) => tree_sitter_chunk(source, lang, 1500),
        _ => core_chunk_lines(source, 1500),
    };
    boundaries_to_chunks(
        source,
        file_path,
        language.map(|s| s.to_string()),
        boundaries,
    )
}
