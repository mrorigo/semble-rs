use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::utils::trace;

pub const DEFAULT_MODEL_ID: &str = "minishlab/potion-code-16M";
const MODEL_FILES: [&str; 4] = [
    "tokenizer.json",
    "embeddings.bin",
    "weights.bin",
    "manifest.json",
];
const BUNDLED_MODEL_FILES: [(&str, &[u8]); 4] = [
    (
        "tokenizer.json",
        include_bytes!("../assets/model/tokenizer.json"),
    ),
    (
        "embeddings.bin",
        include_bytes!("../assets/model/embeddings.bin"),
    ),
    ("weights.bin", include_bytes!("../assets/model/weights.bin")),
    (
        "manifest.json",
        include_bytes!("../assets/model/manifest.json"),
    ),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelManifest {
    pub model_id: String,
    pub embedding_dim: usize,
    pub vocab_size: usize,
    pub token_weights_size: usize,
    pub files: ModelFiles,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelFiles {
    pub tokenizer: String,
    pub embeddings: String,
    pub weights: String,
}

pub fn cache_root() -> PathBuf {
    if let Some(dir) = std::env::var_os("SEMBLE_MODEL_CACHE") {
        return PathBuf::from(dir);
    }
    if let Some(dir) = std::env::var_os("XDG_CACHE_HOME") {
        return PathBuf::from(dir).join("semble").join("model");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home)
            .join(".cache")
            .join("semble")
            .join("model");
    }
    if let Some(localappdata) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(localappdata).join("semble").join("model");
    }
    PathBuf::from(".semble").join("model")
}

pub fn model_cache_dir(model_id: &str) -> PathBuf {
    cache_root().join(safe_model_name(model_id))
}

pub fn ensure_model_installed(model_id: &str, force: bool) -> Result<PathBuf, String> {
    let dest = model_cache_dir(model_id);
    if is_complete_install(&dest) && !force {
        return Ok(dest);
    }
    install_model(model_id, &dest, force)?;
    Ok(dest)
}

pub fn install_model(model_id: &str, destination: &Path, force: bool) -> Result<(), String> {
    if destination.exists() {
        if force {
            fs::remove_dir_all(destination)
                .map_err(|e| format!("failed to clear {}: {e}", destination.display()))?;
        } else if is_complete_install(destination) {
            return Ok(());
        }
    }
    fs::create_dir_all(destination)
        .map_err(|e| format!("failed to create {}: {e}", destination.display()))?;

    trace(format!(
        "installing model {} into {}",
        model_id,
        destination.display()
    ));
    let base_url = format!("https://huggingface.co/{}/resolve/main", model_id);
    for filename in ["tokenizer.json", "embeddings.bin", "weights.bin"] {
        let url = format!("{}/{}?download=1", base_url, filename);
        let path = destination.join(filename);
        if let Err(err) = download_to_path(&url, &path) {
            trace(format!(
                "download failed for {}: {}; falling back to bundled assets",
                filename, err
            ));
            return install_from_bundled_assets(model_id, destination, err);
        }
    }

    let manifest = ModelManifest {
        model_id: model_id.to_string(),
        embedding_dim: 256,
        vocab_size: read_vocab_size(&destination.join("embeddings.bin"))?,
        token_weights_size: read_f32_count(&destination.join("weights.bin"))?,
        files: ModelFiles {
            tokenizer: "tokenizer.json".to_string(),
            embeddings: "embeddings.bin".to_string(),
            weights: "weights.bin".to_string(),
        },
    };
    write_manifest(destination, &manifest)?;
    Ok(())
}

fn install_from_bundled_assets(
    model_id: &str,
    destination: &Path,
    download_error: String,
) -> Result<(), String> {
    if model_id != DEFAULT_MODEL_ID {
        return Err(format!(
            "{}; bundled fallback is only available for {}",
            download_error, DEFAULT_MODEL_ID
        ));
    }

    for (filename, contents) in BUNDLED_MODEL_FILES {
        let dst = destination.join(filename);
        fs::write(&dst, contents).map_err(|e| {
            format!(
                "{}; fallback failed while writing embedded asset {} to {}: {}",
                download_error,
                filename,
                dst.display(),
                e
            )
        })?;
    }

    trace(format!(
        "installed fallback bundled model assets into {}",
        destination.display()
    ));
    Ok(())
}

