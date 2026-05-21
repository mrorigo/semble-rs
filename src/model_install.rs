// Rust guideline compliant 2026-05-21

use serde::{Deserialize, Serialize};

pub const DEFAULT_MODEL_ID: &str = "minishlab/potion-code-16M";
pub const DEFAULT_TOKENIZER_FILENAME: &str = "tokenizer.json";
pub const DEFAULT_EMBEDDINGS_FILENAME: &str = "embeddings.bin";
pub const DEFAULT_WEIGHTS_FILENAME: &str = "weights.bin";
pub const DEFAULT_MANIFEST_FILENAME: &str = "manifest.json";

/// Describes the bundled static model assets.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelManifest {
    /// The model identifier the assets were exported from.
    pub model_id: String,
    /// The embedding dimension of each token vector.
    pub embedding_dim: usize,
    /// The tokenizer vocabulary size.
    pub vocab_size: usize,
    /// The number of token weights stored in the weights buffer.
    pub token_weights_size: usize,
    /// The filenames associated with the model assets.
    pub files: ModelFiles,
}

/// Names of the files that make up a model bundle.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelFiles {
    /// The tokenizer filename.
    pub tokenizer: String,
    /// The embeddings filename.
    pub embeddings: String,
    /// The weights filename.
    pub weights: String,
}

/// Returns the expected bundled filenames for the default model.
pub fn default_model_files() -> [&'static str; 4] {
    [
        DEFAULT_TOKENIZER_FILENAME,
        DEFAULT_EMBEDDINGS_FILENAME,
        DEFAULT_WEIGHTS_FILENAME,
        DEFAULT_MANIFEST_FILENAME,
    ]
}
