use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::index::create::{create_index_from_path, create_index_incremental};
use crate::index::dense::{SelectableBasicBackend, StaticModel, load_model};
use crate::index::persist;
use crate::index::sparse::Bm25Index;
use crate::index::symbols::{SymbolIndex, SymbolOccurrence};
use crate::search::{search_bm25, search_hybrid, search_semantic};
use crate::stats::save_search_stats;
use crate::types::{CallType, Chunk, IndexStats, SearchMode, SearchResult, Symbol, SymbolKind};
use crate::utils::{normalize_file_path, resolve_chunk, resolve_chunk_detailed, trace};

/// A definition or usage reference to a symbol, resolved to its chunk.
#[derive(Debug, Clone)]
pub struct SymbolRef {
    pub chunk: Chunk,
    pub symbol: Symbol,
}

/// The story of one symbol: where it is defined and where it is referenced.
#[derive(Debug, Clone)]
pub struct SymbolReport {
    pub name: String,
    pub definitions: Vec<SymbolRef>,
    pub usages: Vec<SymbolRef>,
}

#[derive(Debug, Clone)]
pub struct SembleIndex {
    pub model: StaticModel,
    pub chunks: Vec<Chunk>,
    bm25_index: Bm25Index,
    semantic_index: SelectableBasicBackend,
    file_sizes: HashMap<String, usize>,
    file_mapping: HashMap<String, Vec<usize>>,
    language_mapping: HashMap<String, Vec<usize>>,
    symbol_index: SymbolIndex,
    root: Option<PathBuf>,
}

impl SembleIndex {
    pub fn new(
        model: StaticModel,
        bm25_index: Bm25Index,
        semantic_index: SelectableBasicBackend,
        chunks: Vec<Chunk>,
        root: Option<PathBuf>,
    ) -> Self {
        let file_sizes = root
            .as_ref()
            .map(|r| compute_file_sizes(&chunks, r))
            .unwrap_or_default();
        Self::from_parts(model, bm25_index, semantic_index, chunks, root, file_sizes)
    }

    /// Constructs an index from already-built pieces, taking `file_sizes`
    /// directly instead of re-reading every file from disk.
    ///
    /// This is used by the cached builders which derive sizes from their walk
    /// metadata, avoiding the per-load re-read `new` performs.
    pub fn from_parts(
        model: StaticModel,
        bm25_index: Bm25Index,
        semantic_index: SelectableBasicBackend,
        chunks: Vec<Chunk>,
        root: Option<PathBuf>,
        file_sizes: HashMap<String, usize>,
    ) -> Self {
        let (file_mapping, language_mapping) = populate_mapping(&chunks);
        let symbol_index = SymbolIndex::build(&chunks);
        Self {
            model,
            chunks,
            bm25_index,
            semantic_index,
            file_sizes,
            file_mapping,
            language_mapping,
            symbol_index,
            root,
        }
    }

    /// Resolves a user-provided file path to the root-relative form used by
    /// this index's chunks.
    ///
    /// Accepts absolute paths (canonicalized and stripped of the index root)
    /// as well as paths relative to the index root. Unresolvable paths are
    /// returned unchanged for exact-match fallback.
    ///
    /// # Arguments
    ///
    /// * `file_path` - An absolute or index-root-relative path.
    ///
    /// # Returns
    ///
    /// The root-relative path string.
    pub fn resolve_path(&self, file_path: &str) -> String {
        normalize_file_path(self.root.as_deref(), file_path)
    }

