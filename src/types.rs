use serde::{Deserialize, Serialize};

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
}

impl Chunk {
    pub fn location(&self) -> String {
        format!("{}:{}-{}", self.file_path, self.start_line, self.end_line)
    }
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
    fn encode(&self, texts: &[String]) -> Vec<[f32; EMBED_DIM]>;
}
