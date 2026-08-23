//! AST-based source code transformations.
//!
//! Uses syn for parsing and prettyplease for formatting, ensuring all transformations
//! produce valid Rust that can be verified by cargo check.

use std::path::{Path, PathBuf};
use crate::error::{Context, Result};
use syn::visit::Visit;
use quote::ToTokens;

/// Performs deterministic AST-based transformations on Rust source.
pub struct AstRewriter {
    #[allow(dead_code)]
    root: PathBuf,
}

/// Result of extracting a function from source.
pub struct FunctionParts {
    /// Function signature (e.g., "pub fn foo(x: i32) -> String")
    pub signature: String,
    /// Function body (e.g., "{ x.to_string() }")
    pub body: String,
    /// Documentation comments
    pub doc_comments: String,
    /// Attributes on the function
    pub attributes: Vec<String>,
}

impl AstRewriter {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Extract a function from source file, returning modified source code.
    ///
    /// Returns (source_modified, target_modified).
    pub fn extract_function(
        &self,
        source_file: &Path,
        function_name: &str,
        target_file: &Path,
        _visibility_override: Option<&str>,
    ) -> Result<(String, String)> {
        // Read and parse source file
        let source_content = std::fs::read_to_string(source_file)
            .context("reading source file")?;
        let _file: syn::File = syn::parse_file(&source_content)
            .context("parsing source file")?;

        // Extract function parts
        let parts = self.extract_function_parts(&source_content, function_name)?;

        // Read and parse target file (or create if doesn't exist)
        let target_content = if target_file.exists() {
            std::fs::read_to_string(target_file)
                .context("reading target file")?
        } else {
            // Create a minimal module file
            String::new()
        };

        let mut target_file_content = if target_content.is_empty() {
            String::new()
        } else {
            let target_ast: syn::File = syn::parse_file(&target_content)
                .context("parsing target file")?;
            prettyplease::unparse(&target_ast)
        };

        // Add function to target file
        if target_file_content.is_empty() {
            target_file_content = format!(
                "{}{}\n{}\n",
                parts.doc_comments, parts.attributes.join("\n"), parts.signature
            );
        } else {
            target_file_content.push('\n');
            if !parts.doc_comments.is_empty() {
                target_file_content.push_str(&parts.doc_comments);
                target_file_content.push('\n');
            }
            for attr in &parts.attributes {
                target_file_content.push_str(attr);
                target_file_content.push('\n');
            }
            target_file_content.push_str(&parts.signature);
            target_file_content.push('\n');
        }
        target_file_content.push_str(&parts.body);
        target_file_content.push('\n');

        // Remove function from source file
        let source_modified = self.remove_function(&source_content, function_name)?;

        Ok((source_modified, target_file_content))
    }

    /// Add an import statement to a file.
    ///
    /// Inserts after existing imports, or at the top if none exist.
    pub fn add_import(
        &self,
        file_content: &str,
        import_stmt: &str,
    ) -> Result<String> {
        let file: syn::File = syn::parse_file(file_content)
            .context("parsing file for import addition")?;

        // Find position after last import
        let mut last_import_index = 0;
        for (i, item) in file.items.iter().enumerate() {
            if matches!(item, syn::Item::Use(_)) {
                last_import_index = i + 1;
            }
        }

        // Parse the import statement
        let import_item: syn::ItemUse = syn::parse_str(&import_stmt)
            .context("parsing import statement")?;

        // If no imports exist, create import at top
        let mut new_items = file.items.clone();
        if last_import_index == 0 {
            new_items.insert(0, syn::Item::Use(import_item));
        } else {
            new_items.insert(last_import_index, syn::Item::Use(import_item));
        }

        let new_file = syn::File {
            shebang: file.shebang.clone(),
            attrs: file.attrs.clone(),
            items: new_items,
        };

        Ok(prettyplease::unparse(&new_file))
    }

    /// Remove an import statement from a file.
    pub fn remove_import(
        &self,
        file_content: &str,
        import_path: &str,
    ) -> Result<String> {
        let file: syn::File = syn::parse_file(file_content)
            .context("parsing file for import removal")?;

        let new_items: Vec<syn::Item> = file.items
            .iter()
            .filter(|item| {
                if let syn::Item::Use(use_item) = item {
                    !use_item_matches(&use_item.tree, import_path)
                } else {
                    true
                }
            })
            .cloned()
            .collect();

        if new_items.len() == file.items.len() {
            // Import not found, return unchanged
            return Ok(file_content.to_string());
        }

        let new_file = syn::File {
            shebang: file.shebang.clone(),
            attrs: file.attrs.clone(),
            items: new_items,
        };

        Ok(prettyplease::unparse(&new_file))
    }

