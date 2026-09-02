// Rust guideline compliant 2026-08-28

use std::fs;
use std::path::{Path, PathBuf};

use crate::index::SembleIndex;
use crate::index::persist;
use crate::types::IndexStats;
use crate::utils::is_git_url;

/// The resolved cache location for a user-supplied index handle (a local
/// directory path or a git URL).
pub struct CacheLoc {
    /// The stable cache key (`canonical path` or `url` / `url@ref`).
    pub source_key: String,
    /// The per-repository cache directory, or `None` when caching is disabled.
    pub cache_dir: Option<PathBuf>,
    /// Whether the handle was interpreted as a git URL.
    pub is_git: bool,
}

/// Resolves a user-supplied index handle into its cache location and key.
///
/// Mirrors the source-key scheme used by [`SembleIndex::from_path_cached`] and
/// [`SembleIndex::from_git_cached`].
///
/// # Arguments
///
/// * `handle` - A local directory path or a git URL.
/// * `ref_name` - Optional git ref; forms the `url@ref` source key.
/// * `include_text_files` - Whether text files are indexed; part of the key.
/// * `model_ref` - Optional user-supplied model id or path; part of the key.
///
/// # Errors
///
/// Returns a message if `handle` is a local path that does not exist or is not
/// a directory.
pub fn resolve_source_key(
    handle: &str,
    ref_name: Option<&str>,
    include_text_files: bool,
    model_ref: Option<&str>,
) -> Result<CacheLoc, String> {
    let (source_key, is_git) = if is_git_url(handle) {
        let key = match ref_name {
            Some(r) => format!("{}@{}", handle, r),
            None => handle.to_string(),
        };
        (key, true)
    } else {
        let path = Path::new(handle);
        if !path.exists() {
            return Err(format!("Path does not exist: {}", path.display()));
        }
        if !path.is_dir() {
            return Err(format!("Path is not a directory: {}", path.display()));
        }
        let canon = path.canonicalize().map_err(|e| e.to_string())?;
        (canon.to_string_lossy().to_string(), false)
    };
    let cache_dir = persist::cache_dir(
        &source_key,
        include_text_files,
        &persist::model_fingerprint(model_ref),
    );
    Ok(CacheLoc {
        source_key,
        cache_dir,
        is_git,
    })
}

/// Prints the cache root location and total on-disk size across all repos.
pub fn run_cache_info() {
    let root = persist::cache_root_dir();
    match root {
        None => {
            println!("Cache is disabled (SEMBLE_CACHE_DIR=none).");
        }
        Some(root) => {
            println!("Cache root: {}", root.display());
            if !root.exists() {
                println!("Cache directory does not exist yet. Run a search to create it.");
            } else {
                let bytes = persist::dir_size(&root);
                println!("Total size: {}", human_size(bytes));
                let repos = list_index_dirs(&root).len();
                println!("Cached repositories: {}", repos);
            }
        }
    }
}

/// Prints cache and corpus status for a single index handle.
///
/// When `build` is set, the index is (re)built/seeded from disk to report full
/// corpus statistics; otherwise only the lightweight on-disk cache metadata is
/// shown without loading the model.
pub fn run_status(
    handle: &str,
    ref_name: Option<&str>,
    include_text_files: bool,
    model_ref: Option<&str>,
    build: bool,
) -> Result<(), String> {
    let loc = resolve_source_key(handle, ref_name, include_text_files, model_ref)?;
    if !build {
        if persist::cache_root_disabled() {
            println!("Cache is disabled (SEMBLE_CACHE_DIR=none). Indexes are rebuilt each run.");
            return Ok(());
        }
        let Some(dir) = &loc.cache_dir else {
            println!("Cache is disabled (SEMBLE_CACHE_DIR=none).");
            return Ok(());
        };
        println!("Source:   {}", loc.source_key);
        println!("Model:    {}", persist::model_fingerprint(model_ref));
        if dir.exists() {
            let manifest = persist::Manifest::load(dir).unwrap_or_default();
            println!("Status:   cached");
            println!("Cache:    {}", dir.display());
            println!("Size:     {}", human_size(persist::dir_size(dir)));
            println!("Files:    {}", manifest.files.len());
            if let Ok(modified) = dir.metadata().and_then(|m| m.modified()).map(|t| {
                t.duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0)
            }) {
                println!("Modified: {}", modified);
            }
        } else {
            println!("Status:   not cached (run a search or `--build` to seed)");
            println!("Cache:    {}", dir.display());
        }
        return Ok(());
    }

    let index = build_index(
        &loc.source_key,
        loc.is_git,
        ref_name,
        include_text_files,
        model_ref,
    )?;
    let stats = index.stats();
    println!("Model:    {}", persist::model_fingerprint(model_ref));
    print_stats(&loc.source_key, &stats);
    Ok(())
}

