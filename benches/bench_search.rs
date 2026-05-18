// Rust guideline compliant 2026-05-18

//! Search retrieval performance benchmarks.
//!
//! This benchmark measures the query latency for lexical (BM25), semantic, and hybrid
//! search retrieval paths against pre-built indexes of small, medium, and large repositories.
//! It also sweeps various top-k configurations to assess scaling characteristics.

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};

mod common;

use common::{CorpusConfig, create_mock_corpus};
use semble_rs::index::dense::load_model;
use semble_rs::index::engine::SembleIndex;
use semble_rs::types::SearchMode;

/// Predefined list of benchmark queries simulating realistic developer search patterns.
const BENCH_QUERIES: &[&str] = &[
    "authentication flow",
    "BM25 IDF calculation",
    "save model checkpoint to disk",
    "impl Display for",
    "trait Encoder",
];

/// Benchmarks lexical (BM25) search latency.
///
/// # Arguments
///
/// * `c` - The Criterion context.
pub fn bench_search_bm25(c: &mut Criterion) {
    let mut group = c.benchmark_group("search/bm25");

    let model = load_model(None);

    // Setup indexes
    let small_dir = create_mock_corpus(CorpusConfig::SMALL).unwrap();
    let small_idx =
        SembleIndex::from_path(small_dir.path(), Some(model.clone()), None, true).unwrap();

    let med_dir = create_mock_corpus(CorpusConfig::MEDIUM).unwrap();
    let med_idx = SembleIndex::from_path(med_dir.path(), Some(model.clone()), None, true).unwrap();

    let large_dir = create_mock_corpus(CorpusConfig::LARGE).unwrap();
    let large_idx = SembleIndex::from_path(large_dir.path(), Some(model), None, true).unwrap();

    for &query in BENCH_QUERIES {
        group.bench_with_input(BenchmarkId::new("small", query), query, |b, q| {
            b.iter(|| {
                let results = small_idx.search(q, 10, SearchMode::Bm25, None, None, None);
                // BM25 results can be empty if query terms are absent in mock corpus
                let _ = results.len();
            })
        });

        group.bench_with_input(BenchmarkId::new("medium", query), query, |b, q| {
            b.iter(|| {
                let results = med_idx.search(q, 10, SearchMode::Bm25, None, None, None);
                let _ = results.len();
            })
        });

        group.bench_with_input(BenchmarkId::new("large", query), query, |b, q| {
            b.iter(|| {
                let results = large_idx.search(q, 10, SearchMode::Bm25, None, None, None);
                let _ = results.len();
            })
        });
    }
    group.finish();
}

/// Benchmarks semantic search latency.
///
/// # Arguments
///
/// * `c` - The Criterion context.
pub fn bench_search_semantic(c: &mut Criterion) {
    let mut group = c.benchmark_group("search/semantic");

    let model = load_model(None);

    let small_dir = create_mock_corpus(CorpusConfig::SMALL).unwrap();
    let small_idx =
        SembleIndex::from_path(small_dir.path(), Some(model.clone()), None, true).unwrap();

    let med_dir = create_mock_corpus(CorpusConfig::MEDIUM).unwrap();
    let med_idx = SembleIndex::from_path(med_dir.path(), Some(model.clone()), None, true).unwrap();

    for &query in BENCH_QUERIES {
        group.bench_with_input(BenchmarkId::new("small", query), query, |b, q| {
            b.iter(|| {
                let results = small_idx.search(q, 10, SearchMode::Semantic, None, None, None);
                assert!(!results.is_empty());
            })
        });

        group.bench_with_input(BenchmarkId::new("medium", query), query, |b, q| {
            b.iter(|| {
                let results = med_idx.search(q, 10, SearchMode::Semantic, None, None, None);
                assert!(!results.is_empty());
            })
        });
    }
    group.finish();
}

/// Benchmarks hybrid (RRF + Reranked) search retrieval latency.
///
/// # Arguments
///
/// * `c` - The Criterion context.
pub fn bench_search_hybrid(c: &mut Criterion) {
    let mut group = c.benchmark_group("search/hybrid");

    let model = load_model(None);

    let small_dir = create_mock_corpus(CorpusConfig::SMALL).unwrap();
    let small_idx =
        SembleIndex::from_path(small_dir.path(), Some(model.clone()), None, true).unwrap();

    let med_dir = create_mock_corpus(CorpusConfig::MEDIUM).unwrap();
    let med_idx = SembleIndex::from_path(med_dir.path(), Some(model.clone()), None, true).unwrap();

    for &query in BENCH_QUERIES {
        group.bench_with_input(BenchmarkId::new("small", query), query, |b, q| {
            b.iter(|| {
                let results = small_idx.search(q, 10, SearchMode::Hybrid, None, None, None);
                assert!(!results.is_empty());
            })
        });

        group.bench_with_input(BenchmarkId::new("medium", query), query, |b, q| {
            b.iter(|| {
                let results = med_idx.search(q, 10, SearchMode::Hybrid, None, None, None);
                assert!(!results.is_empty());
            })
        });
    }
    group.finish();
}

/// Benchmarks search retrieval scaling by sweeping the top-k configuration.
///
/// # Arguments
///
/// * `c` - The Criterion context.
pub fn bench_search_topk_sweep(c: &mut Criterion) {
    let mut group = c.benchmark_group("search/topk_sweep");

    let model = load_model(None);
    let med_dir = create_mock_corpus(CorpusConfig::MEDIUM).unwrap();
    let med_idx = SembleIndex::from_path(med_dir.path(), Some(model), None, true).unwrap();

    let query = "save model checkpoint to disk";

    for &top_k in &[5, 10, 25, 50] {
        group.bench_with_input(BenchmarkId::from_parameter(top_k), &top_k, |b, &k| {
            b.iter(|| {
                let results = med_idx.search(query, k, SearchMode::Hybrid, None, None, None);
                assert!(!results.is_empty());
            })
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_search_bm25,
    bench_search_semantic,
    bench_search_hybrid,
    bench_search_topk_sweep
);
criterion_main!(benches);
