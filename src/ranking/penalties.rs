use std::path::Path;

use crate::types::Chunk;

const STRONG_PENALTY: f32 = 0.3;
const MODERATE_PENALTY: f32 = 0.5;
const MILD_PENALTY: f32 = 0.7;
const FILE_SATURATION_THRESHOLD: usize = 1;
const FILE_SATURATION_DECAY: f32 = 0.5;

pub fn rerank_topk(
    scores: &std::collections::HashMap<Chunk, f32>,
    top_k: usize,
    penalise_paths: bool,
) -> Vec<(Chunk, f32)> {
    if scores.is_empty() {
        return vec![];
    }
    let mut penalised = std::collections::HashMap::new();
    for (chunk, score) in scores {
        penalised.insert(
            chunk.clone(),
            if penalise_paths {
                score * file_path_penalty(&chunk.file_path)
            } else {
                *score
            },
        );
    }
    let mut ranked: Vec<_> = penalised.iter().collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap());
    let mut file_selected: HashMap<String, usize> = HashMap::new();
    let mut selected = Vec::new();
    for (chunk, score) in ranked {
        let already = *file_selected.get(&chunk.file_path).unwrap_or(&0);
        let mut eff = *score;
        if already >= FILE_SATURATION_THRESHOLD {
            eff *= FILE_SATURATION_DECAY.powi((already - FILE_SATURATION_THRESHOLD + 1) as i32);
        }
        selected.push(((*chunk).clone(), eff));
        file_selected.insert(chunk.file_path.clone(), already + 1);
    }
    selected.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    selected.truncate(top_k);
    selected
}

fn file_path_penalty(file_path: &str) -> f32 {
    let normalised = file_path.replace('\\', "/");
    let mut penalty = 1.0;
    if normalised.contains("test/")
        || normalised.contains("tests/")
        || normalised.ends_with("_test.rs")
        || normalised.contains("/tests/")
        || normalised.contains("spec/")
    {
        penalty *= STRONG_PENALTY;
    }
    if Path::new(file_path)
        .file_name()
        .and_then(|s| s.to_str())
        .is_some_and(|n| n == "__init__.py")
    {
        penalty *= MODERATE_PENALTY;
    }
    if normalised.contains("compat")
        || normalised.contains("legacy")
        || normalised.contains("examples")
    {
        penalty *= STRONG_PENALTY;
    }
    if normalised.ends_with(".d.ts") {
        penalty *= MILD_PENALTY;
    }
    penalty
}
use std::collections::HashMap;