pub fn is_complete_install(dir: &Path) -> bool {
    MODEL_FILES.iter().all(|name| dir.join(name).exists())
}

pub fn read_manifest(dir: &Path) -> Result<ModelManifest, String> {
    let path = dir.join("manifest.json");
    let text =
        fs::read_to_string(&path).map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("failed to parse {}: {e}", path.display()))
}

fn write_manifest(dir: &Path, manifest: &ModelManifest) -> Result<(), String> {
    let path = dir.join("manifest.json");
    let mut file =
        fs::File::create(&path).map_err(|e| format!("failed to create {}: {e}", path.display()))?;
    let payload = serde_json::to_string_pretty(manifest)
        .map_err(|e| format!("failed to serialize manifest: {e}"))?;
    file.write_all(payload.as_bytes())
        .and_then(|_| file.write_all(b"\n"))
        .map_err(|e| format!("failed to write {}: {e}", path.display()))
}

fn download_to_path(url: &str, destination: &Path) -> Result<(), String> {
    trace(format!("downloading {} -> {}", url, destination.display()));
    let response = reqwest::blocking::get(url)
        .map_err(|e| format!("failed to request {url}: {e}"))?
        .error_for_status()
        .map_err(|e| format!("download failed for {url}: {e}"))?;
    let bytes = response
        .bytes()
        .map_err(|e| format!("failed to read {url}: {e}"))?;
    fs::write(destination, &bytes)
        .map_err(|e| format!("failed to write {}: {e}", destination.display()))
}

fn read_f32_count(path: &Path) -> Result<usize, String> {
    let bytes = fs::read(path).map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    if bytes.len() % 4 != 0 {
        return Err(format!(
            "{} length {} is not a multiple of 4",
            path.display(),
            bytes.len()
        ));
    }
    Ok(bytes.len() / 4)
}

fn read_vocab_size(embeddings_path: &Path) -> Result<usize, String> {
    let bytes = fs::read(embeddings_path)
        .map_err(|e| format!("failed to read {}: {e}", embeddings_path.display()))?;
    if bytes.len() % (4 * 256) != 0 {
        return Err(format!(
            "{} length {} is not divisible by 256-d float32 rows",
            embeddings_path.display(),
            bytes.len()
        ));
    }
    Ok(bytes.len() / (4 * 256))
}

fn safe_model_name(model_id: &str) -> String {
    model_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_MODEL_ID, install_from_bundled_assets, read_manifest, safe_model_name};

    #[test]
    fn sanitizes_model_name() {
        assert_eq!(
            safe_model_name("minishlab/potion-code-16M"),
            "minishlab_potion-code-16M"
        );
    }

    #[test]
    fn installs_bundled_assets_for_default_model() {
        let dir = tempfile::tempdir().unwrap();

        install_from_bundled_assets(DEFAULT_MODEL_ID, dir.path(), "download failed".to_string())
            .unwrap();

        for filename in [
            "tokenizer.json",
            "embeddings.bin",
            "weights.bin",
            "manifest.json",
        ] {
            assert!(dir.path().join(filename).exists(), "missing {filename}");
        }
        assert_eq!(
            read_manifest(dir.path()).unwrap().model_id,
            DEFAULT_MODEL_ID
        );
    }

    #[test]
    fn rejects_bundled_fallback_for_non_default_model() {
        let dir = tempfile::tempdir().unwrap();

        let err =
            install_from_bundled_assets("other/model", dir.path(), "download failed".to_string())
                .unwrap_err();

        assert!(err.contains("bundled fallback is only available"));
        assert!(!dir.path().join("manifest.json").exists());
    }
}
