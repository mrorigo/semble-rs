// Rust guideline compliant 2026-05-18

#![allow(dead_code)]

//! Common benchmarking utilities for corpus generation.
//!
//! This module provides functions to procedurally generate mock repositories
//! of varying sizes (small, medium, large) to realistically benchmark
//! indexing and searching performance in `semble-rs`.

use std::fs::{self, File};
use std::io::Write;
use tempfile::TempDir;

/// Represents a configuration for a mock corpus tier.
#[derive(Debug, Clone, Copy)]
pub struct CorpusConfig {
    /// Number of files to generate in this corpus.
    pub file_count: usize,
    /// Average number of lines per file.
    pub avg_lines: usize,
}

impl CorpusConfig {
    /// Predefined configuration for a small repository (e.g., small CLI).
    pub const SMALL: Self = Self {
        file_count: 50,
        avg_lines: 200,
    };

    /// Predefined configuration for a medium repository (e.g., mid-sized service).
    pub const MEDIUM: Self = Self {
        file_count: 300,
        avg_lines: 300,
    };

    /// Predefined configuration for a large repository (e.g., large monorepo shard).
    pub const LARGE: Self = Self {
        file_count: 1500,
        avg_lines: 350,
    };
}

/// Generates a realistic mock Rust source file content with specified lines.
///
/// # Arguments
///
/// * `lines` - The desired minimum number of lines in the generated file.
/// * `file_index` - An index identifier to ensure name uniqueness.
///
/// # Returns
///
/// A `String` containing valid, syntactically correct Rust source code with docstrings,
/// struct definitions, and functions.
pub fn generate_realistic_rust_file(lines: usize, file_index: usize) -> String {
    let mut out = String::new();
    out.push_str("// Rust guideline compliant 2026-05-18\n\n");
    out.push_str(&format!(
        "//! Auto-generated benchmark module for file index {}.\n//!\n\
         //! This file is procedurally generated to simulate realistic AST structures.\n\n",
        file_index
    ));
    out.push_str(&format!(
        "/// A sample struct representing data for component {}.\n",
        file_index
    ));
    out.push_str("#[derive(Debug, Clone)]\n");
    out.push_str("pub struct ComponentController {\n");
    out.push_str("    pub component_id: usize,\n");
    out.push_str("    pub name: String,\n");
    out.push_str("    pub active: bool,\n");
    out.push_str("}\n\n");

    let mut current_lines = 14;
    let mut fn_idx = 0;

    while current_lines < lines {
        out.push_str(&format!(
            "    /// Performs operation {} on the controller.\n    ///\n\
                 /// # Arguments\n    ///\n\
                 /// * `value` - Input multiplier.\n    ///\n\
                 /// # Errors\n    ///\n\
                 /// Returns an error if the component is inactive.\n\
                 pub fn perform_op_{}(&mut self, value: usize) -> Result<usize, String> {{\n\
                 if !self.active {{\n\
                     return Err(\"Component is currently inactive\".to_string());\n\
                 }}\n\
                 self.component_id = self.component_id.wrapping_add(value);\n\
                 if self.component_id % 7 == 0 {{\n\
                     self.active = false;\n\
                 }}\n\
                 Ok(self.component_id)\n\
             }}\n\n",
            fn_idx, fn_idx
        ));
        current_lines += 17;
        fn_idx += 1;
    }

    out
}

/// Generates a realistic mock Markdown file content with specified lines.
///
/// # Arguments
///
/// * `lines` - The desired minimum number of lines in the generated file.
/// * `file_index` - An index identifier to ensure name uniqueness.
///
/// # Returns
///
/// A `String` containing valid Markdown source code.
pub fn generate_realistic_markdown_file(lines: usize, file_index: usize) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "# Documentation for Module {}\n\n\
         This is an auto-generated documentation page designed for semantic and lexical search indexing.\n\n\
         ## Overview\n\n\
         The system utilizes advanced high-performance retrieval techniques to analyze software structures.\n\n",
        file_index
    ));

    let mut current_lines = 10;
    let mut section_idx = 0;

    while current_lines < lines {
        out.push_str(&format!(
            "### Section {}\n\n\
             Lorem ipsum dolor sit amet, consectetur adipiscing elit. Integer nec odio. Praesent libero.\n\
             Sed cursus ante dapibus diam. Sed nisi. Nulla quis sem at nibh elementum imperdiet.\n\n\
             - Bullet point number one for detailed search keyword checking.\n\
             - Another item focusing on key concepts of authentication and model performance.\n\n",
            section_idx
        ));
        current_lines += 8;
        section_idx += 1;
    }

    out
}

/// Creates a temporary directory filled with a procedurally generated corpus of files.
///
/// # Arguments
///
/// * `config` - The configuration tier specifying the size of the corpus.
///
/// # Returns
///
/// A `Result` containing the `TempDir` populated with mock files, or an `std::io::Error` on failure.
///
/// # Errors
///
/// This function will return an error if files or directories cannot be created.
pub fn create_mock_corpus(config: CorpusConfig) -> Result<TempDir, std::io::Error> {
    let temp_dir = TempDir::new()?;
    let path = temp_dir.path();

    // 80% Rust files, 20% Markdown files to test both parsing and fallback/markdown chunking
    for i in 0..config.file_count {
        let is_rust = i % 5 != 0;
        let ext = if is_rust { "rs" } else { "md" };
        let file_name = format!("file_{}.{}", i, ext);

        // Subdirectories to test file walker recursion
        let sub_dir = if i % 10 == 0 {
            let dir_name = format!("sub_{}", i / 10);
            let p = path.join(dir_name);
            fs::create_dir_all(&p)?;
            p
        } else {
            path.to_path_buf()
        };

        let file_path = sub_dir.join(file_name);
        let mut file = File::create(&file_path)?;

        let content = if is_rust {
            generate_realistic_rust_file(config.avg_lines, i)
        } else {
            generate_realistic_markdown_file(config.avg_lines, i)
        };

        file.write_all(content.as_bytes())?;
    }

    Ok(temp_dir)
}
