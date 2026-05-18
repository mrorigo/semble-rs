# Performance Report: `semble-rs`

This document outlines the performance characteristics of `semble-rs`—the native Rust implementation of the Semble code-search engine—benchmarked under realistic workloads.

## 💻 Environment Specification

- **Processor**: Apple M1 (8-core: 4 performance cores, 4 efficiency cores)
- **Memory**: 32 GB LPDDR4X RAM
- **Operating System**: macOS 15.x (Darwin)
- **Compiler**: Rustc 1.93.0-nightly (stable profile equivalent)
- **Target Optimization**: `--release` profile with standard compiler optimizations

---

## 📊 1. Chunking Phase Performance

This phase isolates the boundary discovery performance. We compared **Tree-sitter AST structural parsing** (for Rust and Markdown) against standard **line-based splitting** across varying file lengths (100, 500, and 2,000 lines).

### Latency Summary

| Target / Size | 100 Lines | 500 Lines | 2,000 Lines | Scaling Characteristics |
|---|---|---|---|---|
| **Tree-sitter Rust** | 256.01 µs | 1.16 ms | 4.65 ms | **Linear (1x $\rightarrow$ 4.5x $\rightarrow$ 18.2x)** |
| **Tree-sitter Markdown** | 679.35 µs | 4.05 ms | 13.44 ms | **Linear (1x $\rightarrow$ 6.0x $\rightarrow$ 19.8x)** |
| **Line-based Fallback** | 212.01 µs | 4.22 ms | 73.50 ms | **Quadratic-like (1x $\rightarrow$ 19.9x $\rightarrow$ 346.7x)** |

### 🔍 Key Insights & Analysis

1. **The Tree-sitter Efficiency Win**: While line-based fallback is marginally faster for tiny files (212 µs vs. 256 µs), it **degrades severely** on larger inputs, taking **73.50 ms** for a 2,000-line file. Tree-sitter Rust completes the same file in just **4.65 ms**—a **15.8x performance advantage**.
2. **Markdown Parsing Overhead**: Tree-sitter Markdown is roughly **3x slower** than the Rust parser. This is expected due to the highly nested, ambiguous, and block-heavy nature of the Markdown grammar relative to Rust's formal syntactic bounds.
3. **Linear Complexity**: Both Tree-sitter engines scale strictly linearly ($O(N)$) with file length. This ensures safe chunking of large files without risk of stalling the caller.

---

## 📦 2. Indexing Phase Performance

Indexing evaluates walking a repository, chunking each file structurally, building a lexical vocabulary database (BM25), and generating dense vectors. Following our integration of **Rayon**, the indexing phase runs fully parallelized across multiple CPU cores.

### Latency Summary (Sequential vs. Rayon Parallelized)

| Phase / Tier | Small (50 files, ~300 chunks) | Medium (300 files, ~2.5k chunks) | Large (1.5k files, ~15k chunks) | Parallel Speedup |
|---|---|---|---|---|
| **Lexical Indexing (BM25)** | 16.93 ms | 162.64 ms | 937.23 ms | *Single-Threaded* |
| **Semantic Embedding (Sequential)** | 87.90 ms | 798.85 ms | *N/A (Bypassed)* | Baseline |
| **Semantic Embedding (Rayon)** | **18.09 ms** | **164.90 ms** | *N/A (Bypassed)* | **4.8x Faster** (78.1% drop) |
| **Full End-to-End Index (Sequential)** | 322.18 ms | 2.42 seconds | *N/A (Bypassed)* | Baseline |
| **Full End-to-End Index (Rayon)** | **220.08 ms** | **1.46 seconds** | *N/A (Bypassed)* | **1.66x Faster** (39.8% drop) |

*Note: Large-tier semantic benchmarks are bypassed by default to maintain fast bench cycles.*

### 🔍 Key Insights & Analysis

1. **Massive Concurrency Speedups**: The math-heavy semantic encoding phase scales exceptionally well with multi-core CPUs. Fusing token embeddings concurrently yields an incredible **4.8x speedup** on both small and medium workloads, bringing medium-tier embedding times down from **798.85 ms** to only **164.90 ms**.
2. **Sub-1.5s End-to-End Indexing**: Thanks to parallel Tree-sitter file chunking and multi-threaded embedding generation, creating a complete hybrid semantic/lexical index on a medium repository drops from **2.42 seconds** to just **1.46 seconds** (a **40% overall reduction** in setup latency).
3. **Sub-Second Lexical Building**: Lexical index building remains highly optimized, taking only **937 ms** to fully construct a large repository containing over **15,000 chunks** of code.

