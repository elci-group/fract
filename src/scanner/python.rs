use super::mask::{count_branch_tokens, ident_prefix};

pub struct PythonScanner;

impl PythonScanner {
    pub fn count_functions(text: &str) -> usize {
        text.lines()
            .filter(|line| {
                let trimmed = line.trim_start();
                trimmed.starts_with("def ")
                    && trimmed.len() > 4
                    && trimmed[4..].starts_with(|c: char| c.is_alphanumeric() || c == '_')
            })
            .count()
    }

    pub fn count_branches(text: &str) -> usize {
        let keywords = ["if", "elif", "else", "for", "while", "and", "or"];
        text.lines()
            .map(|line| count_branch_tokens(line, &keywords))
            .sum()
    }

    pub fn count_public_items(text: &str) -> usize {
        text.lines()
            .filter(|line| !line.trim_start().starts_with('_'))
            .count()
    }

    pub fn count_imports(text: &str) -> usize {
        text.lines()
            .filter(|line| {
                let trimmed = line.trim_start();
                trimmed.starts_with("import ") || trimmed.starts_with("from ")
            })
            .count()
    }

    pub fn public_symbols(text: &str) -> Vec<String> {
        let mut out = Vec::new();
        for line in text.lines() {
            if line.chars().next().is_some_and(|c| c.is_whitespace()) {
                continue;
            }
            let name = if let Some(rest) = line.strip_prefix("def ") {
                ident_prefix(rest)
            } else if let Some(rest) = line.strip_prefix("class ") {
                ident_prefix(rest)
            } else {
                continue;
            };
            if name.is_empty() || name.starts_with('_') {
                continue;
            }
            out.push(name);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_counts_functions() {
        let text =
            "def one():\n    pass\ndef _two():\n    pass\nclass C:\n    def three(self): pass";
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
}
