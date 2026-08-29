# Roadmap

Direction for Semble-RS beyond the (now extremely fast and reliable) core search,
chunking, and ranking pipeline. Priorities are ordered by the value they deliver to a
coding agent using this as its repository search backend.

## Top Candidate Features

### 1. Incremental, persistent indexing

**Priority: High**

The index is currently rebuilt fresh from a path on each run (`from_path` / `from_git`).
Agents re-run searches across many iterations on a living repository.

- Rebuild only files whose mtime/hash changed; keep the rest.
- Persist the index to disk so it is not rebuilt per process/session.
- Closely couples with honoring git state (tracked vs. untracked).

### 2. Git-aware search scoping

**Priority: High**

The walker already honors `.gitignore` / `.sembleignore` (`file_walker.rs`) but does not
distinguish tracked from untracked files or a specific ref/hash.

- Support "search only the current diff / uncommitted work" flows.
- Optionally restrict to tracked code, skipping build artifacts.
- Builds on the existing `from_git` path and `ref_name` plumbing in the CLI.

### 3. Scoped query filters (path / extension / owner)

**Priority: Medium**

Most agent queries do not want whole-corpus results; they want "within `src/index/`",
"only TypeScript", or "only tests".

- Add path, extension, and last-touched-owner filters to `search_semantic`,
  `search_bm25`, and `search_hybrid` (`src/search.rs`).
- The cheapest high-yield change; cuts significant noise in monorepos.

### 4. Agent-facing structured results

**Priority: Medium**

Agents consume search as JSON. Today results are raw chunks with scores.

- Add an agent-facing schema: top-k chunks with `file:start:end`, symbol type,
  definition-vs-usage classification, and a short synthesized rationale.
- Much is partially present (`FindRelatedTool`, `rankScore`); this is an evolution
  rather than new machinery.

## Recommended Order of Investment

1. **#1 (incremental persistent index)** is the strongest remaining addition; it attacks
   the core agent loop of *find → understand connections → edit* and builds naturally on
   existing primitives.
2. **#2** and **#3** are cheaper tactical wins.
3. **#4** is a schema/UX layer on top of the retrieval results.
