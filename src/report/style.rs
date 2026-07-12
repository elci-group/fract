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