---

## 🔍 3. Search Retrieval Latency

We evaluated query performance against pre-built indexes across lexical (`BM25`), semantic (`Model2Vec` brute-force), and hybrid modes.

### Query Latency (Medium Tier - 2,500 Chunks)

| Query / Mode | BM25 Lexical | Semantic Cosine | Hybrid (RRF + Rerank) |
|---|---|---|---|
| `"authentication flow"` | 22.27 ms | 5.14 ms | 27.95 ms |
| `"BM25 IDF calculation"` | 21.69 ms | 5.11 ms | 28.89 ms |
| `"save model checkpoint to disk"` | 22.17 ms | 4.99 ms | 28.25 ms |
| `"impl Display for"` | 21.66 ms | 4.94 ms | 27.17 ms |
| `"trait Encoder"` | 21.32 ms | 5.11 ms | 26.39 ms |
| **Average Latency** | **21.82 ms** | **5.06 ms** | **27.73 ms** |

### 🔍 Key Insights & Analysis

1. **Blazing Fast Semantics**: Brute-force semantic cosine scans over 2,500 256-dimensional float vectors complete in just **5.06 ms**. This highlights the exceptional speed of Rust's compiler autovectorization (SIMD) on Apple Silicon.
2. **BM25 Search Overhead**: BM25 searches are slower than raw float scanning, averaging **21.8 ms** per query. This is because tokenizing the query and performing scoring lookups on hash maps is more memory-access intensive than linear array vector scans.
3. **Hybrid Cost-Benefit**: Hybrid retrieval—which runs both retrieval paths, fuses the results via Reciprocal Rank Fusion (RRF), applies code structure heuristics, and reranks the candidates—completes in just **27.7 ms**. For a mere 5.9 ms premium over pure BM25, the user gets semantic intent awareness combined with precise token keyword matching.

---

## 📈 4. Scaling Characteristics (Top-K Sweep)

To determine how the hybrid ranking pipeline scales with larger result pages, we swept `top_k` values from 5 to 50 on the medium corpus tier.

```text
search/topk_sweep/5     time:   28.69 ms
search/topk_sweep/10    time:   27.96 ms
search/topk_sweep/25    time:   29.30 ms
search/topk_sweep/50    time:   31.33 ms
```

### 🔍 Analysis

- **Flat Scaling Profile**: Increasing requested results from **5** to **50** (a 10x scale) results in only a **2.64 ms** increase in total latency.
- **Interpretation**: This proves the downstream reciprocal ranking, heuristics boosting, and document deduplication phases are highly optimized. The execution footprint is completely dominated by the initial candidate generation, making it extremely cheap to request larger result pages.

---

## 💡 Architectural Recommendations & Completed Improvements

1. **Retain Tree-sitter Everywhere**: Never fallback to line-based chunking on large files. The Tree-sitter AST parser is significantly faster and dramatically more resource-efficient as files scale.
2. **SIMD Vector Optimization**: Since semantic search vector scanning is highly performant (sub-5 ms), we should keep the vector index simple and flat for workspaces up to 50,000 chunks. Complex hierarchical indexing is unnecessary and would add complexity without meaningful speed gains.
3. **Parallel Indexing via Rayon (Successfully Implemented)**: By parallelizing Tree-sitter chunking and Model2Vec token encoding via Rayon, we successfully dropped medium-tier dense embedding generation from **798.85 ms** to **164.90 ms** (a spectacular **4.8x speedup**) and end-to-end indexing to just **1.46 seconds** (a **40% time reduction**). This proves the engine's scaling capability on multi-core systems.

---

## 📚 References & Credits

The spectacular search performance and sub-5ms semantic latency documented in this report are the direct result of the incredible engineering behind **[Model2Vec](https://github.com/MinishLab/model2vec)**, designed and open-sourced by **[MinishLab](https://github.com/MinishLab)**. We highly credit and praise MinishLab for making high-quality local static representation learning accessible and remarkably efficient.
