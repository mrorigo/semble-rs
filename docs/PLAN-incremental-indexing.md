# Plan: Incremental, persistent indexing

## Goal

Eliminate the per-process full rebuild. Persist per-file chunks + embeddings to a disk
cache keyed by repository, re-chunk/embed only changed files, and re-derive the cheap
corpus-global indexes on load. Cache is on by default with env/CLI opt-out; covers both
local paths and git URLs.

ROADMAP item: "Incremental, persistent indexing" (rebuilt only files whose mtime/hash
changed; persisted index so it is not rebuilt per process/session).

## Context (verified in source)

- Single funnel: `SembleIndex::from_path`/`from_git` (`src/index/engine.rs:101/122`) →
  `create_index_from_path` (`src/index/create.rs:40`): walk → per-file parallel tree-sitter
  chunking → `embed_chunks` (all chunks) → `Bm25Index::new` → `SelectableBasicBackend::new`.
  No persistence anywhere; the MCP `IndexCache` (`src/mcp.rs:28`) is in-memory only, so every
  fresh process rebuilds the whole index.
- `Chunk` is serde-ready (`src/types.rs:20`). `Bm25Index` (`src/index/sparse.rs:38`),
  `SelectableBasicBackend` (`src/index/dense.rs:8`), and `SymbolIndex`
  (`src/index/symbols.rs:25`) are not serialized, and all three embed positional `usize`
  chunk indices plus corpus-global stats (`avg_len`/`idf`/`n`).
- Per-file chunking and (real-model) embedding are deterministic, so re-chunking only
  changed files is sound. The hashing fallback uses toolchain-dependent `DefaultHasher`.
- `~/.semble/` is the established state directory (`src/stats.rs:10`), and `.semble` is
  already walker-excluded (`src/index/file_walker.rs:18`). Only `serde_json` is present; no
  binary codec.
- `from_git` re-clones a fresh tempdir and leaks it via `mem::forget` each call
  (`src/index/engine.rs:133,151`).

## Design

Persist the immutable per-file facts (chunks + their embeddings) and recompute the cheap
corpus-global aggregations (BM25, symbols, mappings) on load. No in-place index mutation;
the positional-index remapping problem is avoided entirely.

### 1. New module `src/index/persist.rs`

- `cache_dir(root_or_url, ref, include_text_files, model_ref) -> PathBuf`:
  `~/.semble/index/<sha256(canonical_root | ref | include_text_files | model_ref)>/`.
  `SEMBLE_CACHE_DIR` overrides the base; `SEMBLE_CACHE_DIR=none` disables.
- Manifest `files.ser`: per-file `(rel_path, mtime_nanos, size, content_hash)` plus the
  persisted `file_sizes` (so `compute_file_sizes` no longer re-reads every file).
- `store(...)` writes chunks + embeddings + manifest atomically (temp file + rename).
- `load(...)` reads the manifest; chunks/embeddings read lazily on first use.

### 2. Incremental build

Extend `create_index_from_path` into `create_index_incremental`:

1. Walk files (existing `walk_files`). A file is dirty if it is absent from the manifest, or
   its (mtime, size) changed (fast path); the content hash is recomputed only for
   metadata-changed files as the authoritative tiebreaker for same-mtime edits.
2. Clean files → reuse cached chunks + embeddings. Dirty/new files → `chunk_source` +
   `model.encode` (subset only).
3. Drop chunks belonging to deleted files.
4. Recompute `build_index`, `SymbolIndex::build`, `populate_mapping` over the final chunk set.
5. Persist updated chunks + embeddings + manifest when the dirty set is non-empty. Preserve
   the "No supported files" error when the corpus empties.

### 3. Wiring

- `SembleIndex::from_path_cached` / `from_git_cached` (thin wrappers; the uncached
  `from_path`/`from_git` remain for fallback/opt-out).
- `from_git`: keep the fresh shallow clone (correct references), but key the cache by a
  stable `url@ref` and validate the fresh tree against the cached manifest.
- CLI `open_index` (`src/cli.rs:267`) and the MCP `IndexCache::get_blocking` miss path
  (`src/mcp.rs:75`) route through the cached builders. Add `--no-cache` to the
  `Search`/`FindRelated`/`Symbol` subcommands and the MCP args.
- `search`, `symbol`, `symbols_at`, `find_related` are unchanged; they consume the
  recomputed in-memory indexes.

### 4. Model identity in the cache key

The fingerprint includes the resolved model ref (`SEMBLE_MODEL_DIR` or default id), so
changing the model invalidates the cache (embeddings are model-bound). The hashing-fallback
backend defaults to no-cache to avoid toolchain `DefaultHasher` drift.

## Verification

- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.
- New unit tests: dirty/new/unchanged/deleted classification; manifest round-trip;
  fingerprint stability; cache-disabled path; `file_sizes` persistence.
- E2E: run `semble search`/`symbol` twice on a repo → 2nd run reuses the cache (verify via
  trace: no re-embed of unchanged files); edit one file → only that file re-chunked; delete
  a file → usages removed; change `SEMBLE_MODEL_DIR` → cache invalidated; git URL → cached
  across calls keyed by `url@ref`.

## Files touched

- New: `src/index/persist.rs`, `docs/PLAN-incremental-indexing.md`.
- Edit: `src/index/create.rs`, `src/index/engine.rs`, `src/index/file_walker.rs`,
  `src/cli.rs`, `src/mcp.rs`, `Cargo.toml` (add `blake3` for fingerprint + content hash).

## Risks

- Corpus-global recompute (BM25/symbols) on load is O(corpus); negligible vs. embedding but
  should be validated on large trees.
- Hashing-fallback drift → default no-cache on the fallback backend.
- Concurrent writers → atomic rename and last-writer-wins; acceptable for the single-user
  agent flow.
