//! Output options and terminal styling helpers shared by the renderers.

use std::fmt::Write;
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
        match choice {
            ColorChoice::Always => Style {
                color: true,
                width: 100,
            },
            ColorChoice::Never => Style {
                color: false,
                width: 100,
            },
            ColorChoice::Auto => {
                let no_color = std::env::var_os("NO_COLOR").is_some();
                let dumb = matches!(std::env::var("TERM").as_deref(), Ok("dumb"));
                Self::detect_auto(no_color, dumb, std::io::stdout().is_terminal())
            }
        }
    }

    /// Pure core of `detect` for the `Auto` case, with the environment passed
    /// in explicitly so tests don't need to mutate process-global env state.
    #[must_use]
    pub(crate) fn detect_auto(no_color: bool, dumb_term: bool, is_terminal: bool) -> Self {
        Style {
            color: !no_color && !dumb_term && is_terminal,
            width: 100,
        }
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
        Severity::Info => "38;5;86",        // Bright cyan
        Severity::Warning => "38;5;208",    // Orange
        Severity::Critical => "38;5;196;1", // Bright red + bold
    }
}

// Extended color palette for glass aesthetic
pub const GLASS_BASE: &str = "38;5;153"; // Light blue glass
pub const GLASS_SHARD: &str = "38;5;117"; // Medium blue shard
pub const GLASS_HIGHLIGHT: &str = "38;5;159"; // Bright highlight
pub const GLASS_REFLECT: &str = "38;5;189"; // Reflection
pub const GLASS_SHADOW: &str = "38;5;66"; // Shadow
pub const HEADER_COLOR: &str = "38;5;147;1"; // Purple header
pub const ACCENT_COLOR: &str = "38;5;141"; // Magenta accent
pub const SUCCESS_COLOR: &str = "38;5;76"; // Green success
pub const SUBTLE_COLOR: &str = "38;5;245"; // Gray subtle

/// Glass-specific paint function with extended palette
#[must_use]
pub fn glass_paint(text: &str, color: &str, on: bool) -> String {
    paint(text, color, on)
}

/// Paint text with a gradient effect (simulated glass refraction)
#[must_use]
pub fn gradient_paint(text: &str, on: bool) -> String {
    if !on {
        return text.to_string();
    }

    let chars: Vec<char> = text.chars().collect();
    let mut result = String::new();
    let colors = [
        GLASS_BASE,
        GLASS_HIGHLIGHT,
        GLASS_REFLECT,
        GLASS_HIGHLIGHT,
        GLASS_BASE,
    ];

    for (i, ch) in chars.iter().enumerate() {
        let color_idx = i % colors.len();
        result.push_str(&paint(&ch.to_string(), colors[color_idx], true));
    }

    result
}

/// Shattering glass animation frames
/// Shattering glass animation frames. Each frame is a complete multi-line
/// screen render; the CLI clears the terminal and prints one frame at a time.
pub struct GlassAnimation {
    frames: Vec<String>,
    current: usize,
}

