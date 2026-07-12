//! Output options and terminal styling helpers shared by the renderers.

use std::io::IsTerminal;

use super::model::Severity;

// ---------------------------------------------------------------------------
// Output options
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Human,
    Json,
    Jsonl,
    Sarif,
    Markdown,
}

impl OutputFormat {
    /// Parse an output-format name (case-insensitive, common aliases accepted).
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "human" | "text" | "table" => Some(OutputFormat::Human),
            "json" => Some(OutputFormat::Json),
            "jsonl" | "ndjson" => Some(OutputFormat::Jsonl),
            "sarif" => Some(OutputFormat::Sarif),
            "markdown" | "md" => Some(OutputFormat::Markdown),
            _ => None,
        }
    }

    /// Canonical name of the format.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            OutputFormat::Human => "human",
            OutputFormat::Json => "json",
            OutputFormat::Jsonl => "jsonl",
            OutputFormat::Sarif => "sarif",
            OutputFormat::Markdown => "markdown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorChoice {
    Auto,
    Always,
    Never,
}

impl ColorChoice {
    /// Parse a color-choice name (case-insensitive, common aliases accepted).
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "auto" => Some(ColorChoice::Auto),
            "always" | "on" | "yes" | "true" => Some(ColorChoice::Always),
            "never" | "off" | "no" | "false" => Some(ColorChoice::Never),
            _ => None,
        }
    }

    /// Canonical name of the choice.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ColorChoice::Auto => "auto",
            ColorChoice::Always => "always",
            ColorChoice::Never => "never",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Verbosity {
    Quiet,
    Normal,
    Verbose,
    Debug,
}

impl Verbosity {
    /// Parse a verbosity name (case-insensitive, common aliases accepted).
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "quiet" | "q" => Some(Verbosity::Quiet),
            "normal" | "default" => Some(Verbosity::Normal),
            "verbose" | "v" => Some(Verbosity::Verbose),
            "debug" | "trace" | "vv" => Some(Verbosity::Debug),
            _ => None,
        }
    }

    /// Fold a signed counter (`-v` adds one, `-q` subtracts one) into a level.
    #[must_use]
    pub fn from_count(count: i32) -> Self {
        match count {
            i32::MIN..=-1 => Verbosity::Quiet,
            0 => Verbosity::Normal,
            1 => Verbosity::Verbose,
            _ => Verbosity::Debug,
        }
    }
}

// ---------------------------------------------------------------------------
// Styling
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct Style {
    pub color: bool,
    pub width: usize,
}

impl Style {
    /// Resolve the effective style from a color choice and the environment.
    #[must_use]
    pub fn detect(choice: ColorChoice) -> Self {
        let color = match choice {
            ColorChoice::Always => true,
            ColorChoice::Never => false,
            ColorChoice::Auto => {
                let no_color = std::env::var_os("NO_COLOR").is_some();
                let dumb = matches!(std::env::var("TERM").as_deref(), Ok("dumb"));
                if no_color || dumb {
                    false
                } else {
                    std::io::stdout().is_terminal()
                }
            }
        };
        Style { color, width: 100 }
    }
}

