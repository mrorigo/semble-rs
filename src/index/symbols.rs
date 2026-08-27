// Rust guideline compliant 2026-08-27

use std::collections::{HashMap, HashSet};

use regex::Regex;

use crate::types::{Chunk, SymbolKind};

/// A definition occurrence of a symbol in the index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolOccurrence {
    pub chunk_idx: usize,
    /// 1-based declaration line within the file.
    pub line: usize,
    pub kind: SymbolKind,
}

/// A symbol-name to its definitions and referencing chunks.
///
/// Definitions come from the AST-accurate [`crate::chunking::tree_sitter`]
/// extraction; usages are every other occurrence of the identifier across the
/// corpus, collapsed per chunk so lookups answer "where is this referenced"
/// rather than listing every token hit.
#[derive(Debug, Clone, Default)]
pub struct SymbolIndex {
    defs: HashMap<String, Vec<SymbolOccurrence>>,
    usages: HashMap<String, Vec<usize>>,
}

impl SymbolIndex {
    /// Builds the index from the corpus chunks.
    ///
    /// Definitions are read from each chunk's AST-sourced `symbols`; usages are
    /// the lowered identifiers appearing in a chunk's content that the chunk
    /// does not itself define.
    pub fn build(chunks: &[Chunk]) -> Self {
        let mut defs: HashMap<String, Vec<SymbolOccurrence>> = HashMap::new();
        let mut usages: HashMap<String, Vec<usize>> = HashMap::new();

        for (idx, chunk) in chunks.iter().enumerate() {
            let defined: HashSet<&str> = chunk.symbols.iter().map(|s| s.name.as_str()).collect();
            for symbol in &chunk.symbols {
                defs.entry(symbol.name.clone())
                    .or_default()
                    .push(SymbolOccurrence {
                        chunk_idx: idx,
                        line: symbol.line,
                        kind: symbol.kind,
                    });
            }
            let mut chunk_used: HashSet<String> = HashSet::new();
            for used in identifiers_in(chunk) {
                if !defined.contains(used.as_str()) {
                    chunk_used.insert(used);
                }
            }
            for used in chunk_used {
                usages.entry(used).or_default().push(idx);
            }
        }

        Self { defs, usages }
    }

    /// The definition occurrences of a lowered symbol name, if any.
    pub fn definitions(&self, name: &str) -> &[SymbolOccurrence] {
        self.defs.get(name).map(Vec::as_slice).unwrap_or(&[])
    }

    /// The chunk indices that reference (but do not define) a lowered symbol name.
    pub fn referencing_chunks(&self, name: &str) -> &[usize] {
        self.usages.get(name).map(Vec::as_slice).unwrap_or(&[])
    }

    /// True if the symbol name has any known definition or usage.
    pub fn contains(&self, name: &str) -> bool {
        self.defs.contains_key(name) || self.usages.contains_key(name)
    }
}

/// The distinct lowered identifiers appearing in a chunk's content.
fn identifiers_in(chunk: &Chunk) -> HashSet<String> {
    let mut out = HashSet::new();
    for m in IDENT_RE.find_iter(&chunk.content) {
        out.insert(m.as_str().to_lowercase());
    }
    out
}

static IDENT_RE: once_cell::sync::Lazy<Regex> =
    once_cell::sync::Lazy::new(|| Regex::new(r"[a-zA-Z_][a-zA-Z0-9_]*").unwrap());

#[cfg(test)]
mod tests {
    use super::SymbolIndex;
    use crate::types::{Chunk, Symbol, SymbolKind};

    fn chunk(contents: &str, symbols: Vec<(String, SymbolKind, usize)>) -> Chunk {
        Chunk {
            content: contents.to_string(),
            file_path: "f.rs".to_string(),
            start_line: 1,
            end_line: 10,
            language: Some("rust".to_string()),
            symbols: symbols
                .into_iter()
                .map(|(name, kind, line)| Symbol { name, kind, line })
                .collect(),
        }
    }

    #[test]
    fn indexes_definitions_and_usages() {
        let chunks = vec![
            chunk(
                "fn add(a, b) { a + b }\nfn use_add() { add(1, 2) }",
                vec![
                    ("add".to_string(), SymbolKind::Function, 1),
                    ("use_add".to_string(), SymbolKind::Function, 2),
                ],
            ),
            chunk(
                "fn call() { add(3, 4) }",
                vec![("call".to_string(), SymbolKind::Function, 1)],
            ),
        ];
        let index = SymbolIndex::build(&chunks);

        // defs: add defined once in chunk 0
        let add_defs = index.definitions("add");
        assert_eq!(add_defs.len(), 1);
        assert_eq!(add_defs[0].chunk_idx, 0);

        // usages: add referenced by chunk 1
        assert_eq!(index.referencing_chunks("add"), &[1]);
        // use_add referenced nowhere else
        assert!(index.referencing_chunks("use_add").is_empty());
    }

    #[test]
    fn defined_names_are_not_their_own_usages() {
        let chunks = vec![chunk(
            "fn add(a, b) { a + b }",
            vec![("add".to_string(), SymbolKind::Function, 1)],
        )];
        let index = SymbolIndex::build(&chunks);
        assert!(index.referencing_chunks("add").is_empty());
    }

    #[test]
    fn reports_absence() {
        let index = SymbolIndex::build(&[chunk("fn x() {}", vec![])]);
        assert!(!index.contains("missing"));
        assert!(index.definitions("missing").is_empty());
        assert!(index.referencing_chunks("missing").is_empty());
    }
}
