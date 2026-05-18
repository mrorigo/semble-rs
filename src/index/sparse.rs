use std::collections::HashMap;

use crate::tokens::tokenize;
use crate::types::Chunk;

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

#[derive(Debug, Clone)]
pub struct Bm25Index {
    docs: Vec<Vec<String>>,
    doc_len: Vec<usize>,
    avg_len: f32,
    idf: HashMap<String, f32>,
}

impl Bm25Index {
    pub fn new(docs: Vec<Vec<String>>) -> Self {
        let doc_len: Vec<usize> = docs.iter().map(|d| d.len()).collect();
        let avg_len = if doc_len.is_empty() {
            0.0
        } else {
            doc_len.iter().sum::<usize>() as f32 / doc_len.len() as f32
        };
        let mut df: HashMap<String, usize> = HashMap::new();
        for doc in &docs {
            let mut seen = std::collections::BTreeSet::new();
            for tok in doc {
                if seen.insert(tok.clone()) {
                    *df.entry(tok.clone()).or_default() += 1;
                }
            }
        }
        let n = docs.len() as f32;
        let idf = df
            .into_iter()
            .map(|(tok, d)| (tok, ((n - d as f32 + 0.5) / (d as f32 + 0.5) + 1.0).ln()))
            .collect();
        Self {
            docs,
            doc_len,
            avg_len,
            idf,
        }
    }

    pub fn get_scores(&self, query_tokens: &[String], mask: Option<&[bool]>) -> Vec<f32> {
        let k1 = 1.5;
        let b = 0.75;
        let mut scores = vec![0.0; self.docs.len()];
        for (i, doc) in self.docs.iter().enumerate() {
            if let Some(mask) = mask
                && !mask[i]
            {
                continue;
            }
            let mut tf: HashMap<&String, usize> = HashMap::new();
            for tok in doc {
                *tf.entry(tok).or_default() += 1;
            }
            let len = self.doc_len[i] as f32;
            for q in query_tokens {
                if let Some(&freq) = tf.get(q) {
                    let idf = *self.idf.get(q).unwrap_or(&0.0);
                    let freq = freq as f32;
                    let denom = freq
                        + k1 * (1.0 - b
                            + b * if self.avg_len > 0.0 {
                                len / self.avg_len
                            } else {
                                0.0
                            });
                    scores[i] += idf * (freq * (k1 + 1.0)) / denom;
                }
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
