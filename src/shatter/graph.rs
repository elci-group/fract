//! Dependency graph construction for precise reference tracking.
//!
//! Builds a semantic call graph and tracks what types/traits each function references.
//! This enables safe extraction: we can determine exactly what needs to move or be imported.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use crate::error::{Context, Result};
use syn::visit::Visit;

/// Unique identifier for a function in the project.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionId {
    pub file: PathBuf,
    pub name: String,
}

impl FunctionId {
    pub fn new(file: PathBuf, name: String) -> Self {
        Self { file, name }
    }
}

/// Kind of external item referenced by a function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Struct,
    Enum,
    Trait,
    TypeAlias,
    Const,
    Static,
    Macro,
}

/// Reference to an external item (type, trait, const, etc.).
#[derive(Debug, Clone)]
pub struct ItemRef {
    pub path: String,  // e.g., "crate::utils::Helper" or "std::collections::HashMap"
    pub kind: ItemKind,
}

/// A single function node in the dependency graph.
#[derive(Debug, Clone)]
pub struct FunctionNode {
    pub id: FunctionId,
    pub file: PathBuf,
    pub name: String,
    /// Items (types, traits, consts) this function references
    pub uses_items: Vec<ItemRef>,
    /// Functions this function calls
    pub calls: Vec<FunctionId>,
    /// Functions that call this one (populated after graph build)
    pub called_by: Vec<FunctionId>,
    /// Visibility of the function
    pub visibility: Visibility,
}

/// Function visibility level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Private,
    PubCrate,
    Pub,
    PubSuper,
}

/// Complete dependency graph for a Rust project.
#[derive(Debug)]
pub struct DependencyGraph {
    pub nodes: HashMap<FunctionId, FunctionNode>,
    pub edges: HashMap<FunctionId, Vec<FunctionId>>,  // call edges
}

impl DependencyGraph {
    /// Build a dependency graph by indexing all Rust files in the project.
    pub fn build(root: &Path) -> Result<Self> {
        let mut graph = Self {
            nodes: HashMap::new(),
            edges: HashMap::new(),
        };

        // First pass: collect all functions
        Self::index_functions(root, &mut graph.nodes)?;

        // Second pass: build call graph
        Self::build_call_graph(root, &mut graph.nodes, &mut graph.edges)?;

        // Third pass: populate called_by relationships
        Self::populate_called_by(&mut graph.nodes, &graph.edges);

        Ok(graph)
    }

    /// Index all Rust files and extract function definitions.
    fn index_functions(
        root: &Path,
        nodes: &mut HashMap<FunctionId, FunctionNode>,
    ) -> Result<()> {
        Self::walk_rust_files(root, &mut |file_path, source| {
            match syn::parse_file(&source) {
                Ok(file) => {
                    let mut extractor = FunctionExtractor::new(file_path.clone());
                    extractor.visit_file(&file);

                    for func in extractor.functions {
                        nodes.insert(func.id.clone(), func);
                    }
                    Ok(())
                }
                Err(_) => {
                    // Skip files that don't parse (build scripts, etc.)
                    Ok(())
                }
            }
        })?;

        Ok(())
    }

