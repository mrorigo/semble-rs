// Rust guideline compliant 2026-05-21

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokenizers::Tokenizer;

use crate::model_install::{
    DEFAULT_EMBEDDINGS_FILENAME, DEFAULT_MANIFEST_FILENAME, DEFAULT_MODEL_ID,
    DEFAULT_TOKENIZER_FILENAME, DEFAULT_WEIGHTS_FILENAME, ModelManifest,
};
use crate::types::{Chunk, EMBED_DIM, Encoder};
use crate::utils::trace;

const DEFAULT_TOKENIZER_BYTES: &[u8] = include_bytes!("../../assets/model/tokenizer.json");
const DEFAULT_EMBEDDINGS_BYTES: &[u8] = include_bytes!("../../assets/model/embeddings.bin");
const DEFAULT_WEIGHTS_BYTES: &[u8] = include_bytes!("../../assets/model/weights.bin");
const DEFAULT_MANIFEST_BYTES: &[u8] = include_bytes!("../../assets/model/manifest.json");

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
    Real(Box<BinaryStaticModel>),
    Hashing,
}

#[derive(Debug, Clone)]
struct BinaryStaticModel {
    tokenizer: Tokenizer,
    embeddings: Vec<f32>,
    token_weights: Vec<f32>,
    vocab_size: usize,
    dim: usize,
}

impl BinaryStaticModel {
    fn load(
        tokenizer: &[u8],
        embeddings: &[u8],
        weights: &[u8],
        manifest: &[u8],
    ) -> Result<Self, String> {
        load_binary_model(tokenizer, embeddings, weights, manifest)
    }

    fn load_from_dir(dir: &Path) -> Result<Self, String> {
        let tokenizer_path = dir.join(DEFAULT_TOKENIZER_FILENAME);
        let embeddings_path = dir.join(DEFAULT_EMBEDDINGS_FILENAME);
        let weights_path = dir.join(DEFAULT_WEIGHTS_FILENAME);
        let manifest_path = dir.join(DEFAULT_MANIFEST_FILENAME);

        let tokenizer = fs::read(&tokenizer_path)
            .map_err(|err| format!("failed to read {}: {err}", tokenizer_path.display()))?;
        let embeddings = fs::read(&embeddings_path)
            .map_err(|err| format!("failed to read {}: {err}", embeddings_path.display()))?;
        let weights = fs::read(&weights_path)
            .map_err(|err| format!("failed to read {}: {err}", weights_path.display()))?;
        let manifest = fs::read(&manifest_path)
            .map_err(|err| format!("failed to read {}: {err}", manifest_path.display()))?;

        Self::load(&tokenizer, &embeddings, &weights, &manifest)
    }

    fn row(&self, token_id: usize) -> Option<&[f32]> {
        if token_id >= self.vocab_size {
            return None;
        }
        let start = token_id * self.dim;
        let end = start + self.dim;
        Some(&self.embeddings[start..end])
    }

    fn encode_text(&self, text: &str) -> [f32; EMBED_DIM] {
        let mut out = [0.0f32; EMBED_DIM];
        let Ok(encoding) = self.tokenizer.encode(text, true) else {
            return out;
        };
        let ids = encoding.get_ids();
        if ids.is_empty() {
            return out;
        }

        let mut total_weight = 0.0f32;
        for &raw_id in ids {
            let token_id = raw_id as usize;
            let Some(row) = self.row(token_id) else {
                continue;
            };
            let weight = self
                .token_weights
                .get(token_id)
                .copied()
                .unwrap_or(1.0)
                .max(0.0);
            if weight == 0.0 {
                continue;
            }
            total_weight += weight;
            for (dst, src) in out.iter_mut().zip(row.iter()) {
                *dst += src * weight;
            }
        }

        if total_weight > 0.0 {
            for value in &mut out {
                *value /= total_weight;
            }
        }
        normalize(&mut out);
        out
    }
}

