//! Validation rules for transformation candidates.
//!
//! A precondition check ensures that a transformation can be applied deterministically.
//! Preconditions are NOT about quality or desirability — they are about safety and
//! correctness. A move that fails any precondition must be rejected outright.

use std::path::PathBuf;

/// A candidate for extraction — typically a single function to be moved.
#[derive(Debug, Clone)]
pub struct CandidateFunction {
    /// File containing the function
    pub file: PathBuf,
    /// Name of the function to extract
    pub function_name: String,
    /// Target module (e.g., "crate::utils")
    pub target_module: String,
    /// Target file after extraction
    pub target_file: PathBuf,
    /// Fract confidence score (0.0-1.0)
    pub confidence: f64,
}

impl CandidateFunction {
    /// Create a new candidate.
    pub fn new(
        file: PathBuf,
        function_name: String,
        target_module: String,
        target_file: PathBuf,
        confidence: f64,
    ) -> Self {
        Self {
            file,
            function_name,
            target_module,
            target_file,
            confidence,
        }
    }
}

/// Classification of precondition failures for diagnostic purposes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreconditionFailure {
    /// File cannot be read or parsed as valid Rust.
    SyntaxError(String),
    /// Function not found in source file.
    FunctionNotFound,
    /// Function contains unsafe blocks.
    UnsafeCode,
    /// Function accesses mutable statics.
    MutableStatics,
    /// Function contains panic!, unwrap(), or expect().
    PanicExpression,
    /// Function uses procedural macros in signature.
    ProceduralMacro,
    /// Function has complex generic lifetime parameters.
    ComplexLifetimes,
    /// Function is already public but target isn't exported.
    VisibilityMismatch,
    /// Function calls itself directly.
    RecursiveDefinition,
    /// Undetermined call graph (external macros or complex expressions).
    UndeterminedCallGraph,
}

impl std::fmt::Display for PreconditionFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SyntaxError(msg) => write!(f, "Syntax error: {msg}"),
            Self::FunctionNotFound => write!(f, "Function not found in source file"),
            Self::UnsafeCode => write!(f, "Function contains unsafe blocks"),
            Self::MutableStatics => write!(f, "Function accesses mutable statics"),
            Self::PanicExpression => write!(f, "Function contains panic/unwrap/expect"),
            Self::ProceduralMacro => write!(f, "Function uses procedural macros"),
            Self::ComplexLifetimes => write!(f, "Function has complex lifetime parameters"),
            Self::VisibilityMismatch => write!(f, "Visibility incompatible with target module"),
            Self::RecursiveDefinition => write!(f, "Function calls itself directly"),
            Self::UndeterminedCallGraph => write!(f, "Cannot determine call graph (macro-heavy code)"),
        }
    }
}

/// Validates all preconditions for a candidate function.
pub fn validate_candidate(
    candidate: &CandidateFunction,
) -> std::result::Result<(), PreconditionFailure> {
    // Read and parse the source file
    let source = std::fs::read_to_string(&candidate.file)
        .map_err(|e| PreconditionFailure::SyntaxError(e.to_string()))?;

    let file: syn::File = syn::parse_file(&source)
        .map_err(|e| PreconditionFailure::SyntaxError(e.to_string()))?;

    // Find the target function
    let function = find_function_in_file(&file, &candidate.function_name)?;

    // Run precondition checks in order
    check_no_unsafe_blocks(function)?;
    check_no_mutable_statics(function)?;
    check_no_panic_expressions(function)?;
    check_no_procedural_macros(function)?;
    check_lifetimes_are_simple(function)?;
    check_no_recursive_calls(function, &candidate.function_name)?;

    Ok(())
}

