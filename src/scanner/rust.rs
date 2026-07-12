//! Rust line scanner: counts `fn` items (skipping `pub`/`async`/`unsafe`
//! prefixes), branch keywords, public items, and imports from raw text.

use super::mask::{count_branch_tokens, ident_prefix, mask_code};

pub struct RustScanner;

impl RustScanner {
    /// Count `fn` items (ignoring `pub`/`async`/`unsafe` prefixes).
    #[must_use]
    pub fn count_functions(text: &str) -> usize {
        text.lines()
            .filter(|line| {
                let trimmed = line.trim_start();
                let mut rest = trimmed;
                loop {
                    if let Some(r) = rest.strip_prefix("pub ") {
                        rest = r;
                    } else if let Some(r) = rest.strip_prefix("async ") {
                        rest = r;
                    } else if let Some(r) = rest.strip_prefix("unsafe ") {
                        rest = r;
                    } else {
                        break;
                    }
                }
                rest.starts_with("fn ")
                    && rest.split_whitespace().nth(1).is_some_and(|ident| {
                        ident
                            .chars()
                            .next()
                            .is_some_and(|c| c.is_alphanumeric() || c == '_')
                    })
            })
            .count()
    }

    /// Count branch keywords (`if`/`else`/`match`/loops).
    #[must_use]
    pub fn count_branches(text: &str) -> usize {
        let keywords = ["if", "else", "match", "while", "for", "loop"];
        text.lines()
            .map(|line| count_branch_tokens(line, &keywords))
            .sum()
    }

    /// Count `pub` lines as public items.
    #[must_use]
    pub fn count_public_items(text: &str) -> usize {
        text.lines()
            .filter(|line| line.trim_start().starts_with("pub "))
            .count()
    }

    /// Count `use` lines.
    #[must_use]
    pub fn count_imports(text: &str) -> usize {
        text.lines()
            .filter(|line| line.trim_start().starts_with("use "))
            .count()
    }

    /// Extract the names of public items.
    #[must_use]
    pub fn public_symbols(text: &str) -> Vec<String> {
        let mut out = Vec::new();
        for line in text.lines() {
            let clean = mask_code(line);
            let Some(rest) = strip_rust_visibility(clean.trim_start()) else {
                continue;
            };
            let toks: Vec<&str> = rest.split_whitespace().collect();
            let mut i = 0;
            while i < toks.len() {
                match toks[i] {
                    "async" | "unsafe" => i += 1,
                    "const" if toks.get(i + 1) == Some(&"fn") => i += 1,
                    _ => break,
                }
            }
            let Some(kw) = toks.get(i) else { continue };
            if !RUST_ITEM_KEYWORDS.contains(kw) {
                continue;
            }
            let Some(name_tok) = toks.get(i + 1) else {
                continue;
            };
            let ident = ident_prefix(name_tok);
            if !ident.is_empty() {
                out.push(ident);
            }
        }
        out
    }
}

const RUST_ITEM_KEYWORDS: &[&str] = &[
    "fn", "struct", "enum", "trait", "type", "const", "static", "mod", "union",
];

/// Strip a leading Rust visibility token (`pub` or `pub(<...>)`), returning the
/// remainder trimmed of leading whitespace. Returns `None` when the line does
/// not start with a visibility modifier.
fn strip_rust_visibility(s: &str) -> Option<&str> {
    let after = s.strip_prefix("pub")?;
    if let Some(paren) = after.strip_prefix('(') {
        let close = paren.find(')')?;
        Some(paren[close + 1..].trim_start())
    } else if after.starts_with(|c: char| c.is_whitespace()) {
        Some(after.trim_start())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_counts_functions() {
        let text = r"
pub fn one() {}
async fn two() {}
unsafe fn three() {}
pub async unsafe fn four() {}
fn five() {}
// fn not counted
";
        assert_eq!(RustScanner::count_functions(text), 5);
    }

    #[test]
    fn rust_counts_branches() {
        let text = "if a { } else if b { } else { }\nmatch x { }\nwhile c && d || e { }\nfor i in 0..10 { loop {} }";
        // if, else, if, else, match, while, &&, ||, for, loop = 10
        assert_eq!(RustScanner::count_branches(text), 10);
    }

    #[test]
    fn rust_counts_public_items() {
        let text = "pub struct Foo;\npub fn bar() {}\nfn private() {}";
        assert_eq!(RustScanner::count_public_items(text), 2);
    }

    #[test]
    fn rust_counts_imports() {
        let text = "use std::fs;\nuse crate::foo;\nfn x() {}";
        assert_eq!(RustScanner::count_imports(text), 2);
    }

    #[test]
    fn rust_ignores_keywords_in_strings_and_comments() {
        let text = r#"
let s = "if for while match";
let t = "escaped \"if\" still masked";
// if else match while for loop
if real && other || done { }
"#;
        // Only the real `if`, `&&`, `||` on the last line count: if + && + || = 3
        assert_eq!(RustScanner::count_branches(text), 3);
    }

    #[test]
    fn handles_crlf_and_tabs() {
        let text = "if a {\r\n\tif b {\r\n\t}\r\n}\r\n";
        assert_eq!(RustScanner::count_branches(text), 2);
    }

    #[test]
    fn property_string_keywords_never_counted() {
        // For randomly generated lines, keywords inside a quoted string must not
        // increase the branch count beyond the real `if` keywords outside it.
        let mut state: u64 = 0xfeed_face_cafe_babe;
        for _ in 0..400 {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let real = (state % 5) as usize;
            let hidden = ((state >> 8) % 5) as usize;
            let mut line = String::new();
            for _ in 0..real {
                line.push_str("if x ");
            }
            line.push('"');
            for _ in 0..hidden {
                line.push_str("for y ");
            }
            line.push('"');
            assert_eq!(RustScanner::count_branches(&line), real, "line={line:?}");
        }
    }
}
