use std::fs;
use std::path::Path;

use crate::chunking::chunk_source;
use crate::index::dense::{SelectableBasicBackend, StaticModel, embed_chunks};
use crate::index::file_walker::walk_files;
use crate::index::files::{detect_language, get_extensions};
use crate::index::sparse::build_index;
use crate::types::{Chunk, Encoder};
use crate::utils::trace;

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
    let mut chunks = Vec::new();
    let exts = get_extensions(include_text_files, extensions);
    let files = walk_files(path, &exts);
    trace(format!(
        "walk_files discovered {} candidate files",
        files.len()
    ));
    for file_path in files {
        if let Ok(meta) = fs::metadata(&file_path)
            && meta.len() > 1_000_000
        {
            continue;
        }
        let source = match fs::read_to_string(&file_path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let rel = display_root
            .and_then(|root| file_path.strip_prefix(root).ok())
            .unwrap_or(&file_path);
        let language = detect_language(&file_path);
        chunks.extend(chunk_source(
            &source,
            &rel.to_string_lossy(),
            language.as_deref(),
        ));
    }
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

pub fn default_model() -> StaticModel {
    StaticModel::from_pretrained("minishlab/potion-code-16M")
}
