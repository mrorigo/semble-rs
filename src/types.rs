use serde::{Deserialize, Serialize};

/// The fixed dimension of the hashing fallback encoder.
///
/// Real Model2Vec models use their native embedding dimension at runtime; only
/// the deterministic hashing fallback in [`crate::index::model::hash_embed_text`]
/// is fixed at 256 dimensions.
pub const EMBED_DIM: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchMode {
    Hybrid,
    Semantic,
    Bm25,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallType {
    Search,
    FindRelated,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Chunk {
    pub content: String,
    pub file_path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub language: Option<String>,
    /// The declared symbol names found in this chunk (definitions), from the
    /// tree-sitter AST when the language was structurally chunked.
    #[serde(default)]
    pub symbols: Vec<Symbol>,
}

impl Chunk {
    pub fn location(&self) -> String {
        format!("{}:{}-{}", self.file_path, self.start_line, self.end_line)
    }
}

/// The kind of a declared symbol, used to hint agents about its role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    Function,
    Method,
    Class,
    Struct,
    Enum,
    Trait,
    Interface,
    Type,
    Constant,
    Module,
    Unknown,
}

/// A symbol declared by a chunk and its source location.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Symbol {
    /// The lowered identifier, e.g. "calculate_total".
    pub name: String,
    pub kind: SymbolKind,
    /// 1-based declaration line within the file.
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResult {
    pub chunk: Chunk,
    pub score: f32,
    pub source: SearchMode,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexStats {
    pub indexed_files: usize,
    pub total_chunks: usize,
    pub languages: std::collections::BTreeMap<String, usize>,
}

pub trait Encoder: Send + Sync {
    /// Encodes a batch of texts into dense normalized embeddings.
    ///
    /// The inner vectors may have any dimension; callers must not assume a
    /// fixed width. Empty input returns an empty outer vector.
    fn encode(&self, texts: &[String]) -> Vec<Vec<f32>>;
}