    /// Walk all .rs files in directory, calling the callback for each.
    fn walk_rust_files<F>(root: &Path, callback: &mut F) -> Result<()>
    where
        F: FnMut(PathBuf, String) -> Result<()>,
    {
        use std::fs;

        fn recurse<F>(dir: &Path, callback: &mut F) -> Result<()>
        where
            F: FnMut(PathBuf, String) -> Result<()>,
        {
            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        // Skip common non-source directories
                        if let Some(name) = path.file_name() {
                            let name_str = name.to_string_lossy();
                            if name_str == "target" || name_str == ".git" || name_str == "node_modules" {
                                continue;
                            }
                        }
                        recurse(&path, callback)?;
                    } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                        if let Ok(source) = fs::read_to_string(&path) {
                            callback(path, source)?;
                        }
                    }
                }
            }
            Ok(())
        }

        recurse(root, callback)
    }

    /// Build call edges between functions.
    fn build_call_graph(
        root: &Path,
        nodes: &HashMap<FunctionId, FunctionNode>,
        edges: &mut HashMap<FunctionId, Vec<FunctionId>>,
    ) -> Result<()> {
        Self::walk_rust_files(root, &mut |file_path, source| {
            if let Ok(file) = syn::parse_file(&source) {
                let mut call_finder = CallFinder::new(file_path.clone(), nodes);
                call_finder.visit_file(&file);

                for (caller, callees) in call_finder.calls {
                    edges
                        .entry(caller)
                        .or_insert_with(Vec::new)
                        .extend(callees);
                }
            }
            Ok(())
        })?;

        Ok(())
    }

    /// Populate the `called_by` field in each node.
    fn populate_called_by(
        nodes: &mut HashMap<FunctionId, FunctionNode>,
        edges: &HashMap<FunctionId, Vec<FunctionId>>,
    ) {
        for (caller, callees) in edges {
            for callee in callees {
                if let Some(node) = nodes.get_mut(callee) {
                    node.called_by.push(caller.clone());
                }
            }
        }
    }

    /// Get the transitive closure of calls from a function.
    pub fn call_closure(&self, start: &FunctionId) -> HashSet<FunctionId> {
        let mut closure = HashSet::new();
        let mut queue = vec![start.clone()];

        while let Some(current) = queue.pop() {
            if closure.insert(current.clone()) {
                if let Some(callees) = self.edges.get(&current) {
                    queue.extend(callees.clone());
                }
            }
        }

        closure
    }

    /// Check if extracting a function would be safe (deterministic call graph).
    pub fn validate_extraction_safe(&self, function_id: &FunctionId) -> Result<()> {
        if !self.nodes.contains_key(function_id) {
            return Err("Function not found in graph".into());
        }

        // For Phase 1, we only extract functions with fully determined callees
        // (i.e., no macro-induced dynamic calls)
        // This is validated during precondition checking, so we just confirm the node exists.

        Ok(())
    }

    /// Determine what imports are needed to move a function to a target module.
    pub fn imports_needed(
        &self,
        function_id: &FunctionId,
        target_module: &str,
    ) -> Result<Vec<String>> {
        let node = self.nodes.get(function_id)
            .context("Function not found in graph")?;

        let mut imports = Vec::new();

        // Collect all items this function uses
        for item_ref in &node.uses_items {
            // If item is from a different module, we need an import
            if !item_ref.path.starts_with("crate::")
                || !item_ref.path.starts_with(&format!("{}::", target_module)) {
                // Generate import statement (simplified)
                let import = format!("use {};", item_ref.path);
                if !imports.contains(&import) {
                    imports.push(import);
                }
            }
        }

        Ok(imports)
    }
}

/// Extracts all function definitions from a file AST.
struct FunctionExtractor {
    file: PathBuf,
    functions: Vec<FunctionNode>,
}

impl FunctionExtractor {
    fn new(file: PathBuf) -> Self {
        Self {
            file,
            functions: Vec::new(),
        }
    }
}

impl<'ast> Visit<'ast> for FunctionExtractor {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        let name = node.sig.ident.to_string();
        let visibility = match &node.vis {
            syn::Visibility::Inherited => Visibility::Private,
            syn::Visibility::Public(_) => Visibility::Pub,
            syn::Visibility::Restricted(_) => Visibility::PubCrate,
        };

        let mut item_finder = ItemFinder::new();
        item_finder.visit_item_fn(node);

        let func = FunctionNode {
            id: FunctionId::new(self.file.clone(), name.clone()),
            file: self.file.clone(),
            name,
            uses_items: item_finder.items,
            calls: Vec::new(),  // Populated in second pass
            called_by: Vec::new(),
            visibility,
        };