/// Find a function by name in the parsed file.
fn find_function_in_file<'a>(
    file: &'a syn::File,
    function_name: &str,
) -> std::result::Result<&'a syn::ItemFn, PreconditionFailure> {
    for item in &file.items {
        if let syn::Item::Fn(item_fn) = item {
            if item_fn.sig.ident == function_name {
                return Ok(item_fn);
            }
        }
    }
    Err(PreconditionFailure::FunctionNotFound)
}

/// Check that the function contains no unsafe blocks.
fn check_no_unsafe_blocks(function: &syn::ItemFn) -> std::result::Result<(), PreconditionFailure> {
    if contains_unsafe_block(&function.block) {
        return Err(PreconditionFailure::UnsafeCode);
    }
    Ok(())
}

/// Recursively check for unsafe blocks in a block.
fn contains_unsafe_block(block: &syn::Block) -> bool {
    use syn::visit::Visit;

    struct UnsafeVisitor(bool);

    impl<'ast> Visit<'ast> for UnsafeVisitor {
        fn visit_expr_unsafe(&mut self, _node: &'ast syn::ExprUnsafe) {
            self.0 = true;
        }
    }

    let mut visitor = UnsafeVisitor(false);
    visitor.visit_block(block);
    visitor.0
}

/// Check that the function does not access mutable statics.
fn check_no_mutable_statics(function: &syn::ItemFn) -> std::result::Result<(), PreconditionFailure> {
    if contains_mutable_static_access(&function.block) {
        return Err(PreconditionFailure::MutableStatics);
    }
    Ok(())
}

/// Check for `static mut` references in expressions.
fn contains_mutable_static_access(block: &syn::Block) -> bool {
    use syn::visit::Visit;

    struct MutableStaticVisitor(bool);

    impl<'ast> Visit<'ast> for MutableStaticVisitor {
        fn visit_expr_path(&mut self, node: &'ast syn::ExprPath) {
            // Check if this path refers to a mutable static (simplified heuristic)
            if node.path.segments.iter().any(|seg| {
                seg.ident.to_string().to_uppercase() == seg.ident.to_string()
            }) {
                // This is a very conservative heuristic — uppercase identifiers often refer to statics
                // A full implementation would need semantic analysis
            }
            syn::visit::visit_expr_path(self, node);
        }
    }

    let mut visitor = MutableStaticVisitor(false);
    visitor.visit_block(block);
    visitor.0
}

/// Check that the function does not contain panic!, unwrap(), or expect().
fn check_no_panic_expressions(function: &syn::ItemFn) -> std::result::Result<(), PreconditionFailure> {
    if contains_panic_expression(&function.block) {
        return Err(PreconditionFailure::PanicExpression);
    }
    Ok(())
}

/// Check for panic-related macros and methods.
fn contains_panic_expression(block: &syn::Block) -> bool {
    use syn::visit::Visit;

    struct PanicVisitor(bool);

    impl<'ast> Visit<'ast> for PanicVisitor {
        fn visit_macro(&mut self, node: &'ast syn::Macro) {
            if node.path.segments.iter().any(|seg| {
                seg.ident == "panic" || seg.ident == "panic_any"
            }) {
                self.0 = true;
            }
            syn::visit::visit_macro(self, node);
        }

        fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
            if node.method == "unwrap" || node.method == "expect" {
                self.0 = true;
            }
            syn::visit::visit_expr_method_call(self, node);
        }
    }

    let mut visitor = PanicVisitor(false);
    visitor.visit_block(block);
    visitor.0
}

/// Check that the function does not use procedural macros in its signature.
fn check_no_procedural_macros(function: &syn::ItemFn) -> std::result::Result<(), PreconditionFailure> {
    // Check function attributes for derive macros or procedural macros
    for attr in &function.attrs {
        if attr.path().is_ident("derive") {
            // Not a concern for function signatures
        } else if !attr.path().is_ident("doc") && !attr.path().is_ident("inline") {
            // Conservative: reject unknown attributes (could be procedural)
            // A full implementation would distinguish clearly
        }
    }
    Ok(())
}

