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
}

const HELP: &str = "Usage: fract [OPTIONS] [COMMAND]

Autonomous architectural maintenance daemon

Options:
  -c, --config <FILE>  Path to configuration file
  -h, --help           Print help
  -V, --version        Print version

Commands:
  run       Run the daemon (default)
  index     Index the project and print module health
  init      Generate a default configuration file
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

        while let Some(arg) = iter.next() {
            let arg = arg.as_ref();
            match arg {
                "-h" | "--help" => return Err(Error::new(HELP)),
                "-V" | "--version" => return Err(Error::new(env!("CARGO_PKG_VERSION"))),
                "-c" | "--config" => {
                    let value = iter
                        .next()
                        .ok_or_else(|| Error::new("missing value for --config"))?;
                    config = Some(PathBuf::from(value.as_ref()));
                }
                s if s.starts_with("--config=") => {
                    config = Some(PathBuf::from(&s["--config=".len()..]));
                }
                s if s.starts_with("-c") => {
                    config = Some(PathBuf::from(&s[2..]));
                }
                "run" => command = Some(Command::Run),
                "index" => command = Some(Command::Index),
                "init" => {
                    let mut path = PathBuf::from(".");
                    while let Some(n) = iter.peek() {
                        let next = n.as_ref().to_string();
                        match next.as_str() {
                            "-p" | "--path" => {
                                iter.next();
                                let value = iter
                                    .next()
                                    .ok_or_else(|| Error::new("missing value for --path"))?;
                                path = PathBuf::from(value.as_ref());
                            }
                            s if s.starts_with("--path=") => {
                                iter.next();
                                path = PathBuf::from(&s["--path=".len()..]);
                            }
                            s if s.starts_with("-p") => {
                                iter.next();
                                path = PathBuf::from(&s[2..]);
                            }
                            s if s.starts_with('-') => {
                                return Err(Error::new(format!(
                                    "unexpected argument for init: {}",
                                    s
                                )));
                            }
                            _ => break,
                        }
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
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_command_is_run() {
        let args = Args::parse_from(["fract"]).unwrap();
        assert!(args.config.is_none());
        assert!(matches!(args.command, Command::Run));
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
}
