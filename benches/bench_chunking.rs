// Rust guideline compliant 2026-05-18

//! Chunking performance benchmarks.
//!
//! This benchmark measures the throughput and latency of tree-sitter based structural
//! chunking versus standard line-based fallback chunking on varying file sizes.

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};

mod common;

use common::{generate_realistic_markdown_file, generate_realistic_rust_file};
use semble_rs::chunking::core::chunk_lines;
use semble_rs::chunking::tree_sitter::chunk as tree_sitter_chunk;

/// Benchmarks Tree-sitter Rust chunking on different input sizes.
///
/// # Arguments
///
/// * `c` - The Criterion context.
pub fn bench_tree_sitter_rust(c: &mut Criterion) {
    let mut group = c.benchmark_group("chunking/tree-sitter-rust");

    for size in &[100, 500, 2000] {
        let content = generate_realistic_rust_file(*size, 42);
        group.bench_with_input(BenchmarkId::from_parameter(size), &content, |b, content| {
            b.iter(|| {
                let res = tree_sitter_chunk(content, "rust", 1500);
                assert!(!res.is_empty());
            });
        });
    }
    group.finish();
}

/// Benchmarks Tree-sitter Markdown chunking on different input sizes.
///
/// # Arguments
///
/// * `c` - The Criterion context.
pub fn bench_tree_sitter_markdown(c: &mut Criterion) {
    let mut group = c.benchmark_group("chunking/tree-sitter-markdown");

    for size in &[100, 500, 2000] {
        let content = generate_realistic_markdown_file(*size, 42);
        group.bench_with_input(BenchmarkId::from_parameter(size), &content, |b, content| {
            b.iter(|| {
                let res = tree_sitter_chunk(content, "markdown", 1500);
                assert!(!res.is_empty());
            });
        });
    }
    group.finish();
}

/// Benchmarks fallback line-based chunking on different input sizes.
///
/// # Arguments
///
/// * `c` - The Criterion context.
pub fn bench_fallback_lines(c: &mut Criterion) {
    let mut group = c.benchmark_group("chunking/fallback-lines");

    for size in &[100, 500, 2000] {
        let content = generate_realistic_rust_file(*size, 42);
        group.bench_with_input(BenchmarkId::from_parameter(size), &content, |b, content| {
            b.iter(|| {
                let res = chunk_lines(content, 1500);
                assert!(!res.is_empty());
            });
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_tree_sitter_rust,
    bench_tree_sitter_markdown,
    bench_fallback_lines
);
criterion_main!(benches);