    /// Reads a window of source lines surrounding a chunk directly from disk.
    ///
    /// The returned window contains `context_lines` lines above the chunk's
    /// `start_line`, the chunk's own lines (`start_line..=end_line`), and
    /// `context_lines` lines below `end_line`, joined by newlines. A lone `...`
    /// marker line separates the surrounding context from the chunk's own lines
    /// on each side; the marker is omitted wherever the surrounding context is
    /// absent (at the top of the file or past its end). The window is clamped
    /// to the file bounds.
    ///
    /// Returns `None` when the index has no root directory or the file cannot
    /// be read, so callers can fall back to chunk-only output.
    ///
    /// # Arguments
    ///
    /// * `rel_path` - A root-relative file path locating the source on disk.
    /// * `start_line` - The chunk's first line (1-based).
    /// * `end_line` - The chunk's last line (1-based).
    /// * `context_lines` - Number of surrounding lines to include above and
    ///   below the chunk.
    ///
    /// # Returns
    ///
    /// The expanded snippet text, or `None` if the file is unavailable.
    pub fn read_snippet_context(
        &self,
        rel_path: &str,
        start_line: usize,
        end_line: usize,
        context_lines: usize,
    ) -> Option<String> {
        let root = self.root.as_ref()?;
        let path = root.join(rel_path);
        let raw = std::fs::read_to_string(&path).ok()?;
        let file_lines: Vec<&str> = raw
            .lines()
            .map(|line| line.strip_suffix('\r').unwrap_or(line))
            .collect();
        let total = file_lines.len();
        if start_line == 0 || end_line < start_line {
            return Some(String::new());
        }
        let chunk_start = (start_line - 1).min(total);
        let chunk_end = end_line.min(total);
        let above_start = chunk_start.saturating_sub(context_lines);
        let below_end = chunk_end.saturating_add(context_lines).min(total);

        let mut out: Vec<&str> = Vec::new();
        if above_start < chunk_start {
            out.extend_from_slice(&file_lines[above_start..chunk_start]);
            out.push("...");
        }
        if chunk_start < chunk_end {
            out.extend_from_slice(&file_lines[chunk_start..chunk_end]);
        }
        if chunk_end < below_end {
            out.push("...");
            out.extend_from_slice(&file_lines[chunk_end..below_end]);
        }
        Some(out.join("\n"))
    }

    pub fn stats(&self) -> IndexStats {
        let mut languages = std::collections::BTreeMap::new();
        for chunk in &self.chunks {
            if let Some(lang) = &chunk.language {
                *languages.entry(lang.clone()).or_insert(0) += 1;
            }
        }
        IndexStats {
            indexed_files: self.file_mapping.len(),
            total_chunks: self.chunks.len(),
            languages,
        }
    }

    pub fn from_path(
        path: impl AsRef<Path>,
        model: Option<StaticModel>,
        model_ref: Option<&str>,
        extensions: Option<&[&str]>,
        include_text_files: bool,
    ) -> Result<Self, String> {
        let path = path.as_ref();
        trace(format!("SembleIndex::from_path path={}", path.display()));
        if !path.exists() {
            return Err(format!("Path does not exist: {}", path.display()));
        }
        if !path.is_dir() {
            return Err(format!("Path is not a directory: {}", path.display()));
        }
        let model = model.unwrap_or_else(|| load_model(model_ref));
        let path = path.canonicalize().map_err(|e| e.to_string())?;
        let (bm25, semantic, chunks) =
            create_index_from_path(&path, &model, extensions, include_text_files, Some(&path))?;
        Ok(Self::new(model, bm25, semantic, chunks, Some(path)))
    }

