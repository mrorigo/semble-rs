use std::collections::BTreeSet;
use std::path::Path;

pub const DOC_LANGUAGES: [&str; 3] = ["markdown", "json", "yaml"];

pub fn detect_language(path: &Path) -> Option<String> {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "py" | "pyi" | "pyw" => Some("python".to_string()),
        "rs" => Some("rust".to_string()),
        "js" | "jsx" | "mjs" | "cjs" => Some("javascript".to_string()),
        "ts" | "tsx" | "mts" | "cts" => Some("typescript".to_string()),
        "java" => Some("java".to_string()),
        "go" => Some("go".to_string()),
        "c" => Some("c".to_string()),
        "h" => Some("c".to_string()),
        "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" => Some("cpp".to_string()),
        "rb" => Some("ruby".to_string()),
        "md" | "markdown" => Some("markdown".to_string()),
        "json" => Some("json".to_string()),
        "yaml" | "yml" => Some("yaml".to_string()),
        _ => None,
    }
}

pub fn get_extensions(include_text_files: bool, additional: Option<&[&str]>) -> Vec<String> {
    let mut exts: BTreeSet<String> = [
        ".rs", ".py", ".js", ".jsx", ".mjs", ".cjs", ".ts", ".tsx", ".mts", ".cts",
        ".java", ".go", ".c", ".h", ".cc", ".cpp", ".cxx", ".hpp", ".hh", ".hxx",
        ".rb",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    if include_text_files {
        exts.extend(
            [".md", ".markdown", ".json", ".yaml", ".yml"]
                .into_iter()
                .map(String::from),
        );
    }
    if let Some(additional) = additional {
        for ext in additional {
            exts.insert((*ext).to_string());
        }
    }
    exts.into_iter().collect()
}
