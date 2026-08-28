// Rust guideline compliant 2026-08-27

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::index::model::DEFAULT_MODEL_ID;
use crate::types::{Chunk, EMBED_DIM};
use crate::utils::trace;

/// Bumped whenever the on-disk cache layout or semantics change.
pub const CACHE_VERSION: u32 = 1;

/// The base directory under which per-repository index caches live.
const DEFAULT_CACHE_ROOT: &str = ".semble/index";

/// Environment variable that overrides the cache root directory.
///
/// Setting it to `none` disables the on-disk cache entirely.
const CACHE_ROOT_ENV: &str = "SEMBLE_CACHE_DIR";

fn cache_root() -> Option<PathBuf> {
    match std::env::var(CACHE_ROOT_ENV) {
        Ok(v) if v.eq_ignore_ascii_case("none") => None,
        Ok(v) if !v.is_empty() => Some(PathBuf::from(v)),
        _ => std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join(DEFAULT_CACHE_ROOT)),
    }
}

/// Returns the base directory under which per-repository index caches live.
///
/// # Returns
///
/// The configured cache root (`SEMBLE_CACHE_DIR`, else `~/.semble/index`), or
/// `None` when caching is disabled via `SEMBLE_CACHE_DIR=none`.
pub fn cache_root_dir() -> Option<PathBuf> {
    cache_root()
}

/// Reports whether the on-disk cache is disabled (`SEMBLE_CACHE_DIR=none`).
pub fn cache_root_disabled() -> bool {
    cache_root().is_none()
}

/// Recursively computes the total size, in bytes, of every file under `dir`.
pub fn dir_size(dir: &Path) -> u64 {
    let mut total = 0;
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                total += dir_size(&path);
            } else if let Ok(meta) = entry.metadata() {
                total += meta.len();
            }
        }
    }
    total
}

/// Removes a cache directory and all of its contents, if it exists.
pub fn delete_cache_dir(dir: &Path) -> Result<(), String> {
    if dir.exists() {
        fs::remove_dir_all(dir).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// The resolved identity of the embedding model, used to key the cache.
///
/// Mirrors how [`crate::index::model::StaticModel::from_pretrained`] selects a
/// model: `SEMBLE_MODEL_DIR` takes precedence, otherwise the default model id.
pub fn model_fingerprint() -> String {
    std::env::var("SEMBLE_MODEL_DIR")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| DEFAULT_MODEL_ID.to_string())
}

/// Computes a stable hex digest of a cache key.
pub fn fingerprint(key: &str) -> String {
    blake3::hash(key.as_bytes()).to_hex().to_string()
}

/// Resolves the on-disk cache directory for a source and build configuration.
///
/// # Arguments
///
/// * `source_key` - A stable key identifying the repository: the canonicalized
///   local path, or `url@ref` for a git source.
/// * `include_text_files` - Whether text files are part of the index; affects
///   the fingerprint because it changes which files are walked.
/// * `model` - The resolved model identity (see [`model_fingerprint`]).
///
/// # Returns
///
/// `None` when caching is disabled (via `SEMBLE_CACHE_DIR=none`), otherwise
/// the cache directory for this repository.
pub fn cache_dir(source_key: &str, include_text_files: bool, model: &str) -> Option<PathBuf> {
    let root = cache_root()?;
    let composite = format!("{}|{}|{}", source_key, include_text_files, model);
    Some(root.join(format!("{}-v{}", fingerprint(&composite), CACHE_VERSION)))
}

/// A single file's change-detection record within a cached index.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestEntry {
    /// File size in bytes at last index.
    pub size: u64,
    /// Last-modified time in nanoseconds since the Unix epoch.
    pub mtime_nanos: i128,
    /// Content hash (blake3 hex) used as a tiebreaker for mtime-stable edits.
    pub hash: String,
}

/// The persisted metadata describing which files a cached index covers.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    /// Maps the chunk-relative file path to its last-indexed metadata.
    pub files: HashMap<String, ManifestEntry>,
}

impl Manifest {
    /// Loads a manifest from `dir`, returning `None` on any read/parse failure.
    pub fn load(dir: &Path) -> Option<Self> {
        let raw = fs::read_to_string(dir.join("manifest.json")).ok()?;
        let manifest: Manifest = serde_json::from_str(&raw).ok()?;
        trace(format!(
            "loaded cache manifest with {} files",
            manifest.files.len()
        ));
        if manifest.version != CACHE_VERSION {
            return None;
        }
        Some(manifest)
    }

    /// Atomically writes the manifest to `dir`.
    pub fn save(&self, dir: &Path) -> Result<(), String> {
        let raw = serde_json::to_string(self).map_err(|e| e.to_string())?;
        atomic_write(dir, "manifest.json", raw.as_bytes())
    }
}

/// A file's chunk range within the cached chunk/embedding blobs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileRecord {
    /// The chunk-relative file path; matches `Chunk::file_path`.
    pub path: String,
    /// Index of the first chunk belonging to this file.
    pub start: usize,
    /// Number of chunks belonging to this file.
    pub count: usize,
}

