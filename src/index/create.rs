// Rust guideline compliant 2026-05-18

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::chunking::chunk_source;
use crate::index::dense::{SelectableBasicBackend, StaticModel, embed_chunks};
use crate::index::file_walker::{FileMeta, walk_files_with_meta};
use crate::index::files::{detect_language, get_extensions};
use crate::index::persist;
use crate::index::sparse::build_index;
use crate::types::{Chunk, Encoder};
use crate::utils::trace;

/// The artifacts produced by an index build: lexical, dense, chunks, and
/// per-file sizes.
type BuiltIndex = (
    crate::index::sparse::Bm25Index,
    SelectableBasicBackend,
    Vec<Chunk>,
    HashMap<String, usize>,
);

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
    let files = crate::index::file_walker::walk_files(path, &exts);

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

/// Chunks an already-read file's bytes into structural chunks.
///
/// Mirrors the per-file filtering of [`create_index_from_path`] (1 MB cap,
/// UTF-8 only, language detection) but for a single file.
///
/// # Arguments
///
/// * `bytes` - The file contents.
/// * `abs_path` - The absolute path of the file (used for language detection).
/// * `display_root` - Root used to make `Chunk::file_path` relative.
///
/// # Returns
///
/// `(relative_path, chunks)` or `None` when the file should be dropped.
fn chunk_file_bytes(
    bytes: &[u8],
    abs_path: &Path,
    display_root: Option<&Path>,
) -> Option<(String, Vec<Chunk>)> {
    if bytes.len() > 1_000_000 {
        return None;
    }
    let source = std::str::from_utf8(bytes).ok()?;
    let rel = display_root
        .and_then(|root| abs_path.strip_prefix(root).ok())
        .unwrap_or(abs_path);
    let language = detect_language(abs_path);
    let chunks = chunk_source(source, &rel.to_string_lossy(), language.as_deref());
    if chunks.is_empty() {
        return None;
    }
    Some((chunks[0].file_path.clone(), chunks))
}

/// One file's chunk group during an incremental rebuild.
///
/// Clean segments are reused verbatim from the prior cache; dirty segments were
/// freshly chunked and still need embeddings.
struct Segment {
    rel: String,
    size: u64,
    mtime_nanos: i128,
    hash: String,
    chunks: Vec<Chunk>,
    embeddings: Option<Vec<Vec<f32>>>,
}

impl Segment {
    /// A clean segment reused from a cached blob slice.
    fn clean(
        rel: String,
        size: u64,
        mtime_nanos: i128,
        hash: String,
        chunks: Vec<Chunk>,
        embeddings: Vec<Vec<f32>>,
    ) -> Self {
        Self {
            rel,
            size,
            mtime_nanos,
            hash,
            chunks,
            embeddings: Some(embeddings),
        }
    }

    /// A freshly chunked (dirty) segment; embeddings are filled after batching.
    fn dirty(rel: String, size: u64, mtime_nanos: i128, hash: String, chunks: Vec<Chunk>) -> Self {
        Self {
            rel,
            size,
            mtime_nanos,
            hash,
            chunks,
            embeddings: None,
        }
    }
}

/// Assembles a final corpus from per-file segments, embedding any dirty segments,
/// then persists the result when `changed` is true.
fn assemble_segments(
    segments: Vec<Segment>,
    path: &Path,
    model: &impl Encoder,
    cdir: Option<&Path>,
    changed: bool,
) -> Result<BuiltIndex, String> {
    // Batch-embed every dirty segment's chunks in one pass.
    let dirty_chunks: Vec<Chunk> = segments
        .iter()
        .filter(|s| s.embeddings.is_none())
        .flat_map(|s| s.chunks.iter().cloned())
        .collect();
    let dirty_embeddings = embed_chunks(model, &dirty_chunks);
    let mut dirty_iter = dirty_embeddings.into_iter();

    let mut final_chunks: Vec<Chunk> = Vec::new();
    let mut final_embeddings: Vec<Vec<f32>> = Vec::new();
    let mut records: Vec<persist::FileRecord> = Vec::new();
    let mut file_sizes: HashMap<String, usize> = HashMap::new();
    let mut manifest_files: HashMap<String, persist::ManifestEntry> = HashMap::new();

    for mut seg in segments {
        if seg.embeddings.is_none() {
            let n = seg.chunks.len();
            let mut embeddings = Vec::with_capacity(n);
            for _ in 0..n {
                if let Some(e) = dirty_iter.next() {
                    embeddings.push(e);
                }
            }
            seg.embeddings = Some(embeddings);
        }
        let count = seg.chunks.len();
        let start = final_chunks.len();
        final_chunks.extend(seg.chunks);
        if let Some(emb) = seg.embeddings {
            final_embeddings.extend(emb);
        }
        if count > 0 {
            records.push(persist::FileRecord {
                path: seg.rel.clone(),
                start,
                count,
            });
            file_sizes.insert(seg.rel.clone(), seg.size as usize);
            manifest_files.insert(
                seg.rel.clone(),
                persist::ManifestEntry {
                    size: seg.size,
                    mtime_nanos: seg.mtime_nanos,
                    hash: seg.hash,
                },
            );
        }
    }

    if final_chunks.is_empty() {
        return Err(format!(
            "No supported files found under {}.",
            path.display()
        ));
    }

    if changed && let Some(dir) = cdir {
        let manifest = persist::Manifest {
            version: persist::CACHE_VERSION,
            files: manifest_files,
        };
        persist::save_blobs(dir, &final_chunks, &final_embeddings, &records, &manifest)
            .map_err(|e| e.to_string())?;
    }

    let bm25 = build_index(&final_chunks);
    let semantic = SelectableBasicBackend::new(final_embeddings);
    Ok((bm25, semantic, final_chunks, file_sizes))
}

