use std::path::Path;

use once_cell::sync::Lazy;
use regex::Regex;

use crate::tokens::split_identifier;
use crate::types::Chunk;

static SYMBOL_QUERY_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(?:[A-Za-z_][A-Za-z0-9_]*(?:(?:::|\\|->|\.)[A-Za-z_][A-Za-z0-9_]*)+|_[A-Za-z0-9_]*|[A-Za-z][A-Za-z0-9]*[A-Z_][A-Za-z0-9_]*|[A-Z][A-Za-z0-9]*)$").unwrap()
});
static EMBEDDED_SYMBOL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"\b(?:[A-Z][a-z][a-zA-Z0-9]*[A-Z][a-zA-Z0-9]*|[a-z][a-zA-Z0-9]*[A-Z][a-zA-Z0-9]+)\b",
    )
    .unwrap()
});
static DEFINITION_KEYWORDS: [&str; 11] = [
    "class",
    "module",
    "defmodule",
    "def",
    "interface",
    "struct",
    "enum",
    "trait",
    "func",
    "function",
    "fn",
];
static STOPWORDS: [&str; 27] = [
    "a", "an", "and", "are", "as", "at", "be", "by", "do", "does", "for", "from", "has", "have",
    "how", "if", "in", "is", "it", "not", "of", "on", "or", "the", "to", "was", "with",
];

pub fn is_symbol_query(query: &str) -> bool {
    SYMBOL_QUERY_RE.is_match(query.trim())
}

pub fn apply_query_boost(
    mut combined_scores: std::collections::HashMap<Chunk, f32>,
    query: &str,
    all_chunks: &[Chunk],
) -> std::collections::HashMap<Chunk, f32> {
    if combined_scores.is_empty() {
        return combined_scores;
    }
    let max_score = combined_scores.values().cloned().fold(0.0, f32::max);
    if is_symbol_query(query) {
        let symbol = query
            .split(&[':', '\\', '-', '.'][..])
            .next_back()
            .unwrap_or(query)
            .trim();
        for chunk in all_chunks {
            if chunk_defines_symbol(chunk, symbol) {
                *combined_scores.entry(chunk.clone()).or_insert(0.0) += max_score * 3.0;
            }
        }
    } else {
        for chunk in all_chunks {
            if chunk_defines_embedded_symbol(chunk, query) {
                *combined_scores.entry(chunk.clone()).or_insert(0.0) += max_score * 1.5;
            }
        }
        boost_path_stems(&mut combined_scores, query, max_score);
    }
    combined_scores
}

pub fn boost_multi_chunk_files(scores: &mut std::collections::HashMap<Chunk, f32>) {
    if scores.is_empty() {
        return;
    }
    let max_score = scores.values().cloned().fold(0.0, f32::max);
    if max_score == 0.0 {
        return;
    }
    let mut file_sum: std::collections::HashMap<String, f32> = std::collections::HashMap::new();
    let mut best: std::collections::HashMap<String, Chunk> = std::collections::HashMap::new();
    for (chunk, score) in scores.iter() {
        *file_sum.entry(chunk.file_path.clone()).or_default() += *score;
        if best
            .get(&chunk.file_path)
            .map(|c| score > scores.get(c).unwrap())
            .unwrap_or(true)
        {
            best.insert(chunk.file_path.clone(), chunk.clone());
        }
    }
    let max_sum = file_sum.values().cloned().fold(0.0, f32::max);
    for (file, chunk) in best {
        if let Some(score) = scores.get_mut(&chunk) {
            *score += max_score * 0.2 * file_sum[&file] / max_sum;
        }
    }
}

fn chunk_defines_symbol(chunk: &Chunk, symbol: &str) -> bool {
    let symbol = symbol.trim();
    DEFINITION_KEYWORDS
        .iter()
        .any(|kw| chunk.content.contains(&format!("{} {}", kw, symbol)))
}

fn chunk_defines_embedded_symbol(chunk: &Chunk, query: &str) -> bool {
    EMBEDDED_SYMBOL_RE
        .find_iter(query)
        .any(|m| chunk_defines_symbol(chunk, m.as_str()))
}

fn boost_path_stems(
    scores: &mut std::collections::HashMap<Chunk, f32>,
    query: &str,
    max_score: f32,
) {
    let keywords: Vec<String> = query
        .split_whitespace()
        .map(|s| s.to_lowercase())
        .filter(|w| w.len() > 2 && !STOPWORDS.contains(&w.as_str()))
        .collect();
    if keywords.is_empty() {
        return;
    }
    for (chunk, score) in scores.iter_mut() {
        let stem = Path::new(&chunk.file_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();
        let parts: Vec<String> = split_identifier(&stem);
        let hits = keywords
            .iter()
            .filter(|k| {
                parts
                    .iter()
                    .any(|p| p == *k || p.starts_with(&k[..3.min(k.len())]))
            })
            .count();
        if hits > 0 {
            *score += max_score * (hits as f32 / keywords.len() as f32);
        }
    }
}
