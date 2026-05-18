use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokenizers::Tokenizer;

use crate::types::{Chunk, EMBED_DIM, Encoder};
use crate::utils::trace;

const DEFAULT_MODEL_DIR: &str = "assets/model";
const DEFAULT_MODEL_ID: &str = "minishlab/potion-code-16M";
const DEFAULT_TOKENIZER_FILENAME: &str = "tokenizer.json";
const DEFAULT_EMBEDDINGS_FILENAME: &str = "embeddings.bin";
const DEFAULT_WEIGHTS_FILENAME: &str = "weights.bin";

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
    fn load(dir: &Path) -> Result<Self, String> {
        let tokenizer_path = dir.join(DEFAULT_TOKENIZER_FILENAME);
        let embeddings_path = dir.join(DEFAULT_EMBEDDINGS_FILENAME);
        let weights_path = dir.join(DEFAULT_WEIGHTS_FILENAME);

        let tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|err| format!("failed to load tokenizer {}: {err}", tokenizer_path.display()))?;
        let embeddings = read_f32_file(&embeddings_path)?;
        let token_weights = read_f32_file(&weights_path)?;
        if embeddings.is_empty() {
            return Err(format!("{} is empty", embeddings_path.display()));
        }
        if token_weights.is_empty() {
            return Err(format!("{} is empty", weights_path.display()));
        }
        if embeddings.len() % EMBED_DIM != 0 {
            return Err(format!(
                "{} size {} is not divisible by embedding dim {}",
                embeddings_path.display(),
                embeddings.len(),
                EMBED_DIM
            ));
        }
        let vocab_size = embeddings.len() / EMBED_DIM;
        if vocab_size != token_weights.len() {
            return Err(format!(
                "embedding vocab size {} does not match token weights size {}",
                vocab_size,
                token_weights.len()
            ));
        }
        Ok(Self {
            tokenizer,
            embeddings,
            token_weights,
            vocab_size,
            dim: EMBED_DIM,
        })
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
            let weight = self.token_weights.get(token_id).copied().unwrap_or(1.0).max(0.0);
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
    pub fn from_pretrained(model_ref: impl AsRef<str>) -> Self {
        let model_ref = model_ref.as_ref();
        match resolve_model_dir(model_ref).and_then(|dir| BinaryStaticModel::load(&dir)) {
            Ok(model) => {
                trace(format!(
                    "loaded real semantic model from {}",
                    resolve_model_dir(model_ref)
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|_| model_ref.to_string())
                ));
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
        match self.backend.as_ref() {
            ModelBackend::Real(model) => texts.iter().map(|text| model.encode_text(text)).collect(),
            ModelBackend::Hashing => texts.iter().map(|text| hash_embed_text(text)).collect(),
        }
    }
}

pub fn load_model(model_path: Option<&str>) -> StaticModel {
    StaticModel::from_pretrained(model_path.unwrap_or(DEFAULT_MODEL_ID))
}

pub fn embed_chunks(model: &impl Encoder, chunks: &[Chunk]) -> Vec<[f32; EMBED_DIM]> {
    if chunks.is_empty() {
        return vec![];
    }
    model.encode(&chunks.iter().map(|c| c.content.clone()).collect::<Vec<_>>())
}

fn resolve_model_dir(model_ref: &str) -> Result<PathBuf, String> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let candidates = [
        std::env::var_os("SEMBLE_MODEL_DIR").map(PathBuf::from),
        if Path::new(model_ref).is_dir() {
            Some(PathBuf::from(model_ref))
        } else {
            None
        },
        if Path::new(model_ref).is_file() {
            Path::new(model_ref).parent().map(|p| p.to_path_buf())
        } else {
            None
        },
        Some(manifest_dir.join(DEFAULT_MODEL_DIR)),
        Some(manifest_dir.join("assets")),
    ];

    for candidate in candidates.into_iter().flatten() {
        let embeddings = candidate.join(DEFAULT_EMBEDDINGS_FILENAME);
        let weights = candidate.join(DEFAULT_WEIGHTS_FILENAME);
        let tokenizer = candidate.join(DEFAULT_TOKENIZER_FILENAME);
        if embeddings.exists() && weights.exists() && tokenizer.exists() {
            return Ok(candidate);
        }
    }

    Err(format!(
        "could not find model assets for {:?}; expected {}/{{{}, {}, {}}}",
        model_ref,
        DEFAULT_MODEL_DIR,
        DEFAULT_EMBEDDINGS_FILENAME,
        DEFAULT_WEIGHTS_FILENAME,
        DEFAULT_TOKENIZER_FILENAME
    ))
}

fn read_f32_file(path: &Path) -> Result<Vec<f32>, String> {
    let bytes = fs::read(path).map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    if bytes.len() % 4 != 0 {
        return Err(format!(
            "{} length {} is not a multiple of 4",
            path.display(),
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
    use super::read_f32_file;

    #[test]
    fn reads_little_endian_f32_blobs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("data.bin");
        std::fs::write(&path, [0u8, 0, 128, 63, 0, 0, 0, 64]).expect("write");
        let values = read_f32_file(&path).expect("read");
        assert_eq!(values, vec![1.0, 2.0]);
    }
}