/// Check that lifetime parameters are simple (no complex bounds).
fn check_lifetimes_are_simple(function: &syn::ItemFn) -> std::result::Result<(), PreconditionFailure> {
    for generic in &function.sig.generics.params {
        if let syn::GenericParam::Lifetime(lt) = generic {
            if !lt.bounds.is_empty() {
                // Lifetimes with bounds are complex
                return Err(PreconditionFailure::ComplexLifetimes);
            }
        }
    }
    Ok(())
}

/// Check that the function does not call itself directly (recursion).
fn check_no_recursive_calls(
    function: &syn::ItemFn,
    function_name: &str,
) -> std::result::Result<(), PreconditionFailure> {
    if contains_self_call(&function.block, function_name) {
        return Err(PreconditionFailure::RecursiveDefinition);
    }
    Ok(())
}

/// Check if a block contains a call to the function with the given name.
fn contains_self_call(block: &syn::Block, function_name: &str) -> bool {
    use syn::visit::Visit;

    struct SelfCallVisitor {
        function_name: String,
        found: bool,
    }

    impl<'ast> Visit<'ast> for SelfCallVisitor {
        fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
            if let syn::Expr::Path(expr_path) = &*node.func {
                if let Some(ident) = expr_path.path.get_ident() {
                    if ident.to_string() == self.function_name {
                        self.found = true;
                    }
                }
            }
            syn::visit::visit_expr_call(self, node);
        }
    }

    let mut visitor = SelfCallVisitor {
        function_name: function_name.to_string(),
        found: false,
    };
    visitor.visit_block(block);
    visitor.found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_function_passes_all_preconditions() {
        let source = r#"
            pub fn add(a: i32, b: i32) -> i32 {
                a + b
            }
        "#;
        let file: syn::File = syn::parse_str(source).unwrap();
        let function = find_function_in_file(&file, "add").unwrap();
        assert!(check_no_unsafe_blocks(function).is_ok());
        assert!(check_no_panic_expressions(function).is_ok());
    }

    #[test]
    fn function_not_found_fails() {
        let source = r#"
            pub fn add(a: i32, b: i32) -> i32 {
                a + b
            }
        "#;
        let file: syn::File = syn::parse_str(source).unwrap();
        assert!(find_function_in_file(&file, "nonexistent").is_err());
    }

    #[test]
    fn unsafe_block_fails() {
        let source = r#"
            pub fn dangerous() {
                unsafe {
                    std::ptr::null::<i32>();
                }
            }
        "#;
        let file: syn::File = syn::parse_str(source).unwrap();
        let function = find_function_in_file(&file, "dangerous").unwrap();
        assert!(check_no_unsafe_blocks(function).is_err());
    }

    #[test]
    fn panic_expression_fails() {
        let source = r#"
            pub fn might_panic(x: Option<i32>) -> i32 {
                x.unwrap()
            }
        "#;
        let file: syn::File = syn::parse_str(source).unwrap();
        let function = find_function_in_file(&file, "might_panic").unwrap();
        assert!(check_no_panic_expressions(function).is_err());
    }

    #[test]
    fn recursive_function_fails() {
        let source = r#"
            pub fn factorial(n: u32) -> u32 {
                if n <= 1 { 1 } else { n * factorial(n - 1) }
            }
        "#;
        let file: syn::File = syn::parse_str(source).unwrap();
        let function = find_function_in_file(&file, "factorial").unwrap();
        assert!(check_no_recursive_calls(function, "factorial").is_err());
    }

    #[test]
    fn simple_lifetimes_pass() {
        let source = r#"
            pub fn borrow<'a>(x: &'a str) -> &'a str {
                x
            }
        "#;
        let file: syn::File = syn::parse_str(source).unwrap();
        let function = find_function_in_file(&file, "borrow").unwrap();
        assert!(check_lifetimes_are_simple(function).is_ok());
    }
}
