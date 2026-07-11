//! Manual command-line parser replacing `clap` for the `fract` binary.

use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone)]
pub struct Error {
    message: String,
}

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for Error {}

#[derive(Debug, Clone, Default)]
pub enum Command {
    #[default]
    Run,
    Index,
    Init {
        path: PathBuf,
    },
}

#[derive(Debug, Clone)]
pub struct Args {
    pub config: Option<PathBuf>,
    pub command: Command,
    /// Output format override (`human|json|jsonl|sarif|markdown`).
    pub format: Option<String>,
    /// Colour override (`auto|always|never`).
    pub color: Option<String>,
    /// Verbosity counter: `-v` adds one, `-q` subtracts one.
    pub verbosity: i32,
}

const HELP: &str = "Usage: fract [OPTIONS] [COMMAND]

Autonomous architectural maintenance daemon

Options:
  -c, --config <FILE>     Path to configuration file
  -f, --format <FORMAT>   Output format: human, json, jsonl, sarif, markdown
      --color <WHEN>      Colour output: auto, always, never
      --no-color          Disable colour output (same as --color=never)
  -v, --verbose           Increase verbosity (repeatable, e.g. -vv)
  -q, --quiet             Decrease verbosity
  -h, --help              Print help
  -V, --version           Print version

Commands:
  run       Run the daemon (default)
  index     Index the project and print module health
  init      Generate a default configuration file

Environment:
  NO_COLOR              When set, disables ANSI colour (same as --color=never)
  RUST_LOG              Tracing filter for the daemon (e.g. fract=debug)
";

impl Args {
    pub fn parse() -> Result<Self> {
        match Self::parse_from(std::env::args()) {
            Ok(args) => Ok(args),
            Err(e) if e.message == HELP || e.message == env!("CARGO_PKG_VERSION") => {
                println!("{}", e.message);
                std::process::exit(0);
            }
            Err(e) => Err(e),
        }
    }

    pub fn parse_from<I, S>(args: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut iter = args.into_iter().peekable();
        let _program = iter.next();

        let mut config: Option<PathBuf> = None;
        let mut command: Option<Command> = None;
        let mut format: Option<String> = None;
        let mut color: Option<String> = None;
        let mut verbosity: i32 = 0;

        while let Some(arg) = iter.next() {
            let arg = arg.as_ref();
            match arg {
                "-h" | "--help" => return Err(Error::new(HELP)),
                "-V" | "--version" => return Err(Error::new(env!("CARGO_PKG_VERSION"))),
                "-c" | "--config" => {
                    config = Some(PathBuf::from(next_value(&mut iter, "--config")?));
                }
                s if s.starts_with("--config=") => {
                    config = Some(PathBuf::from(&s["--config=".len()..]));
                }
                s if s.starts_with("-c") => {
                    config = Some(PathBuf::from(&s[2..]));
                }
                "-f" | "--format" => {
                    format = Some(next_value(&mut iter, "--format")?);
                }
                s if s.starts_with("--format=") => {
                    format = Some(s["--format=".len()..].to_string());
                }
                "--color" => {
                    color = Some(next_value(&mut iter, "--color")?);
                }
                s if s.starts_with("--color=") => {
                    color = Some(s["--color=".len()..].to_string());
                }
                "--no-color" => {
                    color = Some("never".to_string());
                }
                "-v" | "--verbose" => verbosity += 1,
                "-q" | "--quiet" => verbosity -= 1,
                s if is_short_verbosity(s) => {
                    for ch in s[1..].chars() {
                        match ch {
                            'v' => verbosity += 1,
                            'q' => verbosity -= 1,
                            _ => {}
                        }
                    }
                }
                "run" => command = Some(Command::Run),
                "index" => command = Some(Command::Index),
                "init" => {
                    let mut path = PathBuf::from(".");
                    // Copy the peeked token out so the immutable borrow of `iter`
                    // ends before we call `iter.next()` to consume it.
                    let peeked = iter.peek().map(|s| s.as_ref().to_string());
                    match peeked.as_deref() {
                        Some("--path") | Some("-p") => {
                            iter.next();
                            path = PathBuf::from(next_value(&mut iter, "--path")?);
                        }
                        Some(s) if s.starts_with("--path=") => {
                            let p = PathBuf::from(&s["--path=".len()..]);
                            iter.next();
                            path = p;
                        }
                        Some(s) if s.starts_with("-p") && s.len() > 2 => {
                            let p = PathBuf::from(&s[2..]);
                            iter.next();
                            path = p;
                        }
                        _ => {}
                    }
                    command = Some(Command::Init { path });
                }
                s if s.starts_with('-') => {
                    return Err(Error::new(format!("unexpected argument: {}", s)));
                }
                _ => return Err(Error::new(format!("unexpected argument: {}", arg))),
            }
        }

        Ok(Args {
            config,
            command: command.unwrap_or_default(),
            format,
            color,
            verbosity,
        })
    }
}

fn next_value<I, S>(iter: &mut std::iter::Peekable<I>, flag: &str) -> Result<String>
where
    I: Iterator<Item = S>,
    S: AsRef<str>,
{
    iter.next()
        .map(|s| s.as_ref().to_string())
        .ok_or_else(|| Error::new(format!("missing value for {flag}")))
}

