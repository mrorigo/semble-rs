pub use crate::index::model::{
    StaticModel, embed_chunks, hash_embed_text as embed_text, load_model,
};

/// A flat cosine-similarity dense vector backend over runtime-dimensional
/// embeddings.
///
/// Vectors may have any consistent dimension (e.g. 256 for the hashing
/// fallback, or a real model's native dimension such as 768); the backend never
/// assumes a fixed width.
#[derive(Debug, Clone)]
pub struct SelectableBasicBackend {
    vectors: Vec<Vec<f32>>,
    norms: Vec<f32>,
}

impl SelectableBasicBackend {
    pub fn new(vectors: Vec<Vec<f32>>) -> Self {
        let norms = vectors.iter().map(|v| norm(v)).collect();
        Self { vectors, norms }
    }

    fn cosine_distance(a: &[f32], an: f32, b: &[f32], bn: f32) -> f32 {
        let mut dot = 0.0;
        for i in 0..a.len() {
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
        vectors: &[Vec<f32>],
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

fn norm(v: &[f32]) -> f32 {
    v.iter().map(|x| x * x).sum::<f32>().sqrt()
}

#[cfg(test)]
mod tests {
    use super::SelectableBasicBackend;

    #[test]
    fn cosine_ranks_nearest_at_custom_dimension() {
        let backend = SelectableBasicBackend::new(vec![
            vec![1.0f32; 768],
            vec![0.7f32; 768],
            vec![-1.0f32; 768],
        ]);
        let query: Vec<Vec<f32>> = vec![vec![1.0f32; 768]];
        let (indices, _) = backend.query(&query, 3, None).into_iter().next().unwrap();
        assert_eq!(indices, vec![0, 1, 2]);
    }

    #[test]
    fn cosine_handles_zero_vectors() {
        let backend = SelectableBasicBackend::new(vec![vec![0.0f32; 768]]);
        let query: Vec<Vec<f32>> = vec![vec![1.0f32; 768]];
        let (_, scores) = backend.query(&query, 1, None).into_iter().next().unwrap();
        assert_eq!(scores[0], 1.0);
    }
}