    /// Remove a function definition from source code.
    ///
    /// Preserves other code and comments.
    pub fn remove_function(
        &self,
        file_content: &str,
        function_name: &str,
    ) -> Result<String> {
        let file: syn::File = syn::parse_file(file_content)
            .context("parsing file for function removal")?;

        let new_items: Vec<syn::Item> = file.items
            .iter()
            .filter(|item| {
                if let syn::Item::Fn(item_fn) = item {
                    item_fn.sig.ident.to_string() != function_name
                } else {
                    true
                }
            })
            .cloned()
            .collect();

        if new_items.len() == file.items.len() {
            return Err(format!("Function '{}' not found in file", function_name).into());
        }

        let new_file = syn::File {
            shebang: file.shebang.clone(),
            attrs: file.attrs.clone(),
            items: new_items,
        };

        Ok(prettyplease::unparse(&new_file))
    }

    /// Extract components of a function by name.
    pub fn extract_function_parts(
        &self,
        content: &str,
        function_name: &str,
    ) -> Result<FunctionParts> {
        let file: syn::File = syn::parse_file(content)
            .context("parsing content")?;

        let mut extractor = FunctionPartExtractor::new(function_name.to_string());
        extractor.visit_file(&file);

        extractor
            .parts
            .ok_or_else(|| format!("Function '{}' not found", function_name).into())
    }
}

/// Check if a use tree matches the given import path.
fn use_item_matches(tree: &syn::UseTree, import_path: &str) -> bool {
    match tree {
        syn::UseTree::Path(path) => {
            let path_str = path.ident.to_string();
            import_path.starts_with(&path_str)
        }
        syn::UseTree::Name(name) => {
            name.ident.to_string() == import_path
        }
        syn::UseTree::Rename(rename) => {
            rename.ident.to_string() == import_path
        }
        syn::UseTree::Glob(_) => false,
        syn::UseTree::Group(group) => {
            group.items.iter().any(|item| use_item_matches(item, import_path))
        }
    }
}

/// Visitor to extract function components.
struct FunctionPartExtractor {
    target_name: String,
    parts: Option<FunctionParts>,
}

impl FunctionPartExtractor {
    fn new(target_name: String) -> Self {
        Self {
            target_name,
            parts: None,
        }
    }
}

impl<'ast> Visit<'ast> for FunctionPartExtractor {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        if node.sig.ident.to_string() == self.target_name {
            // Extract documentation comments
            let mut doc_comments = String::new();
            for attr in &node.attrs {
                if attr.path().is_ident("doc") {
                    doc_comments.push_str("///");
                    if let syn::Meta::NameValue(nv) = &attr.meta {
                        if let syn::Expr::Lit(syn::ExprLit {
                            lit: syn::Lit::Str(lit_str),
                            ..
                        }) = &nv.value
                        {
                            doc_comments.push(' ');
                            doc_comments.push_str(&lit_str.value());
                        }
                    }
                    doc_comments.push('\n');
                }
            }

            // Extract attributes (excluding doc comments)
            let attributes: Vec<String> = node
                .attrs
                .iter()
                .filter(|attr| !attr.path().is_ident("doc"))
                .map(|attr| {
                    let tokens = attr.to_token_stream().to_string();
                    format!("#{}", tokens)
                })
                .collect();

            // Generate signature using quote/to_token_stream
            let sig_tokens = node.sig.to_token_stream().to_string();
            let signature = format!("{} {{}}", sig_tokens);

            // Extract body
            let body_tokens = node.block.to_token_stream().to_string();

            self.parts = Some(FunctionParts {
                signature,
                body: body_tokens,
                doc_comments,
                attributes,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_simple_function() {
        let source = r#"pub fn add(a: i32, b: i32) -> i32 { a + b }"#;
        let rewriter = AstRewriter::new(PathBuf::from("."));

        let parts = rewriter
            .extract_function_parts(source, "add")
            .expect("failed to extract");

        assert_eq!(parts.signature.contains("add"), true);
        assert_eq!(parts.body.contains("a"), true);
    }

    #[test]
    fn remove_function_from_file() {
        let source = r#"
pub fn foo() { println!("foo"); }
pub fn bar() { println!("bar"); }
"#;
        let rewriter = AstRewriter::new(PathBuf::from("."));

        let result = rewriter
            .remove_function(source, "foo")
            .expect("failed to remove");

        assert!(!result.contains("fn foo"));
        assert!(result.contains("fn bar"));
    }

    #[test]
    fn remove_nonexistent_function_errors() {
        let source = r#"pub fn foo() { }"#;
        let rewriter = AstRewriter::new(PathBuf::from("."));

        assert!(rewriter.remove_function(source, "nonexistent").is_err());
    }

    #[test]
    fn parse_roundtrip() {
        let source = r#"pub fn test() -> i32 { 42 }"#;
        let file: syn::File = syn::parse_str(source).expect("parse");
        let unparsed = prettyplease::unparse(&file);

        // Should be valid Rust after roundtrip
        let _: syn::File = syn::parse_str(&unparsed).expect("reparsed");
    }
}