/// Removes the cache for a single index handle.
pub fn run_clear_one(
    handle: &str,
    ref_name: Option<&str>,
    include_text_files: bool,
    model_ref: Option<&str>,
) -> Result<(), String> {
    let loc = resolve_source_key(handle, ref_name, include_text_files, model_ref)?;
    let Some(dir) = &loc.cache_dir else {
        return Err("Cache is disabled (SEMBLE_CACHE_DIR=none); nothing to clear.".to_string());
    };
    if !dir.exists() {
        println!("No cache found for {}", loc.source_key);
        return Ok(());
    }
    let size = persist::dir_size(dir);
    persist::delete_cache_dir(dir)?;
    println!(
        "Cleared cached index for {} ({} freed).",
        loc.source_key,
        human_size(size)
    );
    Ok(())
}

/// Removes the entire cache root, optionally without prompting.
pub fn run_clear_all(force: bool) -> Result<(), String> {
    let Some(root) = persist::cache_root_dir() else {
        return Err("Cache is disabled (SEMBLE_CACHE_DIR=none); nothing to clear.".to_string());
    };
    if !root.exists() {
        println!("No cache to clear.");
        return Ok(());
    }
    if !force {
        eprint!("Remove entire cache at {}? [y/N] ", root.display());
        let mut input = String::new();
        if std::io::stdin().read_line(&mut input).is_err() {
            return Err("Failed to read confirmation.".to_string());
        }
        if !matches!(input.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
            println!("Aborted.");
            return Ok(());
        }
    }
    let size = persist::dir_size(&root);
    persist::delete_cache_dir(&root)?;
    println!("Cleared all caches ({} freed).", human_size(size));
    Ok(())
}

/// Lists the per-repository cache directories under `root`.
fn list_index_dirs(root: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                dirs.push(entry.path());
            }
        }
    }
    dirs
}

/// Builds an index for `source_key` (a canonical path or git handle).
fn build_index(
    source_key: &str,
    is_git: bool,
    ref_name: Option<&str>,
    include_text_files: bool,
    model_ref: Option<&str>,
) -> Result<SembleIndex, String> {
    if is_git {
        SembleIndex::from_git_cached(
            source_key,
            ref_name,
            None,
            model_ref,
            None,
            include_text_files,
        )
    } else {
        SembleIndex::from_path_cached(source_key, None, model_ref, None, include_text_files)
    }
}

/// Formats `count` bytes as a human-readable size.
fn human_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.2} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.2} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.2} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

/// Prints corpus statistics for an index.
fn print_stats(source: &str, stats: &IndexStats) {
    println!("Source:           {}", source);
    println!("Indexed files:    {}", stats.indexed_files);
    println!("Total chunks:     {}", stats.total_chunks);
    if stats.languages.is_empty() {
        println!("Languages:        (none)");
    } else {
        println!("Languages:");
        for (lang, count) in &stats.languages {
            println!("  {}: {}", lang, count);
        }
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::{human_size, resolve_source_key};

    #[test]
    fn resolve_source_key_marks_git_urls() {
        let loc =
            resolve_source_key("https://github.com/a/b.git", Some("main"), false, None).unwrap();
        assert!(loc.is_git);
        assert_eq!(loc.source_key, "https://github.com/a/b.git@main");
    }

    #[test]
    fn resolve_source_key_marks_git_urls_without_ref() {
        let loc = resolve_source_key("https://github.com/a/b.git", None, false, None).unwrap();
        assert!(loc.is_git);
        assert_eq!(loc.source_key, "https://github.com/a/b.git");
    }

    #[test]
    fn resolve_source_key_canonicalizes_local_path() {
        let dir = tempdir().unwrap();
        let loc = resolve_source_key(dir.path().to_str().unwrap(), None, false, None).unwrap();
        assert!(!loc.is_git);
        assert!(loc.cache_dir.is_some());
        assert!(loc.cache_dir.unwrap().to_str().unwrap().contains("-v1"));
    }

    #[test]
    fn resolve_source_key_model_changes_cache_dir() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        let a = resolve_source_key(path, None, false, Some("model-a")).unwrap();
        let b = resolve_source_key(path, None, false, Some("model-b")).unwrap();
        assert_ne!(a.cache_dir, b.cache_dir);
    }

    #[test]
    fn resolve_source_key_errors_on_missing_path() {
        assert!(resolve_source_key("/definitely/not/here", None, false, None).is_err());
    }

    #[test]
    fn human_size_formats() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(2048), "2.00 KiB");
        assert_eq!(human_size(3 * 1024 * 1024), "3.00 MiB");
    }
}
