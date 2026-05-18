use crate::types::EMBED_DIM;

pub use crate::index::model::{
    StaticModel, embed_chunks, hash_embed_text as embed_text, load_model,
};

#[derive(Debug, Clone)]
pub struct SelectableBasicBackend {
    vectors: Vec<[f32; EMBED_DIM]>,
}

impl SelectableBasicBackend {
    pub fn new(vectors: Vec<[f32; EMBED_DIM]>) -> Self {
        Self { vectors }
    }

    fn cosine_distance(a: &[f32; EMBED_DIM], b: &[f32; EMBED_DIM]) -> f32 {
        let mut dot = 0.0;
        let mut an = 0.0;
        let mut bn = 0.0;
        for i in 0..EMBED_DIM {
            dot += a[i] * b[i];
            an += a[i] * a[i];
            bn += b[i] * b[i];
        }
        if an == 0.0 || bn == 0.0 {
            1.0
        } else {
            1.0 - dot / (an.sqrt() * bn.sqrt())
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
                let mut pairs: Vec<(usize, f32)> = indices
                    .iter()
                    .map(|&i| (i, Self::cosine_distance(query, &self.vectors[i])))
                    .collect();
                pairs.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
                let pairs = pairs.into_iter().take(effective_k).collect::<Vec<_>>();
                (
                    pairs.iter().map(|p| p.0).collect(),
                    pairs.iter().map(|p| p.1).collect(),
                )
            })
            .collect()
    }
}