impl GlassAnimation {
    /// Build a glass-form → crack → shatter animation for `text`.
    /// When `enabled` is `false` a single static frame is produced.
    #[must_use]
    pub fn new(text: &str, enabled: bool) -> Self {
        if !enabled {
            return Self {
                frames: vec![text.to_string()],
                current: 0,
            };
        }

        let w = text.chars().count().max(2);
        let shards = ["◇", "◈", "◆", "⋄", "✧", "✦", "⋆", "★"];
        let glass_chars = ["░", "▒", "▓", "█"];
        let mut frames = Vec::new();

        // Helper: build a boxed frame with a border style and optional cracks.
        let boxed = |border: &str, content: &str, accent: bool| -> String {
            let b = if accent {
                paint(border, GLASS_HIGHLIGHT, true)
            } else {
                paint(border, GLASS_BASE, true)
            };
            let top = format!("{b}{}{b}", paint(&"─".repeat(w), GLASS_BASE, true));
            let mid = format!(
                "{b}{}{b}",
                paint(
                    content,
                    if accent { GLASS_HIGHLIGHT } else { GLASS_BASE },
                    true
                )
            );
            let bot = format!("{b}{}{b}", paint(&"─".repeat(w), GLASS_BASE, true));
            format!("{top}\n{mid}\n{bot}")
        };

        // Frame 0: border forming (thin lines)
        frames.push(boxed("│", text, false));

        // Frame 1: solid glass pane
        frames.push(boxed("┃", text, true));

        // Frames 2-4: progressive cracking inside the pane
        for stage in 0..3 {
            let mut line = String::new();
            for (i, ch) in text.chars().enumerate() {
                let crack_chance = (stage + 1) as f64 / 3.0;
                let is_crack =
                    (i + stage) % (4 - stage) == 0 && (i as f64 / w as f64) < crack_chance;
                if is_crack {
                    line.push_str(&paint(shards[i % shards.len()], GLASS_SHARD, true));
                } else {
                    line.push(ch);
                }
            }
            frames.push(boxed("┃", &line, true));
        }

        // Frame 5: heavy fracture, border cracks
        let mut fractured = String::new();
        for (i, ch) in text.chars().enumerate() {
            if i % 2 == 0 {
                fractured.push_str(&paint(shards[i % shards.len()], GLASS_REFLECT, true));
            } else {
                fractured.push(ch);
            }
        }
        let top5 = paint(&format!("┏{}┓", "━".repeat(w)), GLASS_SHARD, true);
        let mid5 = format!(
            "{}  {}  {}",
            paint("┃", GLASS_SHARD, true),
            paint(&fractured, GLASS_HIGHLIGHT, true),
            paint("┃", GLASS_SHARD, true)
        );
        let bot5 = paint(&format!("┗{}┛", "━".repeat(w)), GLASS_SHARD, true);
        frames.push(format!("{top5}\n{mid5}\n{bot5}"));

        // Frame 6: text dissolving into shards
        let mut dissolve = String::new();
        for (i, ch) in text.chars().enumerate() {
            if i % 3 == 0 {
                dissolve.push_str(&paint(
                    glass_chars[i % glass_chars.len()],
                    GLASS_HIGHLIGHT,
                    true,
                ));
            } else if i % 3 == 1 {
                dissolve.push_str(&paint(shards[i % shards.len()], GLASS_REFLECT, true));
            } else {
                dissolve.push(ch);
            }
        }
        let top6 = paint(&format!("╔{}╗", "═".repeat(w)), GLASS_REFLECT, true);
        let mid6 = format!(
            "{}  {}  {}",
            paint("║", GLASS_REFLECT, true),
            paint(&dissolve, GLASS_HIGHLIGHT, true),
            paint("║", GLASS_REFLECT, true)
        );
        let bot6 = paint(&format!("╚{}╝", "═".repeat(w)), GLASS_REFLECT, true);
        frames.push(format!("{top6}\n{mid6}\n{bot6}"));

        // Frame 7: mostly particles, border fading
        let mut particles = String::new();
        for i in 0..w {
            particles.push_str(&paint(shards[i % shards.len()], GLASS_REFLECT, true));
        }
        let top7 = paint(&format!("  {}  ", "·".repeat(w)), GLASS_SHADOW, true);
        let mid7 = format!("  {}  ", paint(&particles, GLASS_HIGHLIGHT, true));
        let bot7 = paint(&format!("  {}  ", "·".repeat(w)), GLASS_SHADOW, true);
        frames.push(format!("{top7}\n{mid7}\n{bot7}"));

        // Frame 8: scattered remnants
        let mut remnants = String::new();
        for i in 0..w.max(4) {
            remnants.push_str(&paint(
                glass_chars[i % glass_chars.len()],
                GLASS_SHADOW,
                true,
            ));
            remnants.push(' ');
        }
        frames.push(format!("\n  {}  \n", paint(&remnants, GLASS_SHADOW, true)));

        // Frame 9: clear
        frames.push(String::new());

        Self { frames, current: 0 }
    }

    /// Get the current animation frame
    #[must_use]
    pub fn current_frame(&self) -> &str {
        &self.frames[self.current]
    }

    /// Advance to the next frame
    pub fn advance(&mut self) -> bool {
        if self.current < self.frames.len() - 1 {
            self.current += 1;
            true
        } else {
            false
        }
    }

    /// Reset animation to beginning
    pub fn reset(&mut self) {
        self.current = 0;
    }

    /// Get total frame count
    #[must_use]
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }
}

/// Draw a decorative glass border around content
#[must_use]
pub fn glass_border(content: &str, on: bool) -> String {
    if !on {
        return content.to_string();
    }

    let lines: Vec<&str> = content.lines().collect();
    let max_width = lines.iter().map(|l| display_width(l)).max().unwrap_or(0);

    let mut result = String::new();
    let top = paint(
        &format!("╭{}╮", "─".repeat(max_width + 2)),
        GLASS_BASE,
        true,
    );
    let bottom = paint(
        &format!("╰{}╯", "─".repeat(max_width + 2)),
        GLASS_SHADOW,
        true,
    );

    result.push_str(&top);
    result.push('\n');

    for line in &lines {
        let padded = format!(" {}{}", line, " ".repeat(max_width - display_width(line)));
        let left = paint("│", GLASS_HIGHLIGHT, true);
        let right = paint("│", GLASS_HIGHLIGHT, true);
        let _ = writeln!(result, "{left}{padded}{right}");
    }

    result.push_str(&bottom);
    result
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
        assert_eq!(severity_code(Severity::Info), "38;5;86");
        assert_eq!(severity_code(Severity::Warning), "38;5;208");
        assert_eq!(severity_code(Severity::Critical), "38;5;196;1");
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
