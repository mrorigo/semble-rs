use crate::types::EMBED_DIM;

pub use crate::index::model::{
    StaticModel, embed_chunks, hash_embed_text as embed_text, load_model,
};

#[derive(Debug, Clone)]
pub struct SelectableBasicBackend {
    vectors: Vec<[f32; EMBED_DIM]>,
    norms: Vec<f32>,
}

impl SelectableBasicBackend {
    pub fn new(vectors: Vec<[f32; EMBED_DIM]>) -> Self {
        let norms = vectors.iter().map(norm).collect();
        Self { vectors, norms }
    }

    fn cosine_distance(a: &[f32; EMBED_DIM], an: f32, b: &[f32; EMBED_DIM], bn: f32) -> f32 {
        let mut dot = 0.0;
        for i in 0..EMBED_DIM {
            dot += a[i] * b[i];
        }
        if an == 0.0 || bn == 0.0 {
            1.0
        } else {
            1.0 - dot / (an * bn)
        }
    }

    pub fn query(
        &self,
        vectors: &[[f32; EMBED_DIM]],
        k: usize,
        selector: Option<&[usize]>,
    ) -> Vec<(Vec<usize>, Vec<f32>)> {
        if k < 1 {
            panic!("k should be >= 1");
        }
        let indices: Vec<usize> = selector
            .map(|s| s.to_vec())
            .unwrap_or_else(|| (0..self.vectors.len()).collect());
        let effective_k = k.min(indices.len());
        vectors
            .iter()
            .map(|query| {
                let query_norm = norm(query);
                let mut pairs: Vec<(usize, f32)> = indices
                    .iter()
                    .map(|&i| {
                        (
                            i,
                            Self::cosine_distance(
                                query,
                                query_norm,
                                &self.vectors[i],
                                self.norms[i],
                            ),
                        )
                    })
                    .collect();
                if let Some(pivot) = effective_k.checked_sub(1) {
                    pairs.select_nth_unstable_by(pivot, |a, b| a.1.partial_cmp(&b.1).unwrap());
                }
                let pairs = pairs.into_iter().take(effective_k).collect::<Vec<_>>();
                (
                    pairs.iter().map(|p| p.0).collect(),
                    pairs.iter().map(|p| p.1).collect(),
                )
            })
            .collect()
    }
}

fn norm(v: &[f32; EMBED_DIM]) -> f32 {
    v.iter().map(|x| x * x).sum::<f32>().sqrt()
}