fn is_short_verbosity(s: &str) -> bool {
    s.len() > 1 && s.starts_with('-') && s[1..].chars().all(|c| c == 'v' || c == 'q')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_command_is_run() {
        let args = Args::parse_from(["fract"]).unwrap();
        assert!(args.config.is_none());
        assert!(matches!(args.command, Command::Run));
        assert!(args.format.is_none());
        assert!(args.color.is_none());
        assert_eq!(args.verbosity, 0);
    }

    #[test]
    fn explicit_run() {
        let args = Args::parse_from(["fract", "run"]).unwrap();
        assert!(matches!(args.command, Command::Run));
    }

    #[test]
    fn index_command() {
        let args = Args::parse_from(["fract", "index"]).unwrap();
        assert!(matches!(args.command, Command::Index));
    }

    #[test]
    fn init_default_path() {
        let args = Args::parse_from(["fract", "init"]).unwrap();
        match args.command {
            Command::Init { path } => assert_eq!(path, PathBuf::from(".")),
            _ => panic!("expected Init command"),
        }
    }

    #[test]
    fn init_with_long_path() {
        let args = Args::parse_from(["fract", "init", "--path", "/tmp/project"]).unwrap();
        match args.command {
            Command::Init { path } => assert_eq!(path, PathBuf::from("/tmp/project")),
            _ => panic!("expected Init command"),
        }
    }

    #[test]
    fn init_with_short_path() {
        let args = Args::parse_from(["fract", "init", "-p", "src"]).unwrap();
        match args.command {
            Command::Init { path } => assert_eq!(path, PathBuf::from("src")),
            _ => panic!("expected Init command"),
        }
    }

    #[test]
    fn init_with_equals_path() {
        let args = Args::parse_from(["fract", "init", "--path=/foo/bar"]).unwrap();
        match args.command {
            Command::Init { path } => assert_eq!(path, PathBuf::from("/foo/bar")),
            _ => panic!("expected Init command"),
        }
    }

    #[test]
    fn config_long_flag() {
        let args = Args::parse_from(["fract", "--config", "fract.toml"]).unwrap();
        assert_eq!(args.config, Some(PathBuf::from("fract.toml")));
    }

    #[test]
    fn config_short_flag() {
        let args = Args::parse_from(["fract", "-c", "cfg.toml"]).unwrap();
        assert_eq!(args.config, Some(PathBuf::from("cfg.toml")));
    }

    #[test]
    fn config_combined_short_flag() {
        let args = Args::parse_from(["fract", "-ccfg.toml"]).unwrap();
        assert_eq!(args.config, Some(PathBuf::from("cfg.toml")));
    }

    #[test]
    fn config_equals_form() {
        let args = Args::parse_from(["fract", "--config=cfg.toml"]).unwrap();
        assert_eq!(args.config, Some(PathBuf::from("cfg.toml")));
    }

    #[test]
    fn config_before_subcommand() {
        let args = Args::parse_from(["fract", "--config", "cfg.toml", "index"]).unwrap();
        assert_eq!(args.config, Some(PathBuf::from("cfg.toml")));
        assert!(matches!(args.command, Command::Index));
    }

    #[test]
    fn rejects_unknown_flag() {
        assert!(Args::parse_from(["fract", "--unknown"]).is_err());
    }

    #[test]
    fn rejects_unknown_subcommand() {
        assert!(Args::parse_from(["fract", "foo"]).is_err());
    }

    #[test]
    fn rejects_positional_after_init() {
        assert!(Args::parse_from(["fract", "init", "foo"]).is_err());
    }

    #[test]
    fn rejects_missing_config_value() {
        assert!(Args::parse_from(["fract", "--config"]).is_err());
    }

    #[test]
    fn help_returns_error() {
        let err = Args::parse_from(["fract", "--help"]).unwrap_err();
        assert!(err.message.contains("Usage:"));
    }

    // ---- output flags -------------------------------------------------------

    #[test]
    fn format_long_and_short() {
        let a = Args::parse_from(["fract", "index", "--format", "json"]).unwrap();
        assert_eq!(a.format.as_deref(), Some("json"));
        let b = Args::parse_from(["fract", "-f", "sarif", "index"]).unwrap();
        assert_eq!(b.format.as_deref(), Some("sarif"));
        let c = Args::parse_from(["fract", "--format=jsonl", "index"]).unwrap();
        assert_eq!(c.format.as_deref(), Some("jsonl"));
    }

    #[test]
    fn color_flags() {
        let a = Args::parse_from(["fract", "--color", "never", "index"]).unwrap();
        assert_eq!(a.color.as_deref(), Some("never"));
        let b = Args::parse_from(["fract", "--no-color", "index"]).unwrap();
        assert_eq!(b.color.as_deref(), Some("never"));
        let c = Args::parse_from(["fract", "--color=always"]).unwrap();
        assert_eq!(c.color.as_deref(), Some("always"));
    }

    #[test]
    fn verbosity_accumulates() {
        assert_eq!(Args::parse_from(["fract", "-v"]).unwrap().verbosity, 1);
        assert_eq!(Args::parse_from(["fract", "-vv"]).unwrap().verbosity, 2);
        assert_eq!(Args::parse_from(["fract", "-q"]).unwrap().verbosity, -1);
        assert_eq!(
            Args::parse_from(["fract", "-vv", "-q"]).unwrap().verbosity,
            1
        );
    }

    #[test]
    fn output_flags_after_init_are_accepted() {
        let a = Args::parse_from(["fract", "init", "--format", "json"]).unwrap();
        assert_eq!(a.format.as_deref(), Some("json"));
        assert!(matches!(a.command, Command::Init { .. }));
    }

    #[test]
    fn missing_format_value_errors() {
        assert!(Args::parse_from(["fract", "--format"]).is_err());
    }
}
