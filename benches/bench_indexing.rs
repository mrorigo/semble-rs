// Rust guideline compliant 2026-05-18

//! Indexing performance benchmarks.
//!
//! This benchmark measures the throughput and latency of the lexical (BM25) index construction,
//! semantic embeddings generation, and complete end-to-end repository indexing across
//! different repository sizes.

use criterion::{Criterion, criterion_group, criterion_main};
use tempfile::TempDir;

mod common;

use common::{CorpusConfig, create_mock_corpus};
use semble_rs::index::dense::{embed_chunks, load_model};
use semble_rs::index::engine::SembleIndex;
use semble_rs::index::sparse::build_index;
use semble_rs::types::Chunk;

/// Helper function to load all parsed chunks from a generated corpus.
///
/// # Arguments
///
/// * `temp_dir` - The temporary directory containing the mock corpus.
///
/// # Returns
///
/// A vector of parsed chunks.
fn load_chunks_from_temp_dir(temp_dir: &TempDir) -> Vec<Chunk> {
    let model = load_model(None);
    let index = SembleIndex::from_path(temp_dir.path(), Some(model), None, None, true)
        .expect("Failed to build index for benchmark prep");
    index.chunks
}

/// Benchmarks lexical (BM25) index construction.
///
/// # Arguments
///
/// * `c` - The Criterion context.
pub fn bench_lexical_indexing(c: &mut Criterion) {
    let mut group = c.benchmark_group("indexing/bm25_only");

    // Small Corpus
    let small_dir = create_mock_corpus(CorpusConfig::SMALL).unwrap();
    let small_chunks = load_chunks_from_temp_dir(&small_dir);
    group.bench_function("small", |b| {
        b.iter(|| {
            let bm25 = build_index(&small_chunks);
            let _ = bm25;
        });
    });

    // Medium Corpus
    let med_dir = create_mock_corpus(CorpusConfig::MEDIUM).unwrap();
    let med_chunks = load_chunks_from_temp_dir(&med_dir);
    group.bench_function("medium", |b| {
        b.iter(|| {
            let bm25 = build_index(&med_chunks);
            let _ = bm25;
        });
    });

    // Large Corpus
    let large_dir = create_mock_corpus(CorpusConfig::LARGE).unwrap();
    let large_chunks = load_chunks_from_temp_dir(&large_dir);
    group.bench_function("large", |b| {
        b.iter(|| {
            let bm25 = build_index(&large_chunks);
            let _ = bm25;
        });
    });

    group.finish();
}

/// Benchmarks embedding generation using the loaded model.
///
/// # Arguments
///
/// * `c` - The Criterion context.
pub fn bench_semantic_embedding(c: &mut Criterion) {
    let mut group = c.benchmark_group("indexing/embed_only");
    // Reduce sample size since semantic embedding is heavy
    group.sample_size(10);

    let model = load_model(None);

    // Small Corpus
    let small_dir = create_mock_corpus(CorpusConfig::SMALL).unwrap();
    let small_chunks = load_chunks_from_temp_dir(&small_dir);
    group.bench_function("small", |b| {
        b.iter(|| {
            let embeddings = embed_chunks(&model, &small_chunks);
            assert!(!embeddings.is_empty());
        });
    });

    // Medium Corpus
    let med_dir = create_mock_corpus(CorpusConfig::MEDIUM).unwrap();
    let med_chunks = load_chunks_from_temp_dir(&med_dir);
    group.bench_function("medium", |b| {
        b.iter(|| {
            let embeddings = embed_chunks(&model, &med_chunks);
            assert!(!embeddings.is_empty());
        });
    });

    group.finish();
}

/// Benchmarks complete end-to-end index construction from a directory.
///
/// # Arguments
///
/// * `c` - The Criterion context.
pub fn bench_end_to_end_indexing(c: &mut Criterion) {
    let mut group = c.benchmark_group("indexing/full");
    group.sample_size(10);

    let model = load_model(None);

    // Small Corpus
    let small_dir = create_mock_corpus(CorpusConfig::SMALL).unwrap();
    group.bench_function("small", |b| {
        b.iter(|| {
            let index =
                SembleIndex::from_path(small_dir.path(), Some(model.clone()), None, None, true)
                    .expect("Failed to build index");
            assert!(!index.chunks.is_empty());
        });
    });

    // Medium Corpus
    let med_dir = create_mock_corpus(CorpusConfig::MEDIUM).unwrap();
    group.bench_function("medium", |b| {
        b.iter(|| {
            let index =
                SembleIndex::from_path(med_dir.path(), Some(model.clone()), None, None, true)
                    .expect("Failed to build index");
            assert!(!index.chunks.is_empty());
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_lexical_indexing,
    bench_semantic_embedding,
    bench_end_to_end_indexing
);
criterion_main!(benches);
