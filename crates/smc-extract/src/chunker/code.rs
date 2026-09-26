use super::Chunk;
use std::path::Path;
use tree_sitter::{Language, Node, Parser};

pub struct CodeChunkerConfig {
    /// Target chunk size in lines for fallback chunking (~50 lines).
    pub fallback_target_lines: usize,
    /// Overlap in lines for fallback chunking (~10 lines).
    pub fallback_overlap_lines: usize,
}

impl Default for CodeChunkerConfig {
    fn default() -> Self {
        Self {
            fallback_target_lines: 50,
            fallback_overlap_lines: 10,
        }
    }
}

pub struct CodeChunker {
    config: CodeChunkerConfig,
}

impl CodeChunker {
    pub fn new(config: CodeChunkerConfig) -> Self {
        Self { config }
    }

    /// Chunk code file using tree-sitter AST if language is supported, otherwise fallback to line windows.
    pub fn chunk_code(
        &self,
        path: &Path,
        content: &str,
        language_guess: Option<&str>,
    ) -> Vec<Chunk> {
        let trimmed = content.trim();
        if trimmed.is_empty() {
            return Vec::new();
        }

        let lang = language_guess.or_else(|| {
            path.extension()
                .and_then(|e| e.to_str())
                .and_then(Self::extension_to_language)
        });

        if let Some(chunks) = lang
            .and_then(Self::get_tree_sitter_language)
            .and_then(|ts_lang| self.chunk_with_tree_sitter(path, content, ts_lang))
            .filter(|c| !c.is_empty())
        {
            return chunks;
        }

        // Fallback to line-based chunker
        self.chunk_lines_fallback(path, content)
    }

    fn extension_to_language(ext: &str) -> Option<&'static str> {
        match ext.to_lowercase().as_str() {
            "rs" => Some("rust"),
            "py" => Some("python"),
            "js" | "jsx" => Some("javascript"),
            "ts" => Some("typescript"),
            "tsx" => Some("tsx"),
            "c" | "h" => Some("c"),
            "cpp" | "hpp" | "cc" | "cxx" => Some("cpp"),
            "java" => Some("java"),
            "cs" => Some("c_sharp"),
            "go" => Some("go"),
            _ => None,
        }
    }

    fn get_tree_sitter_language(name: &str) -> Option<Language> {
        match name {
            "rust" => Some(tree_sitter_rust::LANGUAGE.into()),
            "python" => Some(tree_sitter_python::LANGUAGE.into()),
            "javascript" => Some(tree_sitter_javascript::LANGUAGE.into()),
            "typescript" => Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
            "tsx" => Some(tree_sitter_typescript::LANGUAGE_TSX.into()),
            "c" => Some(tree_sitter_c::LANGUAGE.into()),
            "cpp" => Some(tree_sitter_cpp::LANGUAGE.into()),
            "java" => Some(tree_sitter_java::LANGUAGE.into()),
            "c_sharp" => Some(tree_sitter_c_sharp::LANGUAGE.into()),
            "go" => Some(tree_sitter_go::LANGUAGE.into()),
            _ => None,
        }
    }

    fn chunk_with_tree_sitter(
        &self,
        path: &Path,
        content: &str,
        language: Language,
    ) -> Option<Vec<Chunk>> {
        let mut parser = Parser::new();
        parser.set_language(&language).ok()?;

        let tree = parser.parse(content, None)?;
        let root_node = tree.root_node();

        let mut symbols = Vec::new();
        Self::collect_symbols(root_node, content, &mut symbols);

        if symbols.is_empty() {
            return None;
        }

        let path_display = path.display().to_string();
        let mut chunks = Vec::new();

        for (ordinal, (symbol_name, node_start, node_end)) in symbols.into_iter().enumerate() {
            let code_slice = content.get(node_start..node_end)?.trim();
            if code_slice.is_empty() {
                continue;
            }

            let chunk_text =
                format!("// File: {path_display} | Symbol: {symbol_name}\n{code_slice}");
            chunks.push(Chunk::new(
                ordinal,
                chunk_text,
                None,
                None,
                Some(symbol_name),
                node_start,
                node_end,
            ));
        }

        Some(chunks)
    }

    fn collect_symbols<'a>(
        node: Node<'a>,
        source: &str,
        symbols: &mut Vec<(String, usize, usize)>,
    ) {
        let kind = node.kind();
        let is_symbol = matches!(
            kind,
            // Functions / Methods
            "function_item"
                | "function_definition"
                | "function_declaration"
                | "method_definition"
                | "method_declaration"
                | "constructor_declaration"
                // Classes / Structs / Types / Interfaces
                | "impl_item"
                | "struct_item"
                | "enum_item"
                | "trait_item"
                | "class_definition"
                | "class_declaration"
                | "class_specifier"
                | "struct_specifier"
                | "interface_declaration"
                | "enum_declaration"
                | "type_declaration"
                | "type_alias_declaration"
                | "type_item"
        );

        if is_symbol {
            let name = node
                .child_by_field_name("name")
                .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| kind.to_string());

            let start = node.start_byte();
            let end = node.end_byte();
            symbols.push((name, start, end));
        } else {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                Self::collect_symbols(child, source, symbols);
            }
        }
    }

    fn chunk_lines_fallback(&self, path: &Path, content: &str) -> Vec<Chunk> {
        let path_display = path.display().to_string();
        let lines: Vec<&str> = content.lines().collect();
        if lines.is_empty() {
            return Vec::new();
        }

        let mut chunks = Vec::new();
        let mut ordinal = 0;
        let mut i = 0;

        while i < lines.len() {
            let end_line = (i + self.config.fallback_target_lines).min(lines.len());
            let slice = &lines[i..end_line];
            let code = slice.join("\n");

            let header = format!("// File: {path_display} (lines {}-{})\n", i + 1, end_line);
            let chunk_text = format!("{header}{code}");

            chunks.push(Chunk::new(
                ordinal,
                chunk_text,
                None,
                Some(format!("lines {}-{}", i + 1, end_line)),
                None,
                0,
                0,
            ));
            ordinal += 1;

            if end_line == lines.len() {
                break;
            }

            i += self
                .config
                .fallback_target_lines
                .saturating_sub(self.config.fallback_overlap_lines)
                .max(1);
        }

        chunks
    }
}

impl Default for CodeChunker {
    fn default() -> Self {
        Self::new(CodeChunkerConfig::default())
    }
}
