# Semble-RS

`Semble-RS` is the native Rust implementation of Semble, a code search engine for agents that combines semantic retrieval, BM25 lexical search, Tree-sitter chunking, and code-aware reranking.

It is designed as a practical CLI and MCP server for local repositories and git URLs.

## Highlights

- Native Rust binary with no Python runtime required at execution time
- Semantic search backed by exported `model2vec` assets
- Tree-sitter chunking for structural code boundaries
- BM25 lexical retrieval for exact identifier matches
- Hybrid ranking with reranking heuristics for code search
- CLI and MCP server support
- Model assets cached locally and installed on demand

## Repository layout

```text
semble-port/
└── semble-rs/
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

### Install the model cache

To download the assets into the local cache ahead of time:

```text
semble model install
```

You can also force a re-download:

```text
semble model install --force
```

By default, the model cache is stored in a platform-appropriate cache directory. You can override it with:

- `SEMBLE_MODEL_CACHE`

The installer downloads the assets from Hugging Face and stores them in a per-model cache directory.

### First-run behavior

If the model assets are not already present, the Rust loader will try to install them automatically on first use. That keeps the normal user flow simple while still allowing an explicit prefetch step for air-gapped or scripted environments.

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

### Install or refresh model assets

```text
semble model install
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

`semble-rs` also runs as an MCP server over stdio. It exposes tools for:

- `search`
- `find_related`

### Example with Claude Code

If you use `uvx`, the MCP setup is:

```text
claude mcp add semble -s user -- uvx --from "semble[mcp]" semble
```

### Other MCP clients

Any MCP client that can launch a stdio server can use the same command line.

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

## Environment variables

The Rust binary recognizes a few useful environment variables:

- `SEMBLE_TRACE=1` — enable trace logging to stderr
- `SEMBLE_MODEL_CACHE` — override the local model cache directory
- `SEMBLE_MODEL_DIR` — point the encoder at a local model asset directory

## Troubleshooting

### The model download is slow

The first install fetches the tokenizer and model binaries from Hugging Face. Subsequent runs use the local cache.

### Search returns no results

Check that:

- the repository path is correct
- the repository contains supported file types
- the model assets are installed and valid
- `SEMBLE_TRACE=1` is set if you want step-by-step runtime logging

### The CLI cannot find `semble`

If you installed from source with `cargo install --path .`, make sure Cargo’s bin directory is on your `PATH`.

## License

MIT
