use std::collections::HashMap;

use crate::index::dense::SelectableBasicBackend;
use crate::index::sparse::{Bm25Index, selector_to_mask};
use crate::ranking::{
    boosting::{apply_query_boost, boost_multi_chunk_files},
    penalties::rerank_topk,
    weighting::resolve_alpha,
};
use crate::tokens::tokenize;
use crate::types::{Chunk, Encoder, SearchMode, SearchResult};

const RRF_K: f32 = 60.0;

fn rrf_scores(scores: &HashMap<Chunk, f32>) -> HashMap<Chunk, f32> {
    if scores.is_empty() {
        return HashMap::new();
    }
    let mut ranked: Vec<_> = scores.iter().collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap());
    ranked
        .into_iter()
        .enumerate()
        .map(|(rank, (chunk, _))| (chunk.clone(), 1.0 / (RRF_K + (rank + 1) as f32)))
        .collect()
}

pub fn search_semantic(
    query: &str,
    model: &impl Encoder,
    semantic_index: &SelectableBasicBackend,
    chunks: &[Chunk],
    top_k: usize,
    selector: Option<&[usize]>,
) -> Vec<SearchResult> {
    let query_embedding = model.encode(&[query.to_string()]);
    let Some((indices, scores)) = semantic_index
        .query(&query_embedding, top_k, selector)
        .into_iter()
        .next()
    else {
        return vec![];
    };
    indices
        .into_iter()
        .zip(scores)
        .map(|(idx, distance)| SearchResult {
            chunk: chunks[idx].clone(),
            score: 1.0 - distance,
            source: SearchMode::Semantic,
        })
        .collect()
}

pub fn search_bm25(
    query: &str,
    bm25_index: &Bm25Index,
    chunks: &[Chunk],
    top_k: usize,
    selector: Option<&[usize]>,
) -> Vec<SearchResult> {
    let tokens = tokenize(query);
    if tokens.is_empty() {
        return vec![];
    }
    let mask = selector_to_mask(selector, chunks.len());
    let scores = bm25_index.get_scores(&tokens, mask.as_deref());
    let mut idxs: Vec<usize> = (0..scores.len()).collect();
    idxs.sort_by(|a, b| scores[*b].partial_cmp(&scores[*a]).unwrap());
    idxs.into_iter()
        .take(top_k)
        .filter(|&i| scores[i] > 0.0)
        .map(|i| SearchResult {
            chunk: chunks[i].clone(),
            score: scores[i],
            source: SearchMode::Bm25,
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub fn search_hybrid(
    query: &str,
    model: &impl Encoder,
    semantic_index: &SelectableBasicBackend,
    bm25_index: &Bm25Index,
    chunks: &[Chunk],
    top_k: usize,
    alpha: Option<f32>,
    selector: Option<&[usize]>,
) -> Vec<SearchResult> {
    let alpha_weight = resolve_alpha(query, alpha);
    let candidate_count = top_k * 5;
    let semantic = search_semantic(
        query,
        model,
        semantic_index,
        chunks,
        candidate_count,
        selector,
    );
    let semantic_scores: HashMap<Chunk, f32> =
        semantic.into_iter().map(|r| (r.chunk, r.score)).collect();
    let bm25_scores: HashMap<Chunk, f32> =
        search_bm25(query, bm25_index, chunks, candidate_count, selector)
            .into_iter()
            .map(|r| (r.chunk, r.score))
            .collect();
    let normalized_semantic = rrf_scores(&semantic_scores);
    let normalized_bm25 = rrf_scores(&bm25_scores);
    let mut combined: HashMap<Chunk, f32> = HashMap::new();
    for chunk in normalized_semantic.keys().chain(normalized_bm25.keys()) {
        combined.entry(chunk.clone()).or_insert(0.0);
    }
    for (chunk, score) in combined.iter_mut() {
        *score = alpha_weight * normalized_semantic.get(chunk).copied().unwrap_or(0.0)
            + (1.0 - alpha_weight) * normalized_bm25.get(chunk).copied().unwrap_or(0.0);
    }
    boost_multi_chunk_files(&mut combined);
    let combined = apply_query_boost(combined, query, chunks);
    let ranked = rerank_topk(&combined, top_k, alpha_weight < 1.0);
    ranked
        .into_iter()
        .map(|(chunk, score)| SearchResult {
            chunk,
            score,
            source: SearchMode::Hybrid,
        })
        .collect()
}