    pub fn from_git(
        url: &str,
        ref_name: Option<&str>,
        model: Option<StaticModel>,
        model_ref: Option<&str>,
        extensions: Option<&[&str]>,
        include_text_files: bool,
    ) -> Result<Self, String> {
        trace(format!(
            "SembleIndex::from_git url={} ref={:?}",
            url, ref_name
        ));
        let tmp = tempfile::tempdir().map_err(|e| e.to_string())?;
        let clone_path = tmp.path().to_path_buf();
        let mut cmd = Command::new("git");
        cmd.args(["clone", "--depth", "1"]);
        if let Some(r) = ref_name {
            cmd.args(["--branch", r]);
        }
        cmd.args(["--", url, clone_path.to_str().unwrap()]);
        let output = cmd.output().map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(format!(
                "git clone failed for {:?}: {}",
                url,
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let model = model.unwrap_or_else(|| load_model(model_ref));
        let path = clone_path.canonicalize().map_err(|e| e.to_string())?;
        std::mem::forget(tmp);
        let (bm25, semantic, chunks) =
            create_index_from_path(&path, &model, extensions, include_text_files, Some(&path))?;
        Ok(Self::new(model, bm25, semantic, chunks, Some(path)))
    }

    /// Builds an index for a local path, reusing unchanged files from a
    /// persistent cache.
    ///
    /// Equivalent to [`Self::from_path`] except that changed files are
    /// re-chunked and re-embedded while unchanged files are reused from the
    /// on-disk cache. When caching is disabled or the cache is unusable this
    /// behaves like a fresh build.
    pub fn from_path_cached(
        path: impl AsRef<Path>,
        model: Option<StaticModel>,
        model_ref: Option<&str>,
        extensions: Option<&[&str]>,
        include_text_files: bool,
    ) -> Result<Self, String> {
        let path = path.as_ref();
        if !path.exists() {
            return Err(format!("Path does not exist: {}", path.display()));
        }
        if !path.is_dir() {
            return Err(format!("Path is not a directory: {}", path.display()));
        }
        let model = model.unwrap_or_else(|| load_model(model_ref));
        let path = path.canonicalize().map_err(|e| e.to_string())?;
        let source_key = path.to_string_lossy().to_string();
        let model_ref = persist::model_fingerprint(model_ref);
        let (bm25, semantic, chunks, file_sizes) = create_index_incremental(
            &path,
            &model,
            extensions,
            include_text_files,
            Some(&path),
            &source_key,
            &model_ref,
        )?;
        Ok(Self::from_parts(
            model,
            bm25,
            semantic,
            chunks,
            Some(path),
            file_sizes,
        ))
    }

    /// Builds an index for a git URL, reusing unchanged files from a persistent
    /// cache keyed by `url@ref`.
    ///
    /// A fresh shallow clone is always taken (so references are correct), but
    /// files unchanged since the last clone reuse their cached chunks and
    /// embeddings rather than being re-processed.
    pub fn from_git_cached(
        url: &str,
        ref_name: Option<&str>,
        model: Option<StaticModel>,
        model_ref: Option<&str>,
        extensions: Option<&[&str]>,
        include_text_files: bool,
    ) -> Result<Self, String> {
        let tmp = tempfile::tempdir().map_err(|e| e.to_string())?;
        let clone_path = tmp.path().to_path_buf();
        let mut cmd = Command::new("git");
        cmd.args(["clone", "--depth", "1"]);
        if let Some(r) = ref_name {
            cmd.args(["--branch", r]);
        }
        cmd.args(["--", url, clone_path.to_str().unwrap()]);
        let output = cmd.output().map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(format!(
                "git clone failed for {:?}: {}",
                url,
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let model = model.unwrap_or_else(|| load_model(model_ref));
        let path = clone_path.canonicalize().map_err(|e| e.to_string())?;
        let source_key = match ref_name {
            Some(r) => format!("{}@{}", url, r),
            None => url.to_string(),
        };
        let model_ref = persist::model_fingerprint(model_ref);
        let (bm25, semantic, chunks, file_sizes) = create_index_incremental(
            &path,
            &model,
            extensions,
            include_text_files,
            Some(&path),
            &source_key,
            &model_ref,
        )?;
        std::mem::forget(tmp);
        Ok(Self::from_parts(
            model,
            bm25,
            semantic,
            chunks,
            Some(path),
            file_sizes,
        ))
    }

    fn selector(
        &self,
        filter_languages: Option<&[String]>,
        filter_paths: Option<&[String]>,
    ) -> Option<Vec<usize>> {
        let mut selector = Vec::new();
        if let Some(langs) = filter_languages {
            for lang in langs {
                selector.extend(self.language_mapping.get(lang).cloned().unwrap_or_default());
            }
        }
        if let Some(paths) = filter_paths {
            for path in paths {
                selector.extend(self.file_mapping.get(path).cloned().unwrap_or_default());
            }
        }
        if selector.is_empty() {
            None
        } else {
            selector.sort_unstable();
            selector.dedup();
            Some(selector)
        }
    }

    pub fn find_related(&self, source: &Chunk, top_k: usize) -> Vec<SearchResult> {
        let selector = source.language.as_ref().map(|lang| vec![lang.clone()]);
        let filter = self.selector(selector.as_deref(), None);
        let results = search_semantic(
            &source.content,
            &self.model,
            &self.semantic_index,
            &self.chunks,
            top_k + 1,
            filter.as_deref(),
        )
        .into_iter()
        .filter(|r| r.chunk != *source)
        .take(top_k)
        .collect::<Vec<_>>();
        save_search_stats(&results, CallType::FindRelated, &self.file_sizes);
        results
    }

    pub fn search(
        &self,
        query: &str,
        top_k: usize,
        mode: SearchMode,
        alpha: Option<f32>,
        filter_languages: Option<&[String]>,
        filter_paths: Option<&[String]>,
    ) -> Vec<SearchResult> {
        if self.chunks.is_empty() || query.trim().is_empty() {
            return vec![];
        }
        let selector = self.selector(filter_languages, filter_paths);
        let results = match mode {
            SearchMode::Bm25 => search_bm25(
                query,
                &self.bm25_index,
                &self.chunks,
                top_k,
                selector.as_deref(),
            ),
            SearchMode::Semantic => search_semantic(
                query,
                &self.model,
                &self.semantic_index,
                &self.chunks,
                top_k,
                selector.as_deref(),
            ),
            SearchMode::Hybrid => search_hybrid(
                query,
                &self.model,
                &self.semantic_index,
                &self.bm25_index,
                &self.chunks,
                top_k,
                alpha,
                selector.as_deref(),
            ),
        };
        save_search_stats(&results, CallType::Search, &self.file_sizes);
        results
    }

    pub fn find_related_by_location(
        &self,
        file_path: &str,
        line: usize,
        top_k: usize,
    ) -> Option<Vec<SearchResult>> {
        let chunk = resolve_chunk(&self.chunks, &self.resolve_path(file_path), line)?;
        Some(self.find_related(&chunk, top_k))
    }

    /// Returns the definition and referencing chunks for a symbol name.
    ///
    /// The name is normalized to a lowered identifier. Returns `None` when the
    /// symbol has neither a definition nor any usage in the index.
    pub fn symbol(&self, name: &str) -> Option<SymbolReport> {
        let lowered = name.to_lowercase();
        if !self.symbol_index.contains(&lowered) {
            return None;
        }
        let occs = self.symbol_index.definitions(&lowered);
        let kind = occs.first().map(|o| o.kind).unwrap_or(SymbolKind::Unknown);
        let definitions = occs
            .iter()
            .map(|occ| self.ref_for(occ, &lowered))
            .collect::<Vec<_>>();
        let usages = self
            .symbol_index
            .referencing_chunks(&lowered)
            .iter()
            .map(|&idx| SymbolRef {
                chunk: self.chunks[idx].clone(),
                symbol: Symbol {
                    name: lowered.clone(),
                    kind,
                    line: self.chunks[idx].start_line,
                },
            })
            .collect::<Vec<_>>();
        Some(SymbolReport {
            name: lowered,
            definitions,
            usages,
        })
    }

    /// Returns reports for every symbol declared in the chunk at `file:line`.
    ///
    /// Uses the nearest chunk when the line does not fall exactly inside one.
    pub fn symbols_at(&self, file_path: &str, line: usize) -> Vec<SymbolReport> {
        let rooted = self.resolve_path(file_path);
        let Some(resolution) = resolve_chunk_detailed(&self.chunks, &rooted, line) else {
            return vec![];
        };
        let chunk = resolution.chunk;
        let mut reports = Vec::new();
        for symbol in &chunk.symbols {
            if let Some(report) = self.symbol(&symbol.name) {
                reports.push(report);
            }
        }
        reports
    }

    fn ref_for(&self, occ: &SymbolOccurrence, name: &str) -> SymbolRef {
        SymbolRef {
            chunk: self.chunks[occ.chunk_idx].clone(),
            symbol: Symbol {
                name: name.to_string(),
                kind: occ.kind,
                line: occ.line,
            },
        }
    }
}

fn populate_mapping(
    chunks: &[Chunk],
) -> (HashMap<String, Vec<usize>>, HashMap<String, Vec<usize>>) {
    let mut file_map = HashMap::new();
    let mut lang_map = HashMap::new();
    for (i, c) in chunks.iter().enumerate() {
        file_map
            .entry(c.file_path.clone())
            .or_insert_with(Vec::new)
            .push(i);
        if let Some(lang) = &c.language {
            lang_map
                .entry(lang.clone())
                .or_insert_with(Vec::new)
                .push(i);
        }
    }
    (file_map, lang_map)
}

fn compute_file_sizes(chunks: &[Chunk], root: &Path) -> HashMap<String, usize> {
    let mut sizes = HashMap::new();
    for chunk in chunks {
        if sizes.contains_key(&chunk.file_path) {
            continue;
        }
        let path = root.join(&chunk.file_path);
        if let Ok(text) = std::fs::read_to_string(&path) {
            sizes.insert(chunk.file_path.clone(), text.len());
        }
    }
    sizes
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use tempfile::tempdir;

    use super::SembleIndex;
    use crate::index::dense::{SelectableBasicBackend, StaticModel};
    use crate::index::sparse::Bm25Index;

    /// Builds an index over a temporary root, using empty backing indexes
    /// since the context reader only needs the root path.
    fn test_index(root: Option<PathBuf>) -> SembleIndex {
        SembleIndex::from_parts(
            StaticModel::from_pretrained("__offline_test_model__"),
            Bm25Index::new(vec![]),
            SelectableBasicBackend::new(vec![]),
            vec![],
            root,
            std::collections::HashMap::new(),
        )
    }

    /// Writes a 20-line file (one "line N" per line) into a temp dir.
    fn fixture() -> (tempfile::TempDir, PathBuf) {
        let dir = tempdir().expect("tempdir");
        let file = dir.path().join("a.rs");
        let content = (1..=20)
            .map(|n| format!("line {n}"))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&file, content).expect("write fixture");
        (dir, file)
    }

    #[test]
    fn window_includes_context_above_and_below_chunk() {
        let (dir, _) = fixture();
        let index = test_index(Some(dir.path().to_path_buf()));
        let out = index
            .read_snippet_context("a.rs", 5, 7, 2)
            .expect("snippet");
        let expected = "line 3\nline 4\n...\nline 5\nline 6\nline 7\n...\nline 8\nline 9";
        assert_eq!(out, expected);
    }

    #[test]
    fn window_clamps_at_top_of_file() {
        let (dir, _) = fixture();
        let index = test_index(Some(dir.path().to_path_buf()));
        let out = index
            .read_snippet_context("a.rs", 1, 2, 2)
            .expect("snippet");
        let expected = "line 1\nline 2\n...\nline 3\nline 4";
        assert_eq!(out, expected);
    }

    #[test]
    fn window_clamps_at_end_of_file() {
        let (dir, _) = fixture();
        let index = test_index(Some(dir.path().to_path_buf()));
        let out = index
            .read_snippet_context("a.rs", 18, 20, 2)
            .expect("snippet");
        let expected = "line 16\nline 17\n...\nline 18\nline 19\nline 20";
        assert_eq!(out, expected);
    }

    #[test]
    fn handles_crlf_line_endings() {
        let dir = tempdir().expect("tempdir");
        let file = dir.path().join("a.rs");
        std::fs::write(&file, "one\r\ntwo\r\nthree").expect("write");
        let index = test_index(Some(dir.path().to_path_buf()));
        let out = index
            .read_snippet_context("a.rs", 2, 2, 1)
            .expect("snippet");
        let expected = "one\n...\ntwo\n...\nthree";
        assert_eq!(out, expected);
    }

    #[test]
    fn returns_none_when_root_is_absent() {
        let index = test_index(None);
        assert_eq!(index.read_snippet_context("a.rs", 1, 2, 2), None);
    }

    #[test]
    fn returns_none_when_file_is_unreadable() {
        let (dir, _) = fixture();
        let index = test_index(Some(dir.path().to_path_buf()));
        assert_eq!(index.read_snippet_context("missing.rs", 1, 2, 2), None);
    }
}