/// Creates a code search index, reusing clean files from a persistent cache and
/// re-chunking/embedding only changed files.
///
/// When the cache is disabled or unusable this builds from scratch (and seeds
/// the cache). In all cases file sizes are derived from the walk metadata, so no
/// re-read of file contents is required.
///
/// # Arguments
///
/// * `path` - The repository root directory.
/// * `model` - The encoder used to embed chunks.
/// * `extensions` - Optional extension filter.
/// * `include_text_files` - Whether text files are indexed (part of the cache key).
/// * `display_root` - Root used to make `Chunk::file_path` relative.
/// * `source_key` - Stable cache key for this source (canonical path or `url@ref`).
/// * `model_ref` - Resolved model identity (part of the cache key).
///
/// # Returns
///
/// `(bm25, semantic, chunks, file_sizes)`.
pub fn create_index_incremental(
    path: &Path,
    model: &impl Encoder,
    extensions: Option<&[&str]>,
    include_text_files: bool,
    display_root: Option<&Path>,
    source_key: &str,
    model_ref: &str,
) -> Result<BuiltIndex, String> {
    let exts = get_extensions(include_text_files, extensions);
    let walked = walk_files_with_meta(path, &exts);
    let current: Vec<(PathBuf, FileMeta)> = walked;
    trace(format!(
        "create_index_incremental root={} files={}",
        path.display(),
        current.len()
    ));

    let cdir = persist::cache_dir(source_key, include_text_files, model_ref);

    // Attempt to load the previous cache: manifest + records + blobs.
    let cached = cdir.as_deref().and_then(|dir| {
        let manifest = persist::Manifest::load(dir)?;
        let records = persist::load_file_records(dir)?;
        let blob_len: usize = records.iter().map(|r| r.count).sum();
        let (chunks, embeddings) = persist::load_blobs(dir, blob_len)?;
        Some((manifest, records, chunks, embeddings))
    });

    let Some((manifest, records, blob_chunks, blob_embeddings)) = cached else {
        // No usable cache: full parallel build, then seed the cache.
        return full_build_incremental(path, model, &current, display_root, cdir.as_deref());
    };

    // Build relative metadata for dirty classification.
    let mut rel_meta: Vec<(String, FileMeta)> = Vec::with_capacity(current.len());
    let mut rel_to_abs: HashMap<String, PathBuf> = HashMap::new();
    for (abs, meta) in &current {
        let rel = display_root
            .and_then(|root| abs.strip_prefix(root).ok())
            .unwrap_or(abs)
            .to_string_lossy()
            .to_string();
        rel_meta.push((rel.clone(), *meta));
        rel_to_abs.insert(rel, abs.clone());
    }

    let read = |rel: &str| fs::read(rel_to_abs.get(rel)?).ok();
    let (clean_hashes, dirty_list) = persist::classify_dirty(&Some(manifest), &rel_meta, read);
    let records_by_path: HashMap<String, persist::FileRecord> =
        records.into_iter().map(|r| (r.path.clone(), r)).collect();
    let current_set: std::collections::HashSet<String> =
        rel_meta.iter().map(|(r, _)| r.clone()).collect();
    let deleted = records_by_path.keys().any(|rel| !current_set.contains(rel));
    let changed = !dirty_list.is_empty() || deleted;
    trace(format!(
        "create_index_incremental dirty={} deleted={}",
        dirty_list.len(),
        deleted
    ));

    let mut segments: Vec<Segment> = Vec::new();
    for (rel, meta) in &rel_meta {
        if let (Some(hash), Some(rec)) = (clean_hashes.get(rel), records_by_path.get(rel)) {
            let chunks = blob_chunks[rec.start..rec.start + rec.count].to_vec();
            let embeddings = blob_embeddings[rec.start..rec.start + rec.count].to_vec();
            segments.push(Segment::clean(
                rel.clone(),
                meta.size,
                meta.mtime_nanos,
                hash.clone(),
                chunks,
                embeddings,
            ));
            continue;
        }
        // Dirty or new file: read, chunk, and compute its content hash.
        let Some(abs) = rel_to_abs.get(rel) else {
            continue;
        };
        let Some(bytes) = fs::read(abs).ok() else {
            continue;
        };
        if let Some((_chunk_rel, chunks)) = chunk_file_bytes(&bytes, abs, display_root) {
            let hash = persist::content_hash(&bytes);
            segments.push(Segment::dirty(
                rel.clone(),
                meta.size,
                meta.mtime_nanos,
                hash,
                chunks,
            ));
        }
    }

    assemble_segments(segments, path, model, cdir.as_deref(), changed)
}

/// Builds the index from scratch over `current` files and seeds the cache.
fn full_build_incremental(
    path: &Path,
    model: &impl Encoder,
    current: &[(PathBuf, FileMeta)],
    display_root: Option<&Path>,
    cdir: Option<&Path>,
) -> Result<BuiltIndex, String> {
    use rayon::prelude::*;

    let mut chunked: Vec<Segment> = current
        .par_iter()
        .filter_map(|(abs, meta)| {
            let bytes = fs::read(abs).ok()?;
            let (rel, chunks) = chunk_file_bytes(&bytes, abs, display_root)?;
            Some((rel, chunks, *meta, persist::content_hash(&bytes)))
        })
        .map(|(rel, chunks, meta, hash)| {
            Segment::dirty(rel, meta.size, meta.mtime_nanos, hash, chunks)
        })
        .collect();

    chunked.sort_by(|a, b| a.rel.cmp(&b.rel));

    assemble_segments(chunked, path, model, cdir, true)
}
