//! Line-based source scanners per language (Rust, Python, JS/TS) plus
//! string/comment masking. Deliberately parser-free: metrics are cheap
//! approximations, not AST-exact.

mod jsts;
mod mask;
mod python;
mod rust;

pub use jsts::JsTsScanner;
pub use mask::public_symbols;
pub use python::PythonScanner;
pub use rust::RustScanner;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_public_symbols_lists_removed_pub_fn() {
        let before = "pub fn keep() {}\npub fn gone() {}\nfn private() {}\n";
        let syms = public_symbols(before, crate::Language::Rust);
        assert!(syms.contains(&"keep".to_string()));
        assert!(syms.contains(&"gone".to_string()));
        assert!(!syms.contains(&"private".to_string()));
    }

    #[test]
    fn rust_public_symbols_ignores_strings_and_comments() {
        let text = r#"
let s = "pub fn fake1()";
// pub fn fake2() {}
pub fn real() {}
"#;
        let syms = public_symbols(text, crate::Language::Rust);
        assert_eq!(syms, vec!["real".to_string()]);
    }

    #[test]
    fn rust_public_symbols_captures_struct_enum_trait() {
        let text = "pub struct Foo;\npub enum Bar { A }\npub trait Baz {}\npub type Alias = u32;\npub(crate) fn inner() {}\n";
        let syms = public_symbols(text, crate::Language::Rust);
        for n in ["Foo", "Bar", "Baz", "Alias", "inner"] {
            assert!(syms.contains(&n.to_string()), "missing {n}: {syms:?}");
        }
    }

    #[test]
    fn python_public_symbols_top_level_only_skipping_underscore() {
        let text = "def foo():\n    pass\nclass Bar:\n    def method(self):\n        pass\ndef _hidden():\n    pass\n";
        let syms = public_symbols(text, crate::Language::Python);
        assert_eq!(syms, vec!["Bar".to_string(), "foo".to_string()]);
    }

    #[test]
    fn jsts_public_symbols_exports_across_forms() {
        let text = "export function a() {}\nexport default function b() {}\nexport async function c() {}\nexport const D = 1;\nexport interface E {}\nexport type F = string;\nconst g = 2;\n";
        let syms = public_symbols(text, crate::Language::TypeScript);
        for n in ["a", "b", "c", "D", "E", "F"] {
            assert!(syms.contains(&n.to_string()), "missing {n}: {syms:?}");
        }
        assert!(!syms.contains(&"g".to_string()));
    }
}
