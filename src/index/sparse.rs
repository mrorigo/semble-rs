use std::collections::HashMap;

use crate::tokens::tokenize;
use crate::types::Chunk;

/// Maps query-token term frequencies to the subset of documents that actually
/// contain them via an atomic postings entry.
type Postings = HashMap<String, Vec<(u32, u32)>>;

pub fn selector_to_mask(selector: Option<&[usize]>, size: usize) -> Option<Vec<bool>> {
    selector.map(|indices| {
        let mut mask = vec![false; size];
        for &idx in indices {
            if idx < size {
                mask[idx] = true;
            }
        }
        mask
    })
}

pub fn enrich_for_bm25(chunk: &Chunk) -> String {
    let stem = std::path::Path::new(&chunk.file_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    format!("{} {} {}", chunk.content, stem, stem)
}

/// A BM25 lexical index backed by a term-inverted postings structure for
/// query-time focused scoring.
///
/// Rather than scanning every document and rebuilding a term-frequency table
/// per query, only documents that actually contain at least one query term are
/// visited, which keeps query latency proportional to the size of the query's
/// postings lists rather than the total corpus size.
#[derive(Debug, Clone)]
pub struct Bm25Index {
    doc_len: Vec<u32>,
    avg_len: f32,
    idf: HashMap<String, f32>,
    postings: Postings,
    n: u32,
}

impl Bm25Index {
    pub fn new(docs: Vec<Vec<String>>) -> Self {
        let n = docs.len() as u32;
        let doc_len: Vec<u32> = docs.iter().map(|d| d.len() as u32).collect();
        let avg_len = if doc_len.is_empty() {
            0.0
        } else {
            doc_len.iter().sum::<u32>() as f32 / doc_len.len() as f32
        };

        let mut df: HashMap<String, u32> = HashMap::new();
        let mut postings: Postings = HashMap::new();
        for (doc_idx, doc) in docs.iter().enumerate() {
            let mut tf: HashMap<&str, u32> = HashMap::new();
            for tok in doc {
                *tf.entry(tok.as_str()).or_default() += 1;
            }
            for (tok, freq) in tf {
                *df.entry(tok.to_string()).or_default() += 1;
                postings
                    .entry(tok.to_string())
                    .or_default()
                    .push((doc_idx as u32, freq));
            }
        }

        let idf = df
            .into_iter()
            .map(|(tok, d)| {
                (
                    tok,
                    ((n as f32 - d as f32 + 0.5) / (d as f32 + 0.5) + 1.0).ln(),
                )
            })
            .collect();

        Self {
            doc_len,
            avg_len,
            idf,
            postings,
            n,
        }
    }

    /// Computes BM25 scores for every document against the provided query tokens.
    ///
    /// Implemented over the inverted index so only documents in the postings
    /// lists of the query tokens are touched. Documents never containing a query
    /// term are left at their initial score of zero.
    ///
    /// # Arguments
    ///
    /// * `query_tokens` - The tokenized query terms.
    /// * `mask` - Optional per-document inclusion mask; masked-out documents are skipped.
    ///
    /// # Returns
    ///
    /// A dense vector of BM25 scores, one per document.
    pub fn get_scores(&self, query_tokens: &[String], mask: Option<&[bool]>) -> Vec<f32> {
        let k1 = 1.5_f32;
        let b = 0.75_f32;
        let n = self.n;

        let mut scores = vec![0.0_f32; n as usize];

        for q in query_tokens {
            let Some(idf) = self.idf.get(q).copied() else {
                continue;
            };
            let Some(posts) = self.postings.get(q) else {
                continue;
            };
            for &(doc_idx, freq) in posts {
                let doc_idx = doc_idx as usize;
                if let Some(mask) = mask
                    && !mask[doc_idx]
                {
                    continue;
                }
                let len = self.doc_len[doc_idx] as f32;
                let norm_len = if self.avg_len > 0.0 {
                    len / self.avg_len
                } else {
                    0.0
                };
                let denom = freq as f32 + k1 * (1.0 - b + b * norm_len);
                scores[doc_idx] += idf * (freq as f32 * (k1 + 1.0)) / denom;
            }
        }

        scores
    }
}

pub fn build_index(chunks: &[Chunk]) -> Bm25Index {
    Bm25Index::new(
        chunks
            .iter()
            .map(|c| tokenize(&enrich_for_bm25(c)))
            .collect(),
    )
}