impl StaticModel {
    /// Loads a pre-trained Model2Vec model from embedded bytes or a local directory.
    ///
    /// # Arguments
    ///
    /// * `model_ref` - A local directory path, or the default bundled model identifier.
    ///
    /// # Returns
    ///
    /// Returns a loaded `StaticModel` instance, or falls back to a hashing model
    /// if loading fails.
    pub fn from_pretrained(model_ref: impl AsRef<str>) -> Self {
        let model_ref = model_ref.as_ref();
        let loaded = if model_ref == DEFAULT_MODEL_ID {
            load_default_model()
        } else {
            resolve_model_dir(model_ref).and_then(|dir| BinaryStaticModel::load_from_dir(&dir))
        };

        match loaded {
            Ok(model) => {
                trace(format!("loaded real semantic model from {}", model_ref));
                Self {
                    backend: Arc::new(ModelBackend::Real(Box::new(model))),
                }
            }
            Err(err) => {
                trace(format!(
                    "falling back to hashing encoder for {:?}: {}",
                    model_ref, err
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
        use rayon::prelude::*;
        match self.backend.as_ref() {
            ModelBackend::Real(model) => texts
                .par_iter()
                .map(|text| model.encode_text(text))
                .collect(),
            ModelBackend::Hashing => texts.par_iter().map(|text| hash_embed_text(text)).collect(),
        }
    }
}

/// Helper utility to load the default semantic Model2Vec static representation model.
///
/// # Arguments
///
/// * `model_path` - Optional local directory path. If `None`, loads the bundled model bytes.
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

fn resolve_model_dir(model_ref: &str) -> Result<PathBuf, String> {
    if Path::new(model_ref).is_dir() {
        return Ok(PathBuf::from(model_ref));
    }

    if Path::new(model_ref).is_file() {
        return Path::new(model_ref)
            .parent()
            .map(|path| path.to_path_buf())
            .ok_or_else(|| format!("could not resolve parent directory for {}", model_ref));
    }

    Err(format!(
        "could not find local model assets for {:?}; expected a directory containing {}, {}, {}, and {}",
        model_ref,
        DEFAULT_TOKENIZER_FILENAME,
        DEFAULT_EMBEDDINGS_FILENAME,
        DEFAULT_WEIGHTS_FILENAME,
        DEFAULT_MANIFEST_FILENAME
    ))
}

fn load_default_model() -> Result<BinaryStaticModel, String> {
    BinaryStaticModel::load(
        DEFAULT_TOKENIZER_BYTES,
        DEFAULT_EMBEDDINGS_BYTES,
        DEFAULT_WEIGHTS_BYTES,
        DEFAULT_MANIFEST_BYTES,
    )
}

fn load_binary_model(
    tokenizer_bytes: &[u8],
    embeddings_bytes: &[u8],
    weights_bytes: &[u8],
    manifest_bytes: &[u8],
) -> Result<BinaryStaticModel, String> {
    let tokenizer = Tokenizer::from_bytes(tokenizer_bytes)
        .map_err(|err| format!("failed to load tokenizer from memory: {err}"))?;
    let manifest: ModelManifest = serde_json::from_slice(manifest_bytes)
        .map_err(|err| format!("failed to parse manifest from memory: {err}"))?;

    if manifest.model_id != DEFAULT_MODEL_ID && !manifest.model_id.is_empty() {
        trace(format!(
            "loading model assets exported from {}",
            manifest.model_id
        ));
    }
    if manifest.embedding_dim != EMBED_DIM {
        return Err(format!(
            "manifest embedding_dim {} does not match expected {}",
            manifest.embedding_dim, EMBED_DIM
        ));
    }

    let embeddings = read_f32_bytes(embeddings_bytes)?;
    let token_weights = read_f32_bytes(weights_bytes)?;
    let vocab_size = token_weights.len();
    if manifest.vocab_size != vocab_size {
        return Err(format!(
            "manifest vocab_size {} does not match weight vector length {}",
            manifest.vocab_size, vocab_size
        ));
    }
    if manifest.token_weights_size != token_weights.len() {
        return Err(format!(
            "manifest token_weights_size {} does not match weight vector length {}",
            manifest.token_weights_size,
            token_weights.len()
        ));
    }
    if embeddings.len() != vocab_size * EMBED_DIM {
        return Err(format!(
            "embeddings buffer size {} does not match expected size of vocab_size * dim ({} * {})",
            embeddings.len(),
            vocab_size,
            EMBED_DIM
        ));
    }

    Ok(BinaryStaticModel {
        tokenizer,
        embeddings,
        token_weights,
        vocab_size,
        dim: EMBED_DIM,
    })
}

fn read_f32_bytes(bytes: &[u8]) -> Result<Vec<f32>, String> {
    if !bytes.len().is_multiple_of(4) {
        return Err(format!(
            "buffer length {} is not a multiple of 4",
            bytes.len()
        ));
    }
    let mut out = Vec::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        out.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    Ok(out)
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
    use super::{BinaryStaticModel, DEFAULT_MANIFEST_BYTES, DEFAULT_MODEL_ID, load_binary_model};
    use crate::model_install::{
        DEFAULT_EMBEDDINGS_FILENAME, DEFAULT_MANIFEST_FILENAME, DEFAULT_TOKENIZER_FILENAME,
        DEFAULT_WEIGHTS_FILENAME,
    };

    #[test]
    fn loads_embedded_default_model_from_memory() {
        let model = super::load_default_model().expect("load default model");
        assert_eq!(model.vocab_size, 61826);
        assert_eq!(model.dim, 256);
    }

    #[test]
    fn loads_model_from_memory_buffers() {
        let model = load_binary_model(
            include_bytes!("../../assets/model/tokenizer.json"),
            include_bytes!("../../assets/model/embeddings.bin"),
            include_bytes!("../../assets/model/weights.bin"),
            DEFAULT_MANIFEST_BYTES,
        )
        .expect("load model");
        assert_eq!(model.vocab_size, 61826);
    }

    #[test]
    fn rejects_bad_manifest_dimension() {
        let mut manifest: serde_json::Value =
            serde_json::from_slice(DEFAULT_MANIFEST_BYTES).expect("parse");
        manifest["embedding_dim"] = serde_json::json!(128);
        let manifest = serde_json::to_vec(&manifest).expect("serialize");

        let err = load_binary_model(
            include_bytes!("../../assets/model/tokenizer.json"),
            include_bytes!("../../assets/model/embeddings.bin"),
            include_bytes!("../../assets/model/weights.bin"),
            &manifest,
        )
        .unwrap_err();

        assert!(err.contains("embedding_dim"));
    }

    #[test]
    fn rejects_bad_manifest_vocab_size() {
        let mut manifest: serde_json::Value =
            serde_json::from_slice(DEFAULT_MANIFEST_BYTES).expect("parse");
        manifest["vocab_size"] = serde_json::json!(1);
        let manifest = serde_json::to_vec(&manifest).expect("serialize");

        let err = load_binary_model(
            include_bytes!("../../assets/model/tokenizer.json"),
            include_bytes!("../../assets/model/embeddings.bin"),
            include_bytes!("../../assets/model/weights.bin"),
            &manifest,
        )
        .unwrap_err();

        assert!(err.contains("vocab_size"));
    }

    #[test]
    fn loads_from_local_directory_bytes() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join(DEFAULT_TOKENIZER_FILENAME),
            include_bytes!("../../assets/model/tokenizer.json"),
        )
        .expect("write tokenizer");
        std::fs::write(
            dir.path().join(DEFAULT_EMBEDDINGS_FILENAME),
            include_bytes!("../../assets/model/embeddings.bin"),
        )
        .expect("write embeddings");
        std::fs::write(
            dir.path().join(DEFAULT_WEIGHTS_FILENAME),
            include_bytes!("../../assets/model/weights.bin"),
        )
        .expect("write weights");
        std::fs::write(
            dir.path().join(DEFAULT_MANIFEST_FILENAME),
            include_bytes!("../../assets/model/manifest.json"),
        )
        .expect("write manifest");

        let model = BinaryStaticModel::load_from_dir(dir.path()).expect("load from dir");
        assert_eq!(model.vocab_size, 61826);
        assert_eq!(model.dim, 256);
    }

    #[test]
    fn default_model_id_is_reserved_for_embedded_assets() {
        assert_eq!(DEFAULT_MODEL_ID, "minishlab/potion-code-16M");
    }
}
