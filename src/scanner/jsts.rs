//! JavaScript/TypeScript line scanner: counts functions (declarations
//! and arrows), branch keywords, `export` lines, and imports from raw
//! text.

use super::mask::{count_branch_tokens, ident_prefix, is_arrow_function};

pub struct JsTsScanner;

impl JsTsScanner {
    /// Count function declarations and arrow functions.
    #[must_use]
    pub fn count_functions(text: &str) -> usize {
        text.lines()
            .map(|line| {
                let trimmed = line.trim_start();
                let function_form = {
                    let mut rest = trimmed;
                    if let Some(r) = rest.strip_prefix("export ") {
                        rest = r;
                    }
                    if let Some(r) = rest.strip_prefix("async ") {
                        rest = r;
                    }
                    usize::from(rest.starts_with("function "))
                };
                let arrow_form = usize::from(is_arrow_function(trimmed));
                function_form + arrow_form
            })
            .sum()
    }

    /// Count branch keywords (`if`/`else`/`switch`/`case`/loops).
    #[must_use]
    pub fn count_branches(text: &str) -> usize {
        let keywords = ["if", "else", "switch", "case", "for", "while"];
        text.lines()
            .map(|line| count_branch_tokens(line, &keywords))
            .sum()
    }

    /// Count `export` lines as public items.
    #[must_use]
    pub fn count_public_items(text: &str) -> usize {
        text.lines()
            .filter(|line| line.trim_start().starts_with("export "))
            .count()
    }

    /// Count `import` lines.
    #[must_use]
    pub fn count_imports(text: &str) -> usize {
        text.lines()
            .filter(|line| line.trim_start().starts_with("import "))
            .count()
    }

    /// Extract the names of exported items.
    #[must_use]
    pub fn public_symbols(text: &str) -> Vec<String> {
        let mut out = Vec::new();
        for line in text.lines() {
            let trimmed = line.trim_start();
            let Some(rest) = trimmed.strip_prefix("export ") else {
                continue;
            };
            let rest = rest.strip_prefix("default ").unwrap_or(rest);
            let rest = rest.strip_prefix("async ").unwrap_or(rest);
            let mut toks = rest.split_whitespace();
            let Some(kw) = toks.next() else { continue };
            if !JS_ITEM_KEYWORDS.contains(&kw) {
                continue;
            }
            let Some(name_tok) = toks.next() else {
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

const JS_ITEM_KEYWORDS: &[&str] = &[
    "function",
    "class",
    "const",
    "let",
    "var",
    "enum",
    "interface",
    "type",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsts_counts_functions() {
        let text = "function one() {}\nexport function two() {}\nexport async function three() {}\nconst four = () => {}\nconst five = async () => {}";
        assert_eq!(JsTsScanner::count_functions(text), 5);
    }

    #[test]
    fn jsts_counts_branches() {
        let text = "if (a) { } else if (b) { } else { }\nswitch (x) { case 1: break; }\nfor (;;) { while (c && d || e) { } }";
        // if, else, if, else, switch, case, for, while, &&, || = 10
        assert_eq!(JsTsScanner::count_branches(text), 10);
    }

    #[test]
    fn jsts_counts_public_items() {
        let text = "export const foo = 1;\nconst bar = 2;\nexport function baz() {}";
        assert_eq!(JsTsScanner::count_public_items(text), 2);
    }

    #[test]
    fn jsts_counts_imports() {
        let text = "import fs from 'fs';\nimport { x } from './x';\nconst y = 1;";
        assert_eq!(JsTsScanner::count_imports(text), 2);
    }
}
