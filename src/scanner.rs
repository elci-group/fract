pub struct RustScanner;
pub struct PythonScanner;
pub struct JsTsScanner;

impl RustScanner {
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
                    && rest
                        .split_whitespace()
                        .nth(1)
                        .map(|ident| ident.chars().next().is_some_and(|c| c.is_alphanumeric() || c == '_'))
                        .unwrap_or(false)
            })
            .count()
    }

    pub fn count_branches(text: &str) -> usize {
        let keywords = ["if", "else", "match", "while", "for", "loop"];
        text.lines().map(|line| count_branch_tokens(line, &keywords)).sum()
    }

    pub fn count_public_items(text: &str) -> usize {
        text.lines().filter(|line| line.trim_start().starts_with("pub ")).count()
    }

    pub fn count_imports(text: &str) -> usize {
        text.lines().filter(|line| line.trim_start().starts_with("use ")).count()
    }
}

impl PythonScanner {
    pub fn count_functions(text: &str) -> usize {
        text.lines()
            .filter(|line| {
                let trimmed = line.trim_start();
                trimmed.starts_with("def ") && trimmed.len() > 4 && trimmed[4..].starts_with(|c: char| c.is_alphanumeric() || c == '_')
            })
            .count()
    }

    pub fn count_branches(text: &str) -> usize {
        let keywords = ["if", "elif", "else", "for", "while", "and", "or"];
        text.lines().map(|line| count_branch_tokens(line, &keywords)).sum()
    }

    pub fn count_public_items(text: &str) -> usize {
        text.lines().filter(|line| !line.trim_start().starts_with('_')).count()
    }

    pub fn count_imports(text: &str) -> usize {
        text.lines()
            .filter(|line| {
                let trimmed = line.trim_start();
                trimmed.starts_with("import ") || trimmed.starts_with("from ")
            })
            .count()
    }
}

impl JsTsScanner {
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
                    if rest.starts_with("function ") {
                        1
                    } else {
                        0
                    }
                };
                let arrow_form = if is_arrow_function(trimmed) { 1 } else { 0 };
                function_form + arrow_form
            })
            .sum()
    }

    pub fn count_branches(text: &str) -> usize {
        let keywords = ["if", "else", "switch", "case", "for", "while"];
        text.lines().map(|line| count_branch_tokens(line, &keywords)).sum()
    }

    pub fn count_public_items(text: &str) -> usize {
        text.lines().filter(|line| line.trim_start().starts_with("export ")).count()
    }

    pub fn count_imports(text: &str) -> usize {
        text.lines().filter(|line| line.trim_start().starts_with("import ")).count()
    }
}

fn count_branch_tokens(line: &str, keywords: &[&str]) -> usize {
    let mut count = 0;
    let mut in_and = false;
    let mut in_or = false;
    for ch in line.chars() {
        if ch == '&' {
            if in_and {
                count += 1;
                in_and = false;
            } else {
                in_and = true;
            }
            in_or = false;
        } else if ch == '|' {
            if in_or {
                count += 1;
                in_or = false;
            } else {
                in_or = true;
            }
            in_and = false;
        } else {
            in_and = false;
            in_or = false;
        }
    }

    for token in line.split(|c: char| !c.is_alphanumeric() && c != '_') {
        if keywords.contains(&token) {
            count += 1;
        }
    }
    count
}

fn is_arrow_function(line: &str) -> bool {
    // const <ident> = [async] (
    let mut rest = line;
    if !rest.starts_with("const ") {
        return false;
    }
    rest = &rest["const ".len()..];
    let ident_end = rest
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    if ident_end == 0 {
        return false;
    }
    rest = &rest[ident_end..];
    rest = rest.trim_start();
    if !rest.starts_with('=') {
        return false;
    }
    rest = &rest[1..];
    rest = rest.trim_start();
    if let Some(r) = rest.strip_prefix("async ") {
        rest = r;
        rest = rest.trim_start();
    }
    rest.starts_with('(')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_counts_functions() {
        let text = r#"
pub fn one() {}
async fn two() {}
unsafe fn three() {}
pub async unsafe fn four() {}
fn five() {}
// fn not counted
"#;
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
    fn python_counts_functions() {
        let text = "def one():\n    pass\ndef _two():\n    pass\nclass C:\n    def three(self): pass";
        assert_eq!(PythonScanner::count_functions(text), 3);
    }

    #[test]
    fn python_counts_branches() {
        let text = "if a:\n    pass\nelif b:\n    pass\nelse:\n    pass\nfor x in y:\n    while z and w or v:\n        pass";
        // if, elif, else, for, while, and, or = 7
        assert_eq!(PythonScanner::count_branches(text), 7);
    }

    #[test]
    fn python_counts_imports() {
        let text = "import os\nfrom collections import defaultdict\nimport typing";
        assert_eq!(PythonScanner::count_imports(text), 3);
    }

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
