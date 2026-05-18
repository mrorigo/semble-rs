use super::boosting::is_symbol_query;

pub fn resolve_alpha(query: &str, alpha: Option<f32>) -> f32 {
    alpha.unwrap_or_else(|| if is_symbol_query(query) { 0.3 } else { 0.5 })
}
