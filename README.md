# Semble-RS

`Semble-RS` is the native Rust implementation of Semble, a code search engine for agents that combines semantic retrieval, BM25 lexical search, Tree-sitter chunking, and code-aware reranking.

It is designed as a practical CLI and MCP server for local repositories and git URLs.

## New in 0.3.0

- **Implicit root resolution**: MCP tools now default `repo` to the server's `--path` or the process working directory when omitted. The resolved source is always surfaced in tool output (e.g. `... in /path (repo omitted; defaulting to /path)`).
- **AST-aware chunk anchoring**: `find_related` snaps a line anchor to the nearest chunk that declares a symbol, skipping doc-comment-only chunks, so results land on syntactic units (functions, structs) rather than `///` comment blocks.
- **Snippet context windowing**: `search` and `find_related` accept an optional `context_lines` (0–200), and the CLI gains `--context-lines <N>`, to render surrounding source around each result.

## Highlights

- Native Rust binary with no Python runtime required at execution time
- Semantic search backed by the official [`model2vec-rs`](https://crates.io/crates/model2vec-rs) crate
- Tree-sitter chunking for structural code boundaries, including Markdown
- BM25 lexical retrieval for exact identifier matches
- Hybrid ranking with reranking heuristics for code search
- CLI and MCP server support with self-describing tools for agents

## Supported Languages

This repository chunks code with **Tree-sitter** for structural AST boundaries in the following languages. Unsupported file types automatically fall back to an optimized line-based chunker.

![Rust](https://img.shields.io/badge/-Rust-000000?style=flat-square&logo=rust&logoColor=white)
![Python](https://img.shields.io/badge/-Python-3776AB?style=flat-square&logo=python&logoColor=white)
![JavaScript](https://img.shields.io/badge/-JavaScript-F7DF1E?style=flat-square&logo=javascript&logoColor=black)
![TypeScript](https://img.shields.io/badge/-TypeScript-3178C6?style=flat-square&logo=typescript&logoColor=white)
![Java](https://img.shields.io/badge/-Java-ED8B00?style=flat-square&logo=openjdk&logoColor=white)
![Go](https://img.shields.io/badge/-Go-00ADD8?style=flat-square&logo=go&logoColor=white)
![C](https://img.shields.io/badge/-C-A8B9CC?style=flat-square&logo=c&logoColor=black)
![C++](https://img.shields.io/badge/-C++-00599C?style=flat-square&logo=cplusplus&logoColor=white)
![Ruby](https://img.shields.io/badge/-Ruby-CC342D?style=flat-square&logo=ruby&logoColor=white)
![Markdown](https://img.shields.io/badge/-Markdown-000000?style=flat-square&logo=markdown&logoColor=white)
![JSON](https://img.shields.io/badge/-JSON-000000?style=flat-square&logo=json&logoColor=white)
![YAML](https://img.shields.io/badge/-YAML-CB171E?style=flat-square&logo=yaml&logoColor=white)

## ⚖️ Comparison with Original Python Semble & Feature Gap

`Semble-RS` is a native Rust port of the Python **[semble](https://github.com/MinishLab/semble)** code-search library. We want to set honest and clear expectations for users transitioning from or choosing between the two implementations.

### 🚀 Where Semble-RS Excels (Rust Advantages)

* **No Python Runtime Required**: Eliminates heavy Python dependency environments, standard library version conflicts, and virtualenv configurations. You get a single, self-contained native binary.
* **Sub-Millisecond Retrieval on Small Corpuses**: Pure semantic searches resolve in **~680 µs** (sub-millisecond!), outperforming Python's ~1.5 ms average.
* **Parallel Indexing via Rayon**: Fully exploits multi-core architectures out-of-the-box. End-to-end repository indexing (including file walking, parallel Tree-sitter AST chunking, parallel model embedding, and lexical BM25 indexing) completes in **~1.46 seconds** on an M1 MacBook Pro (a **40% speed improvement** over the single-threaded baseline).
* **Robust Tree-sitter Structural AST Boundaries**: Integrates precise structural boundary discovery for languages like Rust and Markdown, making retrieved code blocks significantly cleaner and more context-aware compared to standard line-splitters.

### ⚠️ Current Feature Gaps & Limitations

While highly optimized for CLI and MCP agent use-cases, the Rust port currently lacks some features of the parent Python repository:

1. **No Programmatic Python API / FFI Bindings**: The original library is directly importable in Python scripts (`import semble`). `Semble-RS` is built as an executable CLI and MCP server, and does not currently expose PyO3 Python bindings.
2. **Dynamic Model Customization Boundaries**: In Python, you can easily load any Sentence Transformer or custom static Model2Vec variant on the fly. In `Semble-RS`, swapping models is supported for any static Model2Vec variant via `--model` or `SEMBLE_MODEL_DIR` (a local model directory or a Hugging Face repo id), but dynamic transformer architectures (e.g., Sentence Transformers) are not supported.
3. **No File Watching**: Python `semble` watches repositories and automatically rebuilds the index within a session as files change. `semble-rs` re-detects changes on each run (via mtime + content-hash staleness against the persisted cache), but does not yet watch live during a long-running MCP session.
4. **No Programmatic `savings` Accessor API**: Python `semble` exposes an auditing API for agent token savings. `semble-rs` records savings to `.semble/savings.jsonl` and prints reports via `semble savings`, but does not yet expose them through JSON-RPC tools.

## Repository layout

```text
semble-rs/
├── Cargo.toml
├── README.md
├── AGENTS.md
├── assets/
│   └── semble-search.md
└── src/
```

## Quick start

### 1. Install Rust

Install a recent stable Rust toolchain with `rustup` if you do not already have one.

### 2. Build the Rust crate

From the `semble-rs/` directory:

```text
cargo build --release
```

### 3. Install the CLI locally

You can install the binary into your Cargo bin directory with:

```text
cargo install --locked --path .
```

This installs the `semble` executable.

## Model assets

The semantic encoder uses the official [`model2vec-rs`](https://crates.io/crates/model2vec-rs) crate with the `minishlab/potion-code-16M` static embedding model by default.

On first use, model weights are downloaded from the Hugging Face Hub and cached locally; subsequent runs are fully offline. If the model cannot be loaded (e.g., no network and no cache), `semble-rs` falls back to a deterministic hashing encoder with reduced semantic quality.

To override the default model, point `SEMBLE_MODEL_DIR` at either:

- a local directory containing a distilled Model2Vec model, or
- a Hugging Face repo id (e.g., `minishlab/potion-base-8M`)

### Selecting a custom model

Model selection follows a fixed precedence: `SEMBLE_MODEL_DIR` (environment variable), then `--model`, then the default `minishlab/potion-code-16M`.

Pass `--model` to any subcommand that builds an index to choose a different model2vec model for that invocation:

```text
semble search "instantiates a session" ./my-project --model mikeee/m2v-gemma-embedding-300m
semble index status ./my-project --model mikeee/m2v-gemma-embedding-300m
semble --model mikeee/m2v-gemma-embedding-300m --path ./my-project
```

The MCP server also accepts `--model` at startup (shared by every tool; there is no per-tool model parameter). The active model is surfaced in `semble index status` output and in the server's `initialize` description.

semble-rs embeds with the chosen model's **native dimension** (e.g. 768 for `m2v-gemma-embedding-300m`); it never truncates to a fixed width. The on-disk index cache is keyed by the resolved model identity, so switching models never reuses embeddings built by another model.

## CLI

### Search a repository

```text
semble search "authentication flow" ./my-project
```

### Search with a different mode

```text
semble search "save model to disk" ./my-project --mode hybrid
semble search "save model to disk" ./my-project --mode semantic
semble search "save model to disk" ./my-project --mode bm25
```

### Find related code near a file location

```text
semble find-related src/auth.rs 42 ./my-project
```

Both absolute and repository-relative paths are accepted. If no chunk exactly contains the given line, the nearest chunk in the same file is used automatically — pure doc-comment-only chunks are skipped and the anchor snaps to the nearest chunk that declares a symbol (function, struct, etc.), so it lands on a syntactic unit rather than a `///` comment block.

### Show surrounding source context

`search` and `find-related` accept `--context-lines <N>` to also render N lines of surrounding source above and below each matched chunk, so multi-line control flow is visible without follow-up queries:

```text
semble search "auth flow" ./my-project --context-lines 5
semble find-related src/auth.rs 42 ./my-project --context-lines 8
```

### Initialize the Claude Code agent file

```text
semble init
```

### Inspect token savings

```text
semble savings
semble savings --verbose
```

### Trace a symbol

```text
semble symbol add_one ./my-project
semble symbol --file-path src/auth.rs --line 42 ./my-project
```

`symbol` lists a symbol's definition and the chunks that reference it, either by name or by anchoring on a file path and line.

### Manage the on-disk index cache

```text
semble index status ./my-project          # cached? cache dir, size, file count
semble index status ./my-project --build  # also seed/rebuild and show corpus stats
semble index clear ./my-project           # drop one repository's cache
semble index clear --all                  # wipe the entire cache root
semble index cache                        # print cache root and total size
```

The `--include-text-files` flag and `--ref` (for git URLs) form part of the cache key, so use the same flags you would for a search. `semble index clear --all` prompts before deleting unless `--force` is given.

### Bypassing the cache

Every search-style command (`search`, `find-related`, `symbol`) accepts `--no-cache` to skip the on-disk cache and rebuild the index fresh:

```text
semble search "auth" . --no-cache
```

## MCP server

`semble-rs` naturally runs as an MCP (Model Context Protocol) server over `stdio` when launched without subcommands. It exposes tools to AI agents for:

- `search` — semantic, BM25, or hybrid retrieval with mode-specific retry hints on empty results
- `find_related` — related-code lookup from a file path and line, with nearest-chunk fallback and actionable error hints
- `symbol` — trace a symbol to its definition(s) and referencing chunks, either by name or by `file_path` + `line` anchor

All three tools take an optional `repo` (a local directory path or git URL). When `repo` is omitted the server falls back to `--path` if set, otherwise to the server process's current working directory — so a single-root session can omit `repo` entirely. Whenever the source is resolved implicitly, the tool output states it explicitly (e.g. `Search results for: "query" ... in /path (repo omitted; defaulting to /path)`), so agents always know which repository was searched. `search` and `find_related` also accept an optional `context_lines` parameter (0–200, 0 disables) to render surrounding source around each result.

Tool parameters and usage are described in the tool schemas themselves, so MCP clients can discover correct usage without external documentation.

### Example with Claude Code

To add the native `semble-rs` server directly to **Claude Code**, configure it to point to your compiled binary path:

```text
# If installed via cargo install:
claude mcp add semble -s user -- semble

# Or using the absolute path to your release binary:
claude mcp add semble -s user -- /path/to/semble-rs/target/release/semble
```

### Other MCP clients

Any MCP client (such as Cursor or Cline) that can spawn a stdio-based server process can configure `semble-rs` in their settings by specifying the path to the compiled `semble` binary.

## How it works

### Semantic model

The semantic encoder uses the official `model2vec-rs` crate to load the static `potion-code-16M` model (from the local cache, the Hugging Face Hub, or `SEMBLE_MODEL_DIR`). A deterministic token-hashing encoder is used as a fallback when model weights are unavailable.

### Chunking

Code is chunked with Tree-sitter for supported languages:

- Rust
- Python
- JavaScript
- TypeScript
- Java
- Go
- C
- C++
- Ruby
- Markdown
- JSON
- YAML

Unsupported languages fall back to line-based chunking.

### Retrieval

Each search query runs through multiple retrieval paths:

- **BM25** for exact identifier and keyword matching
- **Semantic retrieval** for intent matching
- **Hybrid blending** to combine both scores
- **Reranking** to prioritize definitions, canonical implementations, and file coherence

### Ignore rules

File walking respects common repository noise by default, including:

- `.git`
- `.venv`
- `node_modules`
- `target`
- `.cache`
- `.semble`

It also respects `.gitignore` and `.sembleignore` files where present.

## Development

Run the standard Rust checks from `semble-rs/`:

```text
cargo fmt
cargo clippy -- -D warnings
cargo test
```

For a release-style build:

```text
cargo build --release
```

## Benchmarks

`semble-rs` includes a rigorous benchmarking suite using `criterion` to measure chunking throughput, index build times, and search query latencies (covering lexical, semantic, and hybrid modes).

### Key Performance Metrics (M1 MacBook Pro)

Under a realistic workspace workload (~300 files, ~2,500 chunks):

- 🚀 **Sub-Millisecond BM25 Search**: Lexical retrieval via an inverted postings index completes in just **~0.15 ms** per query on a medium corpus — and scales to only **~0.3 ms** on a large (~124k chunk) corpus.
- ⚡ **Sub-4ms Semantic Search**: Raw semantic vector retrieval takes just **~3.7 ms** per query via CPU autovectorized SIMD.
- 🔀 **Rapid Hybrid Search**: Fusing lexical BM25 and semantic vectors with code-aware reranking completes in just **~5.8 ms** per query.
- 📂 **Parallelized Rapid Indexing**: End-to-end repository indexing (file walking, parallel Tree-sitter chunking, parallel semantic encoding, and BM25 building) takes only **~1.46 seconds** via Rayon.
- 🌳 **AST Chunker Efficiency**: Tree-sitter Rust chunks a 2,000-line source file structurally in **4.6 ms** (outperforming standard line-splitting by **15.8x**).
- 📈 **Flat Scaling Profile**: Upgrading `top_k` from `5` to `50` adds only **~3.7 ms** of latency, proving downstream ranking phases are extremely cheap.

For complete benchmarks and raw timing charts, refer to the full **[PERFORMANCE_REPORT.md](docs/PERFORMANCE_REPORT.md)**.

> **Note on caches:** the figures above measure a *fresh, uncached full build* (the `from_path` path). With the persistent incremental cache enabled (the default), `semble-rs` re-chunks and re-embeds only changed files on subsequent runs, reusing the rest verbatim. In practice, indexing a ~5,500-line repository and answering a query on a warm cache completes in roughly **0.25 s** (cold, fresh cache: ~0.7 s). Search itself is dominated by the same millisecond-scale retrieval costs above.

### Running Benchmarks

To run all benchmarks:

```text
cargo bench
```

To run a specific benchmark target:

```text
cargo bench --bench bench_chunking
cargo bench --bench bench_indexing
cargo bench --bench bench_search
```

### Interactive HTML Reports

After running the benchmarks, Criterion automatically generates interactive HTML reports, charts, and analysis in your `target/criterion/` folder. You can view them using:

```text
open target/criterion/report/index.html
```

### Benchmark Suites

1. **Chunking (`bench_chunking`)**: Measures performance of Tree-sitter AST structural parsing (Rust, Markdown) against the line-based fallback chunker.
2. **Indexing (`bench_indexing`)**: Profiles lexical BM25 builds, semantic embedding generation, and full end-to-end repository indexing across Small, Medium, and Large repository tiers.
3. **Searching (`bench_search`)**: Evaluates query latencies for BM25, semantic brute-force cosine scanning, and hybrid retrieval fusion, sweeping varied `top_k` values to profile scale characteristics.

## Environment variables

The Rust binary recognizes a few useful environment variables:

- `SEMBLE_TRACE=1` — enable trace logging to stderr
- `SEMBLE_MODEL_DIR` — point the encoder at a local static Model2Vec model directory or a Hugging Face repo id (e.g. `minishlab/potion-code-16M`); takes precedence over `--model` and the default
- `SEMBLE_CACHE_DIR` — override the on-disk index cache location (default `~/.semble/index`); set to `none` to disable caching entirely

## Troubleshooting

### The model cannot be loaded

`semble-rs` downloads the default Potion model from the Hugging Face Hub on first use and caches it. If you are offline, either prime the cache once while online or set `SEMBLE_MODEL_DIR` to a local model directory. When no model can be loaded, a reduced-quality hashing encoder is used automatically (check stderr with `SEMBLE_TRACE=1`).

### Search returns no results

Check that:

- the repository path is correct
- the repository contains supported file types
- the Hugging Face model cache is populated (or `SEMBLE_MODEL_DIR` points at a valid local model)
- `SEMBLE_TRACE=1` is set if you want step-by-step runtime logging

### The CLI cannot find `semble`

If you installed from source with `cargo install --path .`, make sure Cargo’s bin directory is on your `PATH`.

## 🙏 Credits & Acknowledgments

`Semble-RS` is a native Rust port and implementation of the outstanding **[Semble](https://github.com/MinishLab/semble)** code-search library created by the incredible team at **[MinishLab](https://github.com/MinishLab)** (pioneered by Stephan Tulkens and Thomas van Dongen).

We want to express our deep appreciation to MinishLab for their groundbreaking work on **[Model2Vec](https://github.com/MinishLab/model2vec)** and their state-of-the-art **Potion** static embedding models.

### Why Model2Vec and Potion?
Traditional dynamic transformer architectures require significant CPU/GPU compute footprints and carry heavy latency bottlenecks, making them impractical for local, real-time agent loops. MinishLab's **Model2Vec** solves this beautifully by distilling dynamic sentence-transformers into lightning-fast, ultra-compact static representations that use pre-computed fixed vectors per token. 

* **Sub-5ms Cosine Scans**: Enables CPU autovectorized SIMD semantic search over large codebases without a Python runtime.
* **Potion Code Model**: By default, `Semble-RS` leverages the highly optimized **`minishlab/potion-code-16M`** static embedding model (hosted on the [Hugging Face Hub](https://huggingface.co/minishlab/potion-code-16M)), delivering extremely high-quality vector representations for code semantics with virtually zero inference latency.
* **Massive Resource Savings**: Enables high-precision local developer agent retrieval with up to **50x smaller size** and **500x faster CPU execution speed** compared to traditional Sentence Transformers.

## License

MIT
