# Roadmap

Direction for Semble-RS beyond the (now extremely fast and reliable) core search,
chunking, and ranking pipeline. Priorities are ordered by the value they deliver to a
coding agent using this as its repository search backend.

## Top 5 Candidate Features

### 1. Related-code / symbol-graph connectivity

**Priority: Highest**

`find_related` (see `src/mcp.rs`) is anchored on embedding similarity rather than
code connectivity. For an agent the most useful primitive after "find the symbol" is
"what references and defines this symbol across the repo."

Build a lightweight symbol index mapping each identifier to its definitions and
usages, leveraging the existing token splitting in `src/tokens.rs`. This turns every
lookup into a mini code-graph query.

- Outcome: go from "here is a match" to "here are the consumers of this API" in one
  query.
- Reuses: identifier tokenization, the MCP tools, the chunk/embedding pipeline.

### 2. Incremental, persistent indexing

**Priority: High**

The index is currently rebuilt fresh from a path on each run (`from_path` / `from_git`).
Agents re-run searches across many iterations on a living repository.

- Rebuild only files whose mtime/hash changed; keep the rest.
- Persist the index to disk so it is not rebuilt per process/session.
- Closely couples with honoring git state (tracked vs. untracked).

### 3. Git-aware search scoping

**Priority: High**

The walker already honors `.gitignore` / `.sembleignore` (`file_walker.rs`) but does not
distinguish tracked from untracked files or a specific ref/hash.

- Support "search only the current diff / uncommitted work" flows.
- Optionally restrict to tracked code, skipping build artifacts.
- Builds on the existing `from_git` path and `ref_name` plumbing in the CLI.

### 4. Scoped query filters (path / extension / owner)

**Priority: Medium**

Most agent queries do not want whole-corpus results; they want "within `src/index/`",
"only TypeScript", or "only tests".

- Add path, extension, and last-touched-owner filters to `search_semantic`,
  `search_bm25`, and `search_hybrid` (`src/search.rs`).
- The cheapest high-yield change; cuts significant noise in monorepos.

### 5. Agent-facing structured results

**Priority: Medium**

Agents consume search as JSON. Today results are raw chunks with scores.

- Add an agent-facing schema: top-k chunks with `file:start:end`, symbol type,
  definition-vs-usage classification, and a short synthesized rationale.
- Much is partially present (`FindRelatedTool`, `rankScore`); this is an evolution
  rather than new machinery.

## Recommended Order of Investment

1. **#1 (related/graph connectivity)** and **#2 (incremental persistent index)** are the
   strongest additions; both attack the core agent loop of *find → understand connections
   → edit* and build naturally on existing primitives.
2. **#3** and **#4** are cheaper tactical wins.
3. **#5** is a schema/UX layer on top of the retrieval results.
