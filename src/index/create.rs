// Rust guideline compliant 2026-05-18

use std::fs;
use std::path::Path;

use crate::chunking::chunk_source;
use crate::index::dense::{SelectableBasicBackend, StaticModel, embed_chunks};
use crate::index::file_walker::walk_files;
use crate::index::files::{detect_language, get_extensions};
use crate::index::sparse::build_index;
use crate::types::{Chunk, Encoder};
use crate::utils::trace;

/// Creates a complete code search index from a local path.
///
/// Walks files recursively under `path`, parses their language structures using
/// Tree-sitter structural chunking, encodes chunks using a static Model2Vec model,
/// and builds a BM25 lexical index.
///
/// # Arguments
///
/// * `path` - The root directory path containing code files to index.
/// * `model` - The static model or encoder used to embed the code chunks.
/// * `extensions` - Optional slice of file extension strings to filter the parsed files.
/// * `include_text_files` - If true, text files are parsed as line-split fallbacks.
/// * `display_root` - Optional display root path used to format relative paths.
///
/// # Returns
///
/// Returns a tuple containing:
/// 1. The built lexical BM25 index.
/// 2. The built semantic dense vector search backend.
/// 3. The vector of all discovered chunks.
///
/// # Errors
///
/// Returns an `Err` if:
/// * No files are found, or no supported files can be parsed under `path`.
/// * Disk operations fail during the walk phase.
pub fn create_index_from_path(
    path: &Path,
    model: &impl Encoder,
    extensions: Option<&[&str]>,
    include_text_files: bool,
    display_root: Option<&Path>,
) -> Result<
    (
        crate::index::sparse::Bm25Index,
        SelectableBasicBackend,
        Vec<Chunk>,
    ),
    String,
> {
    trace(format!(
        "create_index_from_path root={} include_text_files={}",
        path.display(),
        include_text_files
    ));
    let exts = get_extensions(include_text_files, extensions);
    let files = walk_files(path, &exts);

    use rayon::prelude::*;

    let chunks: Vec<Chunk> = files
        .into_par_iter()
        .filter_map(|file_path| {
            if let Ok(meta) = fs::metadata(&file_path)
                && meta.len() > 1_000_000
            {
                return None;
            }
            let source = match fs::read_to_string(&file_path) {
                Ok(s) => s,
                Err(_) => return None,
            };
            let rel = display_root
                .and_then(|root| file_path.strip_prefix(root).ok())
                .unwrap_or(&file_path);
            let language = detect_language(&file_path);
            Some(chunk_source(
                &source,
                &rel.to_string_lossy(),
                language.as_deref(),
            ))
        })
        .flatten()
        .collect();

    if chunks.is_empty() {
        return Err(format!(
            "No supported files found under {}.",
            path.display()
        ));
    }
    trace(format!("indexing {} chunks", chunks.len()));
    let embeddings = embed_chunks(model, &chunks);
    let bm25 = build_index(&chunks);
    let semantic = SelectableBasicBackend::new(embeddings);
    Ok((bm25, semantic, chunks))
}

/// Retrieves the default static representation model for code search.
///
/// By default, `semble-rs` utilizes the `minishlab/potion-code-16M` static model.
///
/// # Returns
///
/// A pre-trained, static Model2Vec `StaticModel` instance.
pub fn default_model() -> StaticModel {
    StaticModel::from_pretrained("minishlab/potion-code-16M")
}