/// Loads the cached chunk file list (`files.json`), if present.
pub fn load_file_records(dir: &Path) -> Option<Vec<FileRecord>> {
    let raw = fs::read_to_string(dir.join("files.json")).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Loads the full cached chunk corpus and its embeddings.
///
/// # Returns
///
/// `(chunks, embeddings)` where embeddings are parallel to `chunks`, or `None`
/// if the blob files are absent or malformed.
pub fn load_blobs(dir: &Path, expected: usize) -> Option<(Vec<Chunk>, Vec<[f32; EMBED_DIM]>)> {
    let chunks_raw = fs::read(dir.join("chunks.json")).ok()?;
    let chunks: Vec<Chunk> = serde_json::from_slice(&chunks_raw).ok()?;
    let emb_raw = fs::read(dir.join("embeddings.bin")).ok()?;
    let dims = expected * EMBED_DIM;
    if dims == 0 {
        return Some((chunks, vec![]));
    }
    if emb_raw.len() != dims * 4 || chunks.len() != expected {
        return None;
    }
    let mut embeddings = Vec::with_capacity(expected);
    for i in 0..expected {
        let base = i * EMBED_DIM * 4;
        let mut vec = [0.0f32; EMBED_DIM];
        for (j, slot) in vec.iter_mut().enumerate() {
            let bytes = [
                emb_raw[base + j * 4],
                emb_raw[base + j * 4 + 1],
                emb_raw[base + j * 4 + 2],
                emb_raw[base + j * 4 + 3],
            ];
            *slot = f32::from_le_bytes(bytes);
        }
        embeddings.push(vec);
    }
    Some((chunks, embeddings))
}

/// Persists the chunk corpus, embeddings, and file records into `dir`.
///
/// # Arguments
///
/// * `dir` - The cache directory; must already exist.
/// * `chunks` - The full ordered chunk corpus.
/// * `embeddings` - Embeddings parallel to `chunks`.
/// * `records` - Per-file chunk ranges, in the same order as `chunks`.
/// * `manifest` - The updated manifest to write.
pub fn save_blobs(
    dir: &Path,
    chunks: &[Chunk],
    embeddings: &[[f32; EMBED_DIM]],
    records: &[FileRecord],
    manifest: &Manifest,
) -> Result<(), String> {
    let chunks_raw = serde_json::to_vec(chunks).map_err(|e| e.to_string())?;
    let mut emb_buf = Vec::with_capacity(embeddings.len() * EMBED_DIM * 4);
    for vec in embeddings {
        for v in vec {
            emb_buf.extend_from_slice(&v.to_le_bytes());
        }
    }
    atomic_write(dir, "chunks.json", &chunks_raw)?;
    atomic_write(dir, "embeddings.bin", &emb_buf)?;
    let rec_raw = serde_json::to_vec(records).map_err(|e| e.to_string())?;
    atomic_write(dir, "files.json", &rec_raw)?;
    manifest.save(dir)
}

/// Writes `bytes` to `dir/name` via a temporary file and atomic rename.
fn atomic_write(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let tmp = dir.join(format!(".{}.tmp", name));
    fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    fs::rename(&tmp, dir.join(name)).map_err(|e| e.to_string())
}

/// The content hash (blake3 hex) of a file's bytes.
pub fn content_hash(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

/// Classifies walked files into a `HashMap<rel_path, hash>` where the hash is
/// present for every clean (reusable) file.
///
/// The `dirty` set is those files whose metadata changed; among those, files
/// whose content is byte-identical to the manifest (mtime-stable edits, e.g.
/// from a checkout touching mtimes) are still considered clean.
///
/// # Arguments
///
/// * `manifest` - The previous manifest, if any.
/// * `current` - `(rel_path, meta)` for every file present in this walk.
/// * `read` - A closure that reads a file's bytes by relative path, used to
///   verify mtime-changed files.
///
/// # Returns
///
/// `(clean_hashes, dirty_paths)` where `clean_hashes` maps a rel path to its
/// stored content hash (for reuse) and `dirty_paths` are files to re-process.
pub fn classify_dirty<F>(
    manifest: &Option<Manifest>,
    current: &[(String, crate::index::file_walker::FileMeta)],
    read: F,
) -> (HashMap<String, String>, Vec<String>)
where
    F: Fn(&str) -> Option<Vec<u8>>,
{
    let mut clean = HashMap::new();
    let mut dirty = Vec::new();
    if let Some(manifest) = manifest {
        for (path, meta) in current {
            match manifest.files.get(path) {
                None => {
                    dirty.push(path.clone());
                }
                Some(entry) => {
                    if entry.size == meta.size && entry.mtime_nanos == meta.mtime_nanos {
                        clean.insert(path.clone(), entry.hash.clone());
                    } else if let Some(bytes) = read(path) {
                        let hash = content_hash(&bytes);
                        if hash == entry.hash {
                            clean.insert(path.clone(), hash);
                        } else {
                            dirty.push(path.clone());
                        }
                    } else {
                        dirty.push(path.clone());
                    }
                }
            }
        }
    } else {
        dirty.extend(current.iter().map(|(p, _)| p.clone()));
    }
    (clean, dirty)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use tempfile::tempdir;

    use super::{
        CACHE_VERSION, FileRecord, Manifest, ManifestEntry, classify_dirty, content_hash,
        delete_cache_dir, dir_size, load_blobs, save_blobs,
    };
    use crate::index::file_walker::FileMeta;
    use crate::types::{Chunk, EMBED_DIM};

    fn chunk(path: &str, content: &str) -> Chunk {
        Chunk {
            content: content.to_string(),
            file_path: path.to_string(),
            start_line: 1,
            end_line: 1,
            language: Some("rust".to_string()),
            symbols: vec![],
        }
    }

    #[test]
    fn blobs_round_trip() {
        let dir = tempdir().unwrap();
        let chunks = vec![chunk("a.rs", "fn a() {}"), chunk("b.rs", "fn b() {}")];
        let embeddings = vec![[1.0f32; EMBED_DIM], [2.0f32; EMBED_DIM]];
        let records = vec![
            FileRecord {
                path: "a.rs".into(),
                start: 0,
                count: 1,
            },
            FileRecord {
                path: "b.rs".into(),
                start: 1,
                count: 1,
            },
        ];
        let manifest = Manifest {
            version: CACHE_VERSION,
            files: Default::default(),
        };
        save_blobs(dir.path(), &chunks, &embeddings, &records, &manifest).unwrap();
        let (loaded_chunks, loaded_embeds) = load_blobs(dir.path(), 2).unwrap();
        assert_eq!(loaded_chunks, chunks);
        assert_eq!(loaded_embeds, embeddings);
    }

    #[test]
    fn classify_dirty_marks_only_metadata_or_hash_changes() {
        let manifest = Manifest {
            version: CACHE_VERSION,
            files: [(
                "a.rs".to_string(),
                ManifestEntry {
                    size: 3,
                    mtime_nanos: 100,
                    hash: content_hash(b"abc"),
                },
            )]
            .into_iter()
            .collect(),
        };
        let read = |p: &str| match p {
            "a.rs" => Some(b"abc".to_vec()),
            _ => None,
        };
        let (clean, dirty) = classify_dirty(
            &Some(manifest),
            &[(
                "a.rs".to_string(),
                FileMeta {
                    size: 3,
                    mtime_nanos: 100,
                },
            )],
            read,
        );
        assert!(dirty.is_empty());
        assert_eq!(clean.get("a.rs").unwrap(), &content_hash(b"abc"));
    }

    #[test]
    fn classify_dirty_reuses_when_only_mtime_touched() {
        let manifest = Manifest {
            version: CACHE_VERSION,
            files: [(
                "a.rs".to_string(),
                ManifestEntry {
                    size: 3,
                    mtime_nanos: 100,
                    hash: content_hash(b"abc"),
                },
            )]
            .into_iter()
            .collect(),
        };
        // Metadata changed (e.g. checkout touching mtime) but content identical.
        let read = |_: &str| Some(b"abc".to_vec());
        let (clean, dirty) = classify_dirty(
            &Some(manifest),
            &[(
                "a.rs".to_string(),
                FileMeta {
                    size: 3,
                    mtime_nanos: 200,
                },
            )],
            read,
        );
        assert!(dirty.is_empty());
        assert_eq!(clean.get("a.rs").unwrap(), &content_hash(b"abc"));
    }

    #[test]
    fn classify_dirty_detects_metadata_and_content_change() {
        let manifest = Manifest {
            version: CACHE_VERSION,
            files: [(
                "a.rs".to_string(),
                ManifestEntry {
                    size: 3,
                    mtime_nanos: 100,
                    hash: content_hash(b"abc"),
                },
            )]
            .into_iter()
            .collect(),
        };
        let read = |_: &str| Some(b"xyz".to_vec());
        let (_, dirty) = classify_dirty(
            &Some(manifest),
            &[(
                "a.rs".to_string(),
                FileMeta {
                    size: 3,
                    mtime_nanos: 200,
                },
            )],
            read,
        );
        assert_eq!(dirty, vec!["a.rs".to_string()]);
    }

    #[test]
    fn dir_size_sums_nested_files() {
        let dir = tempdir().unwrap();
        let sub = dir.path().join("sub");
        fs::create_dir_all(&sub).unwrap();
        fs::write(dir.path().join("a.bin"), vec![0u8; 10]).unwrap();
        fs::write(sub.join("b.bin"), vec![0u8; 5]).unwrap();
        assert_eq!(dir_size(dir.path()), 15);
    }

    #[test]
    fn delete_cache_dir_removes_existing_and_tolerates_missing() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("cache");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("x"), b"1").unwrap();
        delete_cache_dir(&target).unwrap();
        assert!(!target.exists());
        delete_cache_dir(&target).unwrap();
    }
}
