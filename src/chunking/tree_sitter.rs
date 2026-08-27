// Rust guideline compliant 2026-08-27

use std::collections::HashMap;

use tree_sitter::{Language, Node, Parser, Query, QueryCursor, StreamingIterator};

use crate::chunking::core::{ChunkBoundary, chunk_lines};
use crate::types::{Symbol, SymbolKind};

const STRUCTURAL_MULTIPLIER: usize = 2;

/// Returns per-language tree-sitter query source that captures the declared
/// name of each definition, keyed by the capture name used for each roll kind.
fn language_queries(
    language: &str,
) -> Option<(&'static str, &'static [(&'static str, SymbolKind)])> {
    let (query, mappings): (&str, &[(&str, SymbolKind)]) = match language {
        "rust" => (
            r#"
(function_item name: (identifier) @fn)
(function_signature_item name: (identifier) @fn)
(struct_item name: (type_identifier) @struct)
(enum_item name: (type_identifier) @enum)
(trait_item name: (type_identifier) @trait)
(mod_item name: (identifier) @module)
(const_item name: (identifier) @const)
(static_item name: (identifier) @const)
(type_item name: (type_identifier) @type)
"#,
            &[
                ("fn", SymbolKind::Function),
                ("struct", SymbolKind::Struct),
                ("enum", SymbolKind::Enum),
                ("trait", SymbolKind::Trait),
                ("module", SymbolKind::Module),
                ("const", SymbolKind::Constant),
                ("type", SymbolKind::Type),
            ],
        ),
        "python" => (
            r#"
(function_definition name: (identifier) @fn)
(class_definition name: (identifier) @class)
"#,
            &[("fn", SymbolKind::Function), ("class", SymbolKind::Class)],
        ),
        "javascript" => (
            r#"
(function_declaration name: (identifier) @fn)
(method_definition name: (property_identifier) @method)
(class_declaration name: (identifier) @class)
"#,
            &[
                ("fn", SymbolKind::Function),
                ("method", SymbolKind::Method),
                ("class", SymbolKind::Class),
            ],
        ),
        "typescript" => (
            r#"
(function_declaration name: (identifier) @fn)
(method_definition name: (property_identifier) @method)
(class_declaration name: (type_identifier) @class)
(abstract_class_declaration name: (type_identifier) @class)
(interface_declaration name: (type_identifier) @interface)
(type_alias_declaration name: (type_identifier) @type)
(enum_declaration name: (identifier) @enum)
"#,
            &[
                ("fn", SymbolKind::Function),
                ("method", SymbolKind::Method),
                ("class", SymbolKind::Class),
                ("interface", SymbolKind::Interface),
                ("type", SymbolKind::Type),
                ("enum", SymbolKind::Enum),
            ],
        ),
        "java" => (
            r#"
(class_declaration name: (identifier) @class)
(record_declaration name: (identifier) @class)
(interface_declaration name: (identifier) @interface)
(enum_declaration name: (identifier) @enum)
(method_declaration name: (identifier) @method)
"#,
            &[
                ("class", SymbolKind::Class),
                ("interface", SymbolKind::Interface),
                ("enum", SymbolKind::Enum),
                ("method", SymbolKind::Method),
            ],
        ),
        "go" => (
            r#"
(function_declaration name: (identifier) @fn)
(method_declaration name: (field_identifier) @method)
(type_spec name: (type_identifier) @type)
(const_spec name: (identifier) @const)
"#,
            &[
                ("fn", SymbolKind::Function),
                ("method", SymbolKind::Method),
                ("type", SymbolKind::Type),
                ("const", SymbolKind::Constant),
            ],
        ),
        "c" => (
            r#"
(function_definition declarator: (function_declarator declarator: (identifier) @fn))
(struct_specifier name: (type_identifier) @struct)
(enum_specifier name: (type_identifier) @enum)
(union_specifier name: (type_identifier) @struct)
"#,
            &[
                ("fn", SymbolKind::Function),
                ("struct", SymbolKind::Struct),
                ("enum", SymbolKind::Enum),
            ],
        ),
        "cpp" => (
            r#"
(function_definition declarator: (function_declarator declarator: (identifier) @fn))
(class_specifier name: (type_identifier) @class)
(struct_specifier name: (type_identifier) @struct)
(enum_specifier name: (type_identifier) @enum)
(union_specifier name: (type_identifier) @struct)
(alias_declaration name: (type_identifier) @type)
"#,
            &[
                ("fn", SymbolKind::Function),
                ("class", SymbolKind::Class),
                ("struct", SymbolKind::Struct),
                ("enum", SymbolKind::Enum),
                ("type", SymbolKind::Type),
            ],
        ),
        "ruby" => (
            r#"
(method name: (_) @method)
(singleton_method name: (_) @method)
(class name: (constant) @class)
(module name: (constant) @module)
"#,
            &[
                ("method", SymbolKind::Method),
                ("class", SymbolKind::Class),
                ("module", SymbolKind::Module),
            ],
        ),
        _ => return None,
    };
    Some((query, mappings))
}

/// Extracts the declared symbol names within a byte range of `source` using
/// per-language tree-sitter queries.
///
/// Best-effort: returns an empty vec for unsupported or doc languages, or when
/// a query fails to compile for a given language.
pub fn extract_definitions(
    source: &str,
    language: &str,
    start_byte: usize,
    end_byte: usize,
) -> Vec<Symbol> {
    let Some(mut parser) = parser_for(language) else {
        return vec![];
    };
    let Some(tree) = parser.parse(source, None) else {
        return vec![];
    };
    let Some((query, kinds)) = compile_language_query(language) else {
        return vec![];
    };
    let mut cursor = QueryCursor::new();
    query_symbols(
        &query,
        &kinds,
        &mut cursor,
        source,
        &tree,
        start_byte,
        end_byte,
    )
}

/// Compiles the definition query for a language into its query and kind map.
///
/// Returns `None` when the language has no patterns or the query fails to
/// compile (best-effort, so a broken pattern is a no-op rather than a panic).
fn compile_language_query(language: &str) -> Option<(Query, HashMap<&'static str, SymbolKind>)> {
    let (query_source, mappings) = language_queries(language)?;
    let lang = tree_sitter_language(language)?;
    let Ok(query) = Query::new(&lang, query_source) else {
        return None;
    };
    let kinds: HashMap<&'static str, SymbolKind> =
        mappings.iter().map(|(name, kind)| (*name, *kind)).collect();
    Some((query, kinds))
}

/// Collects the symbols declared in `[start_byte, end_byte)` of `source`,
/// querying the already-parsed `tree` with a reusable cursor.
fn query_symbols(
    query: &Query,
    kinds: &HashMap<&'static str, SymbolKind>,
    cursor: &mut QueryCursor,
    source: &str,
    tree: &tree_sitter::Tree,
    start_byte: usize,
    end_byte: usize,
) -> Vec<Symbol> {
    let mut symbols = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut captures = cursor.captures(query, tree.root_node(), source.as_bytes());
    while let Some((matched, capture_idx)) = captures.next() {
        let capture = &matched.captures[*capture_idx];
        let node = capture.node;
        if node.start_byte() < start_byte || node.end_byte() > end_byte {
            continue;
        }
        let capture_name = query.capture_names()[capture.index as usize];
        let Some(&kind) = kinds.get(capture_name) else {
            continue;
        };
        let Ok(name) = node.utf8_text(source.as_bytes()) else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        let lowered = name.to_lowercase();
        let line = source[..node.start_byte()].matches('\n').count() + 1;
        if seen.insert((capture.index, lowered.clone(), line)) {
            symbols.push(Symbol {
                name: lowered,
                kind,
                line,
            });
        }
    }
    symbols
}

pub fn chunk(source: &str, language: &str, desired_length: usize) -> Vec<ChunkBoundary> {
    parse_and_chunk(source, language, desired_length)
        .map(|(boundaries, _)| boundaries)
        .unwrap_or_else(|| chunk_lines(source, desired_length))
}

/// Chunks `source` structurally and returns both the boundaries and the
/// declared symbols per boundary. Parses the source exactly once.
pub fn chunk_with_symbols(
    source: &str,
    language: &str,
    desired_length: usize,
) -> (Vec<ChunkBoundary>, Vec<Vec<Symbol>>) {
    let Some((boundaries, tree)) = parse_and_chunk(source, language, desired_length) else {
        let bounds = chunk_lines(source, desired_length);
        let n = bounds.len();
        return (bounds, vec![Vec::new(); n]);
    };
    let symbols = match compile_language_query(language) {
        Some((query, kinds)) => {
            let mut cursor = QueryCursor::new();
            boundaries
                .iter()
                .map(|b| query_symbols(&query, &kinds, &mut cursor, source, &tree, b.start, b.end))
                .collect()
        }
        None => vec![Vec::new(); boundaries.len()],
    };
    (boundaries, symbols)
}

/// Parses `source` once and computes merge-ordered structural boundaries.
fn parse_and_chunk(
    source: &str,
    language: &str,
    desired_length: usize,
) -> Option<(Vec<ChunkBoundary>, tree_sitter::Tree)> {
    let mut parser = parser_for(language)?;
    let tree = parser.parse(source, None)?;
    let mut boundaries = Vec::new();
    collect_boundaries(tree.root_node(), desired_length, &mut boundaries);
    if boundaries.is_empty() {
        return None;
    }
    boundaries.sort_by_key(|b| (b.start, b.end));
    Some((merge_boundaries(boundaries, desired_length), tree))
}

fn parser_for(language: &str) -> Option<Parser> {
    let lang = tree_sitter_language(language)?;
    let mut parser = Parser::new();
    if parser.set_language(&lang).is_err() {
        return None;
    }
    Some(parser)
}

/// The `tree_sitter::Language` for a supported structural language, if any.
fn tree_sitter_language(language: &str) -> Option<Language> {
    Some(match language {
        "rust" => tree_sitter_rust::LANGUAGE.into(),
        "python" => tree_sitter_python::LANGUAGE.into(),
        "javascript" => tree_sitter_javascript::LANGUAGE.into(),
        "typescript" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        "java" => tree_sitter_java::LANGUAGE.into(),
        "go" => tree_sitter_go::LANGUAGE.into(),
        "c" => tree_sitter_c::LANGUAGE.into(),
        "cpp" => tree_sitter_cpp::LANGUAGE.into(),
        "ruby" => tree_sitter_ruby::LANGUAGE.into(),
        "markdown" => tree_sitter_md::LANGUAGE.into(),
        "json" => tree_sitter_json::LANGUAGE.into(),
        "yaml" => tree_sitter_yaml::LANGUAGE.into(),
        _ => return None,
    })
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
            // Ruby
            | "singleton_class"
            | "singleton_method"
            | "method"
            // JSON
            | "object"
            | "array"
            // YAML
            | "stream"
            | "block_mapping"
            | "block_mapping_pair"
            | "block_sequence"
            | "block_node"
    )
}

#[cfg(test)]
mod tests {
    use super::{chunk, extract_definitions};
    use crate::types::SymbolKind;

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

    #[test]
    fn chunks_ruby_source_structurally() {
        let source = r#"
class Greeter
  def initialize(name)
    @name = name
  end

  def hello
    "Hello, #{@name}!"
  end
end

module Helpers
  def greet(who)
    puts hello
  end
end
"#;
        let boundaries = chunk(source, "ruby", 30);
        assert!(!boundaries.is_empty());
        assert!(boundaries.iter().any(|b| b.end - b.start >= 10));
    }

    #[test]
    fn chunks_json_source_structurally() {
        let source = r#"{
  "name": "semble",
  "nested": {
    "enabled": true,
    "tags": ["rust", "mcp", "semantic"]
  },
  "count": 42
}
"#;
        let boundaries = chunk(source, "json", 30);
        assert!(!boundaries.is_empty());
    }

    #[test]
    fn chunks_yaml_source_structurally() {
        let source = r#"
name: semble
server:
  transport: stdio
  port: 8080
features:
  - semantic
  - lexical
  - hybrid
"#;
        let boundaries = chunk(source, "yaml", 30);
        assert!(!boundaries.is_empty());
    }

    #[test]
    fn extracts_rust_definitions() {
        let source = r#"
fn one(a: i32) -> i32 { a }
struct Config { name: String }
enum Kind { A, B }
trait Greet { fn hi(); }
const MAX: usize = 10;
"#;
        let defs = extract_definitions(source, "rust", 0, source.len());
        let names: Vec<(&str, SymbolKind)> =
            defs.iter().map(|s| (s.name.as_str(), s.kind)).collect();
        assert!(names.contains(&("one", SymbolKind::Function)));
        assert!(names.contains(&("config", SymbolKind::Struct)));
        assert!(names.contains(&("kind", SymbolKind::Enum)));
        assert!(names.contains(&("greet", SymbolKind::Trait)));
        assert!(names.contains(&("hi", SymbolKind::Function)));
        assert!(names.contains(&("max", SymbolKind::Constant)));
    }

    #[test]
    fn extracts_python_definitions() {
        let source = "# comment\n\ndef compute(x):\n    return x\n\nclass Loader:\n    def run(self):\n        pass\n";
        let defs = extract_definitions(source, "python", 0, source.len());
        assert!(
            defs.iter()
                .any(|s| s.name == "compute" && s.kind == SymbolKind::Function)
        );
        assert!(
            defs.iter()
                .any(|s| s.name == "loader" && s.kind == SymbolKind::Class)
        );
        // Python methods are function_definition nodes.
        assert!(
            defs.iter()
                .any(|s| s.name == "run" && s.kind == SymbolKind::Function)
        );
    }

    #[test]
    fn extracts_ruby_definitions() {
        let source = "class Greeter\n  def hello\n    'hi'\n  end\nend\n\nmodule Helpers\n  def greet\n    hello\n  end\nend\n";
        let defs = extract_definitions(source, "ruby", 0, source.len());
        assert!(
            defs.iter()
                .any(|s| s.name == "greeter" && s.kind == SymbolKind::Class)
        );
        assert!(
            defs.iter()
                .any(|s| s.name == "helpers" && s.kind == SymbolKind::Module)
        );
        assert!(
            defs.iter()
                .any(|s| s.name == "hello" && s.kind == SymbolKind::Method)
        );
        assert!(
            defs.iter()
                .any(|s| s.name == "greet" && s.kind == SymbolKind::Method)
        );
    }

    #[test]
    fn extracts_typescript_definitions() {
        let source = "export interface Task { id: number }\ntype UserId = string;\nexport function fetchTask(id: number) { return id; }\nclass Repo implements Task {}\n";
        let defs = extract_definitions(source, "typescript", 0, source.len());
        assert!(
            defs.iter()
                .any(|s| s.name == "task" && s.kind == SymbolKind::Interface)
        );
        assert!(
            defs.iter()
                .any(|s| s.name == "userid" && s.kind == SymbolKind::Type)
        );
        assert!(
            defs.iter()
                .any(|s| s.name == "fetchtask" && s.kind == SymbolKind::Function)
        );
        assert!(
            defs.iter()
                .any(|s| s.name == "repo" && s.kind == SymbolKind::Class)
        );
    }

    #[test]
    fn extracts_go_definitions() {
        let source = "package main\n\nfunc Add(a, b int) int { return a + b }\n\ntype Store struct { mu sync.Mutex }\n\nfunc (s *Store) Get() string { return \"\" }\n";
        let defs = extract_definitions(source, "go", 0, source.len());
        assert!(
            defs.iter()
                .any(|s| s.name == "add" && s.kind == SymbolKind::Function)
        );
        assert!(
            defs.iter()
                .any(|s| s.name == "store" && s.kind == SymbolKind::Type)
        );
        assert!(
            defs.iter()
                .any(|s| s.name == "get" && s.kind == SymbolKind::Method)
        );
    }

    #[test]
    fn extracts_java_definitions() {
        let source = "public class Main {\n  public int compute() { return 1; }\n}\ninterface Reader { int read(); }\n";
        let defs = extract_definitions(source, "java", 0, source.len());
        assert!(
            defs.iter()
                .any(|s| s.name == "main" && s.kind == SymbolKind::Class)
        );
        assert!(
            defs.iter()
                .any(|s| s.name == "compute" && s.kind == SymbolKind::Method)
        );
        assert!(
            defs.iter()
                .any(|s| s.name == "reader" && s.kind == SymbolKind::Interface)
        );
        assert!(
            defs.iter()
                .any(|s| s.name == "read" && s.kind == SymbolKind::Method)
        );
    }
}
