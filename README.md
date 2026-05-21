# Semble-RS

`Semble-RS` is the native Rust implementation of Semble, a code search engine for agents that combines semantic retrieval, BM25 lexical search, Tree-sitter chunking, and code-aware reranking.

It is designed as a practical CLI and MCP server for local repositories and git URLs.

## Highlights

- Native Rust binary with no Python runtime required at execution time
- Semantic search backed by exported `model2vec` assets
- Tree-sitter chunking for structural code boundaries, including Markdown
- BM25 lexical retrieval for exact identifier matches
- Hybrid ranking with reranking heuristics for code search
- CLI and MCP server support
- Model assets embedded for the default Potion model

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
2. **Dynamic Model Customization Boundaries**: In Python, you can easily load any Sentence Transformer or custom static Model2Vec variant on the fly. In `Semble-RS`, swapping models requires conforming to strict binary float layout requirements and fixed dimension sizes (`potion-code-16M` is the hardcoded default).
3. **Automated Remote Git Workspace Clones**: Python `semble` handles full automated downloading, cloning, and caching of remote Git URLs in temporary directories natively. In `Semble-RS`, indexing is highly optimized for local directories; full clone-and-cache automation for remote repositories is a work-in-progress.
4. **Token Savings Logger (`savings.jsonl`)**: Python `semble` logs and appends agent token-efficiency savings (typically ~98% saved) to `~/.semble/savings.jsonl`. This auditing feature is not yet ported to `semble-rs`.

## Repository layout

```text
semble-rs/
├── Cargo.toml
├── README.md
├── AGENTS.md
├── assets/
│   └── model/
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

The Rust encoder uses the exported `potion-code-16M` model assets stored in `assets/model/`.

At runtime, `semble-rs` looks for the following files:

- `tokenizer.json`
- `embeddings.bin`
- `weights.bin`
- `manifest.json`

### Bundled model assets

The default Potion model ships embedded in the binary, so normal startup does not download or write model files to disk.

If you want to use a local override directory, point `SEMBLE_MODEL_DIR` at a directory containing:

- `tokenizer.json`
- `embeddings.bin`
- `weights.bin`
- `manifest.json`

The loader reads those files into memory and does not populate a cache directory.

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

### Initialize the Claude Code agent file

```text
semble init
```

### Inspect token savings

```text
semble savings
semble savings --verbose
```

## MCP server

`semble-rs` naturally runs as an MCP (Model Context Protocol) server over `stdio` when launched without subcommands. It exposes tools to AI agents for:

- `search`
- `find_related`

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

The semantic encoder uses the exported `model2vec` assets:

- `tokenizer.json` for tokenization
- `embeddings.bin` for the token embedding matrix
- `weights.bin` for token importance weights

At query time, text is tokenized, token vectors are looked up, weighted, averaged, and normalized into a 256-dimensional embedding.

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
- Markdown

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

- 🚀 **Sub-5.1ms Semantic Search**: Raw semantic vector retrieval takes just **~5.0 ms** per query via CPU autovectorized SIMD.
- ⚡ **Highly Optimized Hybrid Search**: Fusing lexical BM25 and semantic vectors with code-aware reranking completes in just **~27.7 ms** per query.
- 📂 **Parallelized Rapid Indexing**: End-to-end repository indexing (file walking, parallel Tree-sitter chunking, parallel semantic encoding, and BM25 building) takes only **~1.46 seconds** via Rayon.
- 🌳 **AST Chunker Efficiency**: Tree-sitter Rust chunks a 2,000-line source file structurally in **4.6 ms** (outperforming standard line-splitting by **15.8x**).
- 📈 **Flat Scaling Profile**: Upgrading `top_k` from `5` to `50` adds only **~2.6 ms** of latency, proving downstream ranking phases are extremely cheap.

For complete benchmarks and raw timing charts, refer to the full **[PERFORMANCE_REPORT.md](docs/PERFORMANCE_REPORT.md)**.

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
- `SEMBLE_MODEL_DIR` — point the encoder at a local model asset directory

## Troubleshooting

### The model files are missing

Set `SEMBLE_MODEL_DIR` to a directory that contains the bundled asset files if you want to override the default embedded model.

### Search returns no results

Check that:

- the repository path is correct
- the repository contains supported file types
- the embedded model assets are intact, or `SEMBLE_MODEL_DIR` points at a valid override directory
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
