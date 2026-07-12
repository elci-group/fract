use super::jsts::JsTsScanner;
use super::python::PythonScanner;
use super::rust::RustScanner;

/// List the public symbols of `text` for the given language, sorted and deduped.
#[must_use]
pub fn public_symbols(text: &str, lang: crate::Language) -> Vec<String> {
    let mut v = match lang {
        crate::Language::Rust => RustScanner::public_symbols(text),
        crate::Language::Python => PythonScanner::public_symbols(text),
        crate::Language::TypeScript | crate::Language::JavaScript => {
            JsTsScanner::public_symbols(text)
        }
        crate::Language::Other => Vec::new(),
    };
    v.sort();
    v.dedup();
    v
}

pub(crate) fn count_branch_tokens(line: &str, keywords: &[&str]) -> usize {
    // Mask string/char literals and trailing `//` comments so keywords inside
    // them (e.g. `let s = "if for while";`) are not counted as branches.
    let clean = mask_code(line);
    let mut count = 0;
    let mut in_and = false;
    let mut in_or = false;
    for ch in clean.chars() {
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

    for token in clean.split(|c: char| !c.is_alphanumeric() && c != '_') {
        if keywords.contains(&token) {
            count += 1;
        }
    }
    count
}

/// Replace the contents of string/char literals with spaces and drop everything
/// after a `//` line comment, preserving token boundaries for the caller.
pub(crate) fn mask_code(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    let mut in_str = false;
    let mut in_chr = false;
    let mut escape = false;
    while let Some(c) = chars.next() {
        if in_str {
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_str = false;
            }
            out.push(' ');
            continue;
        }
        if in_chr {
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '\'' {
                in_chr = false;
            }
            out.push(' ');
            continue;
        }
        if c == '"' {
            in_str = true;
            out.push(' ');
            continue;
        }
        if c == '\'' {
            in_chr = true;
            out.push(' ');
            continue;
        }
        if c == '/' && matches!(chars.peek(), Some('/')) {
            break; // rest of the line is a comment
        }
        out.push(c);
    }
    out
}

pub(crate) fn is_arrow_function(line: &str) -> bool {
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

/// Leading run of identifier characters (alphanumeric / `_`).
pub(crate) fn ident_prefix(s: &str) -> String {
    s.chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_code_blanks_literals() {
        let masked = mask_code(r#"let s = "if for"; // if"#);
        assert!(masked.starts_with("let s = "));
        assert!(!masked.contains("if"));
        assert!(!masked.contains("for"));
    }

    #[test]
    fn property_mask_is_idempotent() {
        let mut state: u64 = 0x0dd_f00d;
        let alphabet = ['i', 'f', ' ', '"', '/', 'e', '\\', '\n'];
        for _ in 0..300 {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            let len = (state % 24) as usize;
            let mut s = String::new();
            for _ in 0..len {
                state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                s.push(alphabet[usize::try_from(state).unwrap() % alphabet.len()]);
            }
            let once = mask_code(&s);
            let twice = mask_code(&once);
            assert_eq!(once, twice, "mask not idempotent for {s:?}");
        }
    }
}
