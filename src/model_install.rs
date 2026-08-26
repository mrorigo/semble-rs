// Rust guideline compliant 2026-08-26

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::utils::trace;

pub const DEFAULT_MODEL_ID: &str = "minishlab/potion-code-16M";
pub const DEFAULT_TOKENIZER_FILENAME: &str = "tokenizer.json";
pub const DEFAULT_EMBEDDINGS_FILENAME: &str = "embeddings.bin";
pub const DEFAULT_WEIGHTS_FILENAME: &str = "weights.bin";
pub const DEFAULT_MANIFEST_FILENAME: &str = "manifest.json";

const ASSET_BASE_URL: &str =
    "https://raw.githubusercontent.com/mrorigo/semble-rs/main/assets/model";

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

/// Resolves the default user cache directory for storing downloaded models.
pub fn default_cache_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".cache")
        .join("semble")
        .join("models")
        .join("potion-code-16M")
}

/// Checks if all default model assets exist and are non-empty in the given directory.
pub fn are_model_assets_present(dir: &Path) -> bool {
    default_model_files().iter().all(|name| {
        let file_path = dir.join(name);
        match fs::metadata(&file_path) {
            Ok(meta) => meta.is_file() && meta.len() > 0,
            Err(_) => false,
        }
    })
}

/// Ensures default model assets are available locally in `SEMBLE_MODEL_DIR` or the default cache directory,
/// downloading them on-demand if missing.
pub fn ensure_default_model_assets() -> Result<PathBuf, String> {
    if let Some(env_dir) = std::env::var_os("SEMBLE_MODEL_DIR") {
        let env_path = PathBuf::from(env_dir);
        if are_model_assets_present(&env_path) {
            return Ok(env_path);
        }
    }

    let cache_dir = default_cache_dir();
    if are_model_assets_present(&cache_dir) {
        return Ok(cache_dir);
    }

    trace(format!(
        "Default model assets missing in cache. Downloading to {}",
        cache_dir.display()
    ));

    fs::create_dir_all(&cache_dir).map_err(|e| {
        format!(
            "Failed to create model cache directory {}: {e}",
            cache_dir.display()
        )
    })?;

    download_model_assets(&cache_dir)?;
    Ok(cache_dir)
}

/// Downloads all required default model files from the remote asset repository into the destination directory.
pub fn download_model_assets(dest_dir: &Path) -> Result<(), String> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("semble-rs/0.1.1")
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    for filename in default_model_files() {
        let target_path = dest_dir.join(filename);
        let temp_path = dest_dir.join(format!("{}.download", filename));
        let url = format!("{}/{}", ASSET_BASE_URL, filename);

        trace(format!("Downloading {} from {}", filename, url));
        eprintln!("[semble] Downloading {}...", filename);

        let mut response = client
            .get(&url)
            .send()
            .map_err(|e| format!("Failed to download {filename} from {url}: {e}"))?;

        if !response.status().is_success() {
            return Err(format!(
                "Failed to download {filename} from {url}: HTTP {}",
                response.status()
            ));
        }

        let mut file = File::create(&temp_path).map_err(|e| {
            format!(
                "Failed to create temporary file {}: {e}",
                temp_path.display()
            )
        })?;

        std::io::copy(&mut response, &mut file)
            .map_err(|e| format!("Failed to save content to {}: {e}", temp_path.display()))?;

        file.flush()
            .map_err(|e| format!("Failed to flush file {}: {e}", temp_path.display()))?;

        fs::rename(&temp_path, &target_path)
            .map_err(|e| format!("Failed to finalize {}: {e}", target_path.display()))?;
    }

    eprintln!("[semble] Default model assets ready.");
    Ok(())
}
