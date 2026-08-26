// Rust guideline compliant 2026-08-26

use std::path::Path;
use std::sync::Arc;

use model2vec_rs::model::StaticModel as M2VStaticModel;

use crate::types::{Chunk, EMBED_DIM, Encoder};
use crate::utils::trace;

pub const DEFAULT_MODEL_ID: &str = "minishlab/potion-code-16M";

/// A static word representation encoder backed by Model2Vec/Potion models.
///
/// Encodes raw token sequences into dense, normalized floating-point vectors
/// using static token embeddings. Falls back to a deterministic hashing
/// backend if model files cannot be loaded.
#[derive(Clone)]
pub struct StaticModel {
    backend: Arc<ModelBackend>,
}

impl std::fmt::Debug for StaticModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StaticModel").finish_non_exhaustive()
    }
}

enum ModelBackend {
    Real(Box<M2VStaticModel>),
    Hashing,
}

impl StaticModel {
    /// Loads a pre-trained Model2Vec model from Hugging Face Hub or a local path.
    ///
    /// # Arguments
    ///
    /// * `model_ref` - A Hugging Face repo id (e.g. `minishlab/potion-code-16M`) or local folder path.
    ///
    /// # Returns
    ///
    /// Returns a loaded `StaticModel` instance, or falls back to a hashing model
    /// if loading fails.
    pub fn from_pretrained(model_ref: impl AsRef<str>) -> Self {
        let model_ref = model_ref.as_ref();
        let target_ref = if let Some(env_dir) = std::env::var_os("SEMBLE_MODEL_DIR") {
            let env_str = env_dir.to_string_lossy().to_string();
            trace(format!("Using SEMBLE_MODEL_DIR override: {}", env_str));
            env_str
        } else {
            model_ref.to_string()
        };

        let resolved_path = resolve_local_or_hub(&target_ref);
        let loaded = M2VStaticModel::from_pretrained(&resolved_path, None, Some(true), None);

        match loaded {
            Ok(model) => {
                trace(format!("loaded real semantic model from {}", resolved_path));
                Self {
                    backend: Arc::new(ModelBackend::Real(Box::new(model))),
                }
            }
            Err(err) => {
                trace(format!(
                    "falling back to hashing encoder for {:?}: {}",
                    resolved_path, err
                ));
                Self {
                    backend: Arc::new(ModelBackend::Hashing),
                }
            }
        }
    }
}

impl Encoder for StaticModel {
    fn encode(&self, texts: &[String]) -> Vec<[f32; EMBED_DIM]> {
        if texts.is_empty() {
            return vec![];
        }

        match self.backend.as_ref() {
            ModelBackend::Real(model) => {
                let embeddings = model.encode(texts);
                embeddings
                    .into_iter()
                    .map(|vec| {
                        let mut arr = [0.0f32; EMBED_DIM];
                        let copy_len = vec.len().min(EMBED_DIM);
                        arr[..copy_len].copy_from_slice(&vec[..copy_len]);
                        arr
                    })
                    .collect()
            }
            ModelBackend::Hashing => {
                use rayon::prelude::*;
                texts.par_iter().map(|text| hash_embed_text(text)).collect()
            }
        }
    }
}

/// Helper utility to load the default semantic Model2Vec static representation model.
///
/// # Arguments
///
/// * `model_path` - Optional local directory path or HF model id. If `None`, loads `minishlab/potion-code-16M`.
///
/// # Returns
///
/// A constructed `StaticModel` instance.
pub fn load_model(model_path: Option<&str>) -> StaticModel {
    StaticModel::from_pretrained(model_path.unwrap_or(DEFAULT_MODEL_ID))
}

/// Encodes a list of code structural chunks into dense floating-point embeddings.
///
/// # Arguments
///
/// * `model` - The vector encoder used to transform code text into embeddings.
/// * `chunks` - The list of structural code chunks to encode.
///
/// # Returns
///
/// A vector of 256-dimensional normalized floating-point arrays representing each chunk.
pub fn embed_chunks(model: &impl Encoder, chunks: &[Chunk]) -> Vec<[f32; EMBED_DIM]> {
    if chunks.is_empty() {
        return vec![];
    }
    model.encode(&chunks.iter().map(|c| c.content.clone()).collect::<Vec<_>>())
}

fn resolve_local_or_hub(model_ref: &str) -> String {
    let path = Path::new(model_ref);
    if path.exists() {
        return model_ref.to_string();
    }

    let default_assets = Path::new("assets/model");
    if (model_ref == DEFAULT_MODEL_ID || model_ref.is_empty())
        && default_assets.exists()
        && default_assets.join("model.safetensors").exists()
    {
        return default_assets.to_string_lossy().to_string();
    }

    model_ref.to_string()
}

fn normalize(values: &mut [f32; EMBED_DIM]) {
    let norm = values.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm > 0.0 {
        for value in values.iter_mut() {
            *value /= norm;
        }
    }
}

/// Computes a deterministic token-hashing dense representation fallback for input text.
///
/// Used when static Model2Vec weights or model tokenizers are not available.
///
/// # Arguments
///
/// * `text` - The input string to embed.
///
/// # Returns
///
/// A normalized 256-dimensional floating point representation.
pub fn hash_embed_text(text: &str) -> [f32; EMBED_DIM] {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut vec = [0.0f32; EMBED_DIM];
    let tokens = crate::tokens::tokenize(text);
    if tokens.is_empty() {
        return vec;
    }
    for token in tokens {
        let mut hasher = DefaultHasher::new();
        token.hash(&mut hasher);
        let idx = (hasher.finish() as usize) % EMBED_DIM;
        vec[idx] += 1.0;
    }
    normalize(&mut vec);
    vec
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_MODEL_ID, StaticModel};
    use crate::types::Encoder;

    #[test]
    fn default_model_id_is_potion_code() {
        assert_eq!(DEFAULT_MODEL_ID, "minishlab/potion-code-16M");
    }

    #[test]
    fn hashing_encoder_produces_normalized_embeddings() {
        let texts = vec![
            "fn calculate_hash() -> u64".to_string(),
            "struct UserAccount { id: u64 }".to_string(),
        ];
        let model = StaticModel::from_pretrained("non-existent-model-fallback");
        let embeddings = model.encode(&texts);
        assert_eq!(embeddings.len(), 2);
        for emb in embeddings {
            let norm = emb.iter().map(|v| v * v).sum::<f32>().sqrt();
            assert!((norm - 1.0).abs() < 1e-4);
        }
    }
}
