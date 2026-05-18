use tree_sitter::{Language, Node, Parser};

use crate::chunking::core::{ChunkBoundary, chunk_lines};

const STRUCTURAL_MULTIPLIER: usize = 2;

pub fn chunk(source: &str, language: &str, desired_length: usize) -> Vec<ChunkBoundary> {
    let Some(mut parser) = parser_for(language) else {
        return chunk_lines(source, desired_length);
    };
    let Some(tree) = parser.parse(source, None) else {
        return chunk_lines(source, desired_length);
    };

    let mut boundaries = Vec::new();
    let root = tree.root_node();
    collect_boundaries(root, desired_length, &mut boundaries);
    if boundaries.is_empty() {
        return chunk_lines(source, desired_length);
    }

    boundaries.sort_by_key(|b| (b.start, b.end));
    merge_boundaries(boundaries, desired_length)
}

fn parser_for(language: &str) -> Option<Parser> {
    let mut parser = Parser::new();
    let lang: Language = match language {
        "rust" => tree_sitter_rust::LANGUAGE.into(),
        "python" => tree_sitter_python::LANGUAGE.into(),
        "javascript" => tree_sitter_javascript::LANGUAGE.into(),
        "typescript" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        "java" => tree_sitter_java::LANGUAGE.into(),
        "go" => tree_sitter_go::LANGUAGE.into(),
        "c" => tree_sitter_c::LANGUAGE.into(),
        "cpp" => tree_sitter_cpp::LANGUAGE.into(),
        "markdown" => tree_sitter_md::LANGUAGE.into(),
        _ => return None,
    };
    if parser.set_language(&lang).is_err() {
        return None;
    }
    Some(parser)
}

fn collect_boundaries(node: Node, desired_length: usize, out: &mut Vec<ChunkBoundary>) {
    if node.is_error() || node.is_missing() {
        return;
    }
    let len = node.end_byte().saturating_sub(node.start_byte());
    if len == 0 {
        return;
    }

    if is_structural(node.kind())
        && (len <= desired_length * STRUCTURAL_MULTIPLIER || node.child_count() == 0)
    {
        out.push(ChunkBoundary {
            start: node.start_byte(),
            end: node.end_byte(),
        });
        return;
    }

    let mut saw_named_child = false;
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        saw_named_child = true;
        if child.end_byte().saturating_sub(child.start_byte())
            > desired_length * STRUCTURAL_MULTIPLIER
            && child.named_child_count() > 0
        {
            collect_boundaries(child, desired_length, out);
        } else if !child.is_error() && !child.is_missing() {
            out.push(ChunkBoundary {
                start: child.start_byte(),
                end: child.end_byte(),
            });
        }
    }

    if !saw_named_child {
        out.push(ChunkBoundary {
            start: node.start_byte(),
            end: node.end_byte(),
        });
    }
}

fn merge_boundaries(
    mut boundaries: Vec<ChunkBoundary>,
    desired_length: usize,
) -> Vec<ChunkBoundary> {
    let mut merged = Vec::new();
    let mut current: Option<ChunkBoundary> = None;

    for boundary in boundaries.drain(..) {
        match &mut current {
            None => current = Some(boundary),
            Some(chunk) => {
                let candidate_len = boundary.end.saturating_sub(chunk.start);
                let is_adjacent = boundary.start <= chunk.end + 1;
                if is_adjacent && candidate_len <= desired_length * STRUCTURAL_MULTIPLIER {
                    chunk.end = chunk.end.max(boundary.end);
                } else {
                    merged.push(*chunk);
                    current = Some(boundary);
                }
            }
        }
    }

    if let Some(chunk) = current {
        merged.push(chunk);
    }
    merged
}

fn is_structural(kind: &str) -> bool {
    matches!(
        kind,
        "function_item"
            | "impl_item"
            | "struct_item"
            | "enum_item"
            | "trait_item"
            | "mod_item"
            | "type_item"
            | "const_item"
            | "static_item"
            | "function_definition"
            | "class_definition"
            | "decorated_definition"
            | "method_definition"
            | "class_declaration"
            | "function_declaration"
            | "method_declaration"
            | "type_declaration"
            | "interface_declaration"
            | "package_clause"
            | "import_declaration"
            | "package_declaration"
            | "source_file"
            | "module"
            | "translation_unit"
            | "document"
            | "section"
            | "atx_heading"
            | "setext_heading"
            | "paragraph"
            | "fenced_code_block"
            | "indented_code_block"
            | "list"
            | "list_item"
            | "block_quote"
    )
}

#[cfg(test)]
mod tests {
    use super::chunk;

    #[test]
    fn chunks_rust_source_structurally() {
        let source = r#"
fn one() {
    println!("one");
}

fn two() {
    println!("two");
}
"#;
        let boundaries = chunk(source, "rust", 30);
        assert!(!boundaries.is_empty());
        assert!(boundaries.iter().any(|b| b.end - b.start >= 10));
    }

    #[test]
    fn chunks_javascript_source_structurally() {
        let source = r#"
function first() {
  return 1;
}

function second() {
  return 2;
}
"#;
        let boundaries = chunk(source, "javascript", 30);
        assert!(!boundaries.is_empty());
    }

    #[test]
    fn chunks_markdown_source_structurally() {
        let source = r#"# Title

Intro paragraph.

## Section

- item one
- item two

```rust
fn example() {}
```
"#;
        let boundaries = chunk(source, "markdown", 30);
        assert!(!boundaries.is_empty());
    }
}