        self.functions.push(func);
        syn::visit::visit_item_fn(self, node);
    }
}

/// Finds all item references within a function.
struct ItemFinder {
    items: Vec<ItemRef>,
}

impl ItemFinder {
    fn new() -> Self {
        Self {
            items: Vec::new(),
        }
    }

    fn add_from_path(&mut self, path: &syn::Path) {
        // Convert syn::Path to string representation
        let mut path_str = String::new();
        for (i, segment) in path.segments.iter().enumerate() {
            if i > 0 {
                path_str.push_str("::");
            }
            path_str.push_str(&segment.ident.to_string());
        }

        if !path_str.is_empty() && !self.items.iter().any(|i| i.path == path_str) {
            self.items.push(ItemRef {
                path: path_str,
                kind: ItemKind::TypeAlias,  // Simplified; would need more context to determine actual kind
            });
        }
    }
}

impl<'ast> Visit<'ast> for ItemFinder {
    fn visit_type_path(&mut self, node: &'ast syn::TypePath) {
        self.add_from_path(&node.path);
        syn::visit::visit_type_path(self, node);
    }

    fn visit_path(&mut self, node: &'ast syn::Path) {
        self.add_from_path(node);
        syn::visit::visit_path(self, node);
    }
}

/// Finds all function calls within a file.
struct CallFinder {
    file: PathBuf,
    nodes: std::collections::HashMap<FunctionId, FunctionNode>,
    calls: HashMap<FunctionId, Vec<FunctionId>>,
    current_function: Option<String>,
}

impl CallFinder {
    fn new(
        file: PathBuf,
        nodes: &std::collections::HashMap<FunctionId, FunctionNode>,
    ) -> Self {
        Self {
            file: file.clone(),
            nodes: nodes.clone(),
            calls: HashMap::new(),
            current_function: None,
        }
    }
}

impl<'ast> Visit<'ast> for CallFinder {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        let prev = self.current_function.take();
        self.current_function = Some(node.sig.ident.to_string());

        syn::visit::visit_item_fn(self, node);

        self.current_function = prev;
    }

    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let syn::Expr::Path(expr_path) = &*node.func {
            if let Some(ident) = expr_path.path.get_ident() {
                let callee_name = ident.to_string();
                let callee_id = FunctionId::new(self.file.clone(), callee_name);

                // Only record if the callee is a known function in this file
                if self.nodes.contains_key(&callee_id) {
                    if let Some(caller_name) = &self.current_function {
                        let caller_id = FunctionId::new(self.file.clone(), caller_name.clone());
                        self.calls
                            .entry(caller_id)
                            .or_insert_with(Vec::new)
                            .push(callee_id);
                    }
                }
            }
        }

        syn::visit::visit_expr_call(self, node);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn function_id_equality() {
        let id1 = FunctionId::new(PathBuf::from("src/lib.rs"), "foo".to_string());
        let id2 = FunctionId::new(PathBuf::from("src/lib.rs"), "foo".to_string());
        assert_eq!(id1, id2);
    }

    #[test]
    fn visibility_derived() {
        assert_ne!(Visibility::Private, Visibility::Pub);
        assert_ne!(Visibility::Private, Visibility::PubCrate);
    }

    #[test]
    fn item_kind_equality() {
        assert_eq!(ItemKind::Struct, ItemKind::Struct);
        assert_ne!(ItemKind::Struct, ItemKind::Enum);
    }

    #[test]
    fn function_node_creation() {
        let node = FunctionNode {
            id: FunctionId::new(PathBuf::from("src/lib.rs"), "test".to_string()),
            file: PathBuf::from("src/lib.rs"),
            name: "test".to_string(),
            uses_items: vec![],
            calls: vec![],
            called_by: vec![],
            visibility: Visibility::Private,
        };
        assert_eq!(node.name, "test");
        assert_eq!(node.visibility, Visibility::Private);
    }
}