pub(crate) fn paint(text: &str, code: &str, on: bool) -> String {
    if on {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

pub(crate) fn severity_code(s: Severity) -> &'static str {
    match s {
        Severity::Info => "32",
        Severity::Warning => "33",
        Severity::Critical => "31;1",
    }
}

pub(crate) fn display_width(s: &str) -> usize {
    s.chars().count()
}

pub(crate) fn ellipsize(s: &str, width: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= width {
        return s.to_string();
    }
    if width <= 1 {
        return "…".to_string();
    }
    let tail: String = chars.iter().skip(chars.len() - (width - 1)).collect();
    format!("…{tail}")
}

/// Index-time confidence may be unknown; render the raw 0-100 number or an em dash.
pub(crate) fn fmt_confidence_num(c: Option<f64>) -> String {
    match c {
        Some(v) => format!("{:.0}", v * 100.0),
        None => "—".to_string(),
    }
}

/// Same, with a trailing percent for prose contexts.
pub(crate) fn fmt_confidence_pct(c: Option<f64>) -> String {
    match c {
        Some(v) => format!("{:.0}%", v * 100.0),
        None => "—".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_format_parses_aliases_case_insensitively() {
        for (input, expected) in [
            ("human", OutputFormat::Human),
            ("TEXT", OutputFormat::Human),
            ("table", OutputFormat::Human),
            ("json", OutputFormat::Json),
            ("JSON", OutputFormat::Json),
            ("jsonl", OutputFormat::Jsonl),
            ("ndjson", OutputFormat::Jsonl),
            ("sarif", OutputFormat::Sarif),
            ("markdown", OutputFormat::Markdown),
            ("MD", OutputFormat::Markdown),
        ] {
            assert_eq!(OutputFormat::parse(input), Some(expected), "input {input}");
        }
        assert_eq!(OutputFormat::parse("yaml"), None);
    }

    #[test]
    fn output_format_canonical_names_roundtrip() {
        for (format, name) in [
            (OutputFormat::Human, "human"),
            (OutputFormat::Json, "json"),
            (OutputFormat::Jsonl, "jsonl"),
            (OutputFormat::Sarif, "sarif"),
            (OutputFormat::Markdown, "markdown"),
        ] {
            assert_eq!(format.as_str(), name);
            assert_eq!(OutputFormat::parse(name), Some(format));
        }
    }

    #[test]
    fn color_choice_parses_aliases_case_insensitively() {
        for (input, expected) in [
            ("auto", ColorChoice::Auto),
            ("AUTO", ColorChoice::Auto),
            ("always", ColorChoice::Always),
            ("on", ColorChoice::Always),
            ("yes", ColorChoice::Always),
            ("true", ColorChoice::Always),
            ("never", ColorChoice::Never),
            ("off", ColorChoice::Never),
            ("no", ColorChoice::Never),
            ("false", ColorChoice::Never),
        ] {
            assert_eq!(ColorChoice::parse(input), Some(expected), "input {input}");
        }
        assert_eq!(ColorChoice::parse("sometimes"), None);
    }

    #[test]
    fn color_choice_canonical_names() {
        assert_eq!(ColorChoice::Auto.as_str(), "auto");
        assert_eq!(ColorChoice::Always.as_str(), "always");
        assert_eq!(ColorChoice::Never.as_str(), "never");
    }

    #[test]
    fn verbosity_parses_aliases_case_insensitively() {
        for (input, expected) in [
            ("quiet", Verbosity::Quiet),
            ("q", Verbosity::Quiet),
            ("normal", Verbosity::Normal),
            ("default", Verbosity::Normal),
            ("verbose", Verbosity::Verbose),
            ("V", Verbosity::Verbose),
            ("debug", Verbosity::Debug),
            ("trace", Verbosity::Debug),
            ("vv", Verbosity::Debug),
        ] {
            assert_eq!(Verbosity::parse(input), Some(expected), "input {input}");
        }
        assert_eq!(Verbosity::parse("chatty"), None);
    }

    #[test]
    fn verbosity_from_count_folds_signed_counter() {
        assert_eq!(Verbosity::from_count(-3), Verbosity::Quiet);
        assert_eq!(Verbosity::from_count(0), Verbosity::Normal);
        assert_eq!(Verbosity::from_count(1), Verbosity::Verbose);
        assert_eq!(Verbosity::from_count(2), Verbosity::Debug);
        assert_eq!(Verbosity::from_count(i32::MAX), Verbosity::Debug);
    }

    #[test]
    fn verbosity_orders_quiet_to_debug() {
        assert!(Verbosity::Quiet < Verbosity::Normal);
        assert!(Verbosity::Normal < Verbosity::Verbose);
        assert!(Verbosity::Verbose < Verbosity::Debug);
    }

    #[test]
    fn style_detect_honours_explicit_color_choice() {
        assert!(Style::detect(ColorChoice::Always).color);
        let never = Style::detect(ColorChoice::Never);
        assert!(!never.color);
        assert_eq!(never.width, 100);
    }

    #[test]
    fn paint_wraps_only_when_enabled() {
        assert_eq!(paint("x", "31", true), "\x1b[31mx\x1b[0m");
        assert_eq!(paint("x", "31", false), "x");
    }

    #[test]
    fn severity_codes_match_palette() {
        assert_eq!(severity_code(Severity::Info), "32");
        assert_eq!(severity_code(Severity::Warning), "33");
        assert_eq!(severity_code(Severity::Critical), "31;1");
    }

    #[test]
    fn display_width_counts_chars_not_bytes() {
        assert_eq!(display_width("hello"), 5);
        assert_eq!(display_width("héllo"), 5);
        assert_eq!(display_width(""), 0);
    }

    #[test]
    fn ellipsize_leaves_short_strings_alone() {
        assert_eq!(ellipsize("short", 10), "short");
        assert_eq!(ellipsize("exact", 5), "exact");
    }

    #[test]
    fn ellipsize_truncates_with_leading_ellipsis() {
        assert_eq!(ellipsize("abcdefghij", 5), "…ghij");
        assert_eq!(ellipsize("abcdef", 1), "…");
        assert_eq!(ellipsize("abcdef", 0), "…");
    }

    #[test]
    fn fmt_confidence_renders_number_or_dash() {
        assert_eq!(fmt_confidence_num(Some(0.9)), "90");
        assert_eq!(fmt_confidence_num(None), "—");
        assert_eq!(fmt_confidence_pct(Some(0.55)), "55%");
        assert_eq!(fmt_confidence_pct(None), "—");
    }
}
