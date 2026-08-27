# Plan: Symbol-graph connectivity (ROADMAP #1)

## Problem

`find_related` (`src/mcp.rs:151`, `src/index/engine.rs:163`) is pure embedding
similarity: given chunk X, return semantically-similar chunks in the same language.
It does **not** answer "what defines and uses this symbol", so an agent cannot trace
an identifier to its definition and consumers in one query.

## Goal

Turn lookup into a mini code-graph query:

- a symbol name -> its **definition(s)** and **usage(s)** across the repo, with
  file:start-end locations
- a file+line anchor -> the symbols defined there, plus where each is referenced

## Design

### 1. Definitions captured at chunk time (`src/chunking/tree_sitter.rs`)

The chunker already parses source. Add `extract_definitions(source, language,
start_byte, end_byte) -> Vec<Symbol>` that runs a language-specific tree-sitter
[`Query`] to find declared names inside the chunk's byte range.

```rust
pub struct Symbol {
    pub name: String,       // lowered identifier, e.g. "calculate_total"
    pub kind: SymbolKind,   // Function | Method | Class | Struct | ...
    pub line: usize,        // 1-based declaration line within the file
}
```

Each struct `Chunk` gains `symbols: Vec<Symbol>` (the names declared by its
contained definitions).

Languages with parsers get pattern sets (e.g. from grammar node-types):
- Rust: `function_item`/`name`, `struct_item`/`name`, `enum_item`/`name`,
  `trait_item`/`name`, `impl_item`/`type`, `mod_item`/`name`, `type_item`/`name`
- Python: `function_definition`/`name`, `class_definition`/`name`
- TypeScript/JavaScript: `function_declaration`/`name`, `method_definition`/`name`,
  `class_declaration`/`name`, `interface_declaration`/`name`
- Java: `method_declaration`/`name`, `class_declaration`/`name`, `interface_declaration`/`name`
- Go: `function_declaration`/`name`, `method_declaration`/`name`, `type_declaration`
- C/C++: `function_definition`/`declarator`, `struct_specifier`/`name`
- Ruby: `method`/`name`, `singleton_method`/`name`, `class`/`name`, `module`/`name`

Doc languages (markdown/json/yaml) declare no symbols; extracted sets are empty.

### 2. Symbol index (`src/index/symbols.rs`, new module)

Built once in `SembleIndex::new` from the chunks:

```rust
pub struct SymbolIndex {
    // lowered identifier -> occurrence list
    defs: HashMap<String, Vec<SymbolOccurrence>>,  // declared names
    usages: HashMap<String, Vec<SymbolOccurrence>>, // all other identifier hits
}
pub struct SymbolOccurrence {
    pub chunk_idx: usize,
    pub line: usize,
}
```

- Definitions come from `chunk.symbols` (AST-accurate).
- Usages come from `tokenize(chunk.content)` minus the definitions of that chunk,
  aligned to lines by searching back to the line start.

### 3. Query API (`src/index/engine.rs`)

```rust
impl SembleIndex {
    /// Definitions + usages for a symbol, across the repo.
    pub fn symbol(&self, name: &str) -> SymbolReport;
    /// Symbols declared by the chunk at a file:line anchor, plus their usages.
    pub fn symbols_at(&self, file_path: &str, line: usize) -> Option<SymbolReport>;
}
```

`SymbolReport` groups definition `Chunk`s and usage `Chunk`s, each resolved to its
`SearchResult`-style block for formatting. Uses existing `resolve_chunk_detailed`
for anchor resolution (`src/utils.rs:86`).

### 4. MCP tool (`src/mcp.rs`)

Add a `symbol` tool that accepts either a `name` or a `file_path`+`line` anchor and
emits a graph-shaped result: `Defined as <file>:<start>-<end>` + `Referenced at
<file>:<start>-<end>` for each usage, with language-fenced snippets via the existing
`format_results` / `truncate_content` helpers (`src/utils.rs:191,172`). Enhance
`find_related` output to include the anchor chunk's declared symbols.

## Interfaces & risks

- Break/guarantee: `Chunk` gains a field; all constructions of `Chunk` must be
  updated (chunking, tests, `SearchResult` usage). Gated behind a default `Vec::new()`
  where semantics depend only on content.
- Query API cost: queries run once per chunk at index time. To cap cost, extract only
  for parsed (non-doc) languages, and cap symbols per chunk.
- Backward compatibility: existing `SearchResult`, `find_related`, `search` unchanged;
  the new surface is additive.
- No new RUSTSEC surface: reuses `tree-sitter` (already a dep) and `regex`/`tokens`.

## Verification

- Unit tests: definition extraction per language; usage alignment; `symbol()`
  resolves defs+usages; anchor via `resolve_chunk_detailed`.
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.
- End-to-end: index a small repo, call `symbol` for a function => definition +
  call sites listed.
