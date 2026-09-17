//! Binary entry point: parses CLI args, installs tracing, and dispatches
//! the `init` / `index` / `run` commands on a tokio runtime (`run`
//! starts the daemon plus the web dashboard).

use fract::cli::{Args, Command};
use fract::config::Config;
use fract::error::Result;
use fract::report::{
    glass_paint, ColorChoice, GlassAnimation, OutputFormat, Report, Style, Verbosity, SUCCESS_COLOR,
};
use fract::shatter;
use fract::{daemon::Daemon, web};
use std::path::PathBuf;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

mod update;

/// Helper to run glass animation if color is enabled
fn run_animation(text: &str, style: &Style) {
    if style.color {
        let mut anim = GlassAnimation::new(text, true);
        for _ in 0..anim.frame_count() {
            println!("\x1b[2J\x1b[H{}", anim.current_frame());
            std::thread::sleep(std::time::Duration::from_millis(80));
            anim.advance();
        }
        println!("\x1b[2J\x1b[H");
    }
}

/// Helper to resolve color choice from CLI args (before config is loaded)
fn resolve_color_from_cli(cli_color: Option<String>) -> ColorChoice {
    cli_color
        .and_then(|c| ColorChoice::parse(&c))
        .unwrap_or(ColorChoice::Auto)
}

fn main() {
    // Parse before building the runtime so `--help`/`--version` can exit cleanly
    // and so the verbosity flag can size the log level.
    let cli = match Args::parse() {
        Ok(args) => args,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };

    init_tracing(cli.verbosity);

    let rt = tokio::runtime::Runtime::new().expect("build tokio runtime");
    if let Err(e) = rt.block_on(run(cli)) {
        // Print the human-readable Display chain, never the `{:?}` Debug dump.
        let style = Style::detect(ColorChoice::Auto);
        eprintln!(
            "💎 {}",
            glass_paint(&e.to_string(), "38;5;196", style.color)
        );
        std::process::exit(1);
    }
}

fn init_tracing(verbosity: i32) {
    let level = match Verbosity::from_count(verbosity) {
        Verbosity::Quiet => tracing::Level::WARN,
        Verbosity::Normal => tracing::Level::INFO,
        Verbosity::Verbose => tracing::Level::DEBUG,
        Verbosity::Debug => tracing::Level::TRACE,
    };
    let filter = EnvFilter::from_default_env().add_directive(level.into());
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

async fn run(cli: Args) -> Result<()> {
    match cli.command {
        Command::Update => update::run(),
        Command::Init { path } => {
            std::fs::create_dir_all(&path)
                .map_err(|e| format!("create project dir {}: {e}", path.display()))?;
            let root = path
                .canonicalize()
                .map_err(|e| format!("resolve project root {}: {e}", path.display()))?;
            let cfg = Config::default_for(root.clone());
            let text = toml::to_string_pretty(&cfg)?;
            let out = root.join("fract.toml");
            std::fs::write(&out, text)?;

            let color = resolve_color_from_cli(cli.color.clone());
            let style = Style::detect(color);
            run_animation("Creating fract.toml", &style);
            println!(
                "Created {}",
                glass_paint(&out.display().to_string(), SUCCESS_COLOR, style.color)
            );
            Ok(())
        }
        Command::Index => {
            let cfg = load_config(cli.config.clone())?;
            let (format, color, verbosity) = resolve_output(&cli, &cfg)?;
            let style = Style::detect(color);

            run_animation("Analyzing project structure", &style);

            let indexer =
                fract::indexer::Indexer::new(cfg.project_root.clone(), cfg.ignore_patterns.clone());
            let outcome = indexer.index()?;
            let mut report = Report::from_modules(
                &cfg.project_root,
                &outcome.modules,
                outcome.files_walked,
                cfg.entropy_threshold,
                false,
            );
            report.apply_budget(cfg.output.max_findings);
            let rendered = report.render(format, &style, verbosity);

            if rendered.ends_with('\n') {
                print!("{rendered}");
            } else {
                println!("{rendered}");
            }
            Ok(())
        }
        Command::Shatter {
            candidates,
            dry_run,
            skip_validation,
        } => {
            let cfg = load_config(cli.config.clone())?;
            let color = resolve_color_from_cli(cli.color.clone());
            let style = Style::detect(color);

            run_animation("Executing shatter transformations", &style);

            let report = shatter::execute_shatter(
                cfg.project_root,
                candidates,
                skip_validation,
                dry_run,
            )
            .await?;

            if report.candidates_failed > 0 || !report.errors.is_empty() {
                for error in &report.errors {
                    eprintln!("{}", glass_paint(error, "38;5;196", style.color));
                }
                return Err(format!(
                    "shatter: {} candidate(s) failed",
                    report.candidates_failed
                ).into());
            }

            println!(
                "{}",
                glass_paint(
                    &format!(
                        "✓ Shattered {} candidate(s), {} succeeded",
                        report.candidates_attempted, report.candidates_succeeded
                    ),
                    SUCCESS_COLOR,
                    style.color
                )
            );
            Ok(())
        }
        Command::Run => {
            let cfg = load_config(cli.config.clone())?;
            let (_format, color, _verbosity) = resolve_output(&cli, &cfg)?;
            let style = Style::detect(color);

            run_animation("Starting fract daemon", &style);
            println!(
                "{}",
                glass_paint("Fract daemon started", SUCCESS_COLOR, style.color)
            );

            let daemon = Arc::new(Daemon::new(cfg));
            let daemon_clone = Arc::clone(&daemon);
            daemon.run().await?;
            // Start web dashboard.
            web::serve(&daemon_clone.config().clone(), daemon_clone).await?;
            Ok(())
        }
    }
}

/// CLI flags override config-file values; unknown strings produce a clear error.
fn resolve_output(cli: &Args, cfg: &Config) -> Result<(OutputFormat, ColorChoice, Verbosity)> {
    let format_str = cli
        .format
        .clone()
        .unwrap_or_else(|| cfg.output.format.clone());
    let format = OutputFormat::parse(&format_str)
        .ok_or_else(|| format!("unknown output format: {format_str}"))?;

    let color_str = cli
        .color
        .clone()
        .unwrap_or_else(|| cfg.output.color.clone());
    let color =
        ColorChoice::parse(&color_str).ok_or_else(|| format!("unknown color mode: {color_str}"))?;

    let verbosity = if cli.verbosity != 0 {
        Verbosity::from_count(cli.verbosity)
    } else {
        Verbosity::parse(&cfg.output.verbosity).unwrap_or(Verbosity::Normal)
    };

    Ok((format, color, verbosity))
}

fn load_config(path: Option<PathBuf>) -> Result<Config> {
    if let Some(p) = path {
        Config::load(p)
    } else if std::path::Path::new("fract.toml").exists() {
        Config::load("fract.toml")
    } else {
        Ok(Config::default_for(
            std::env::current_dir().map_err(|e| format!("no working directory: {e}"))?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        // Rust runs the test binary's tests in parallel threads within one
        // process, so a pid-only name would collide. Mix in a per-call counter.
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("fract-main-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cli_args(format: Option<&str>, color: Option<&str>, verbosity: i32) -> Args {
        Args {
            config: None,
            command: Command::Run,
            format: format.map(str::to_string),
            color: color.map(str::to_string),
            verbosity,
        }
    }

    #[test]
    fn resolve_output_prefers_cli_flags_over_config() {
        let cfg = Config::default_for(PathBuf::from("/tmp/example"));
        let args = cli_args(Some("json"), Some("never"), 1);
        let (format, color, verbosity) = resolve_output(&args, &cfg).unwrap();
        assert_eq!(format, OutputFormat::Json);
        assert_eq!(color, ColorChoice::Never);
        assert_eq!(verbosity, Verbosity::Verbose);
    }

    #[test]
    fn resolve_output_falls_back_to_config_values() {
        let mut cfg = Config::default_for(PathBuf::from("/tmp/example"));
        cfg.output.format = "sarif".to_string();
        cfg.output.color = "always".to_string();
        cfg.output.verbosity = "debug".to_string();
        let args = cli_args(None, None, 0);
        let (format, color, verbosity) = resolve_output(&args, &cfg).unwrap();
        assert_eq!(format, OutputFormat::Sarif);
        assert_eq!(color, ColorChoice::Always);
        assert_eq!(verbosity, Verbosity::Debug);
    }

    #[test]
    fn resolve_output_quiet_flag_maps_to_quiet() {
        let cfg = Config::default_for(PathBuf::from("/tmp/example"));
        let args = cli_args(None, None, -2);
        let (_, _, verbosity) = resolve_output(&args, &cfg).unwrap();
        assert_eq!(verbosity, Verbosity::Quiet);
    }

    #[test]
    fn resolve_output_unknown_format_is_a_clear_error() {
        let cfg = Config::default_for(PathBuf::from("/tmp/example"));
        let args = cli_args(Some("bogus"), None, 0);
        let err = resolve_output(&args, &cfg).unwrap_err();
        assert!(
            err.to_string().contains("unknown output format: bogus"),
            "err: {err}"
        );
    }

    #[test]
    fn resolve_output_unknown_color_is_a_clear_error() {
        let cfg = Config::default_for(PathBuf::from("/tmp/example"));
        let args = cli_args(None, Some("purple"), 0);
        let err = resolve_output(&args, &cfg).unwrap_err();
        assert!(
            err.to_string().contains("unknown color mode: purple"),
            "err: {err}"
        );
    }

    #[test]
    fn resolve_output_unknown_config_verbosity_defaults_to_normal() {
        let mut cfg = Config::default_for(PathBuf::from("/tmp/example"));
        cfg.output.verbosity = "chatty".to_string();
        let args = cli_args(None, None, 0);
        let (_, _, verbosity) = resolve_output(&args, &cfg).unwrap();
        assert_eq!(verbosity, Verbosity::Normal);
    }

    #[test]
    fn load_config_reads_explicit_path() {
        let dir = temp_dir();
        let path = dir.join("fract.toml");
        std::fs::write(
            &path,
            format!(
                "project_root = \"{}\"\nmode = \"autonomous\"\n",
                dir.display()
            ),
        )
        .unwrap();
        let cfg = load_config(Some(path)).unwrap();
        assert_eq!(cfg.project_root, dir.canonicalize().unwrap());
        assert_eq!(cfg.mode.to_string(), "autonomous");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_config_missing_explicit_path_errors() {
        let dir = temp_dir();
        assert!(load_config(Some(dir.join("missing.toml"))).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn run_init_writes_config_into_target_dir() {
        let dir = temp_dir();
        let args = Args {
            config: None,
            command: Command::Init { path: dir.clone() },
            format: None,
            color: None,
            verbosity: 0,
        };
        run(args).await.unwrap();
        let written = dir.join("fract.toml");
        assert!(written.exists(), "init must create fract.toml");
        let cfg = Config::load(&written).unwrap();
        assert_eq!(cfg.project_root, dir.canonicalize().unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn run_index_renders_report_for_temp_project() {
        let dir = temp_dir();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/lib.rs"), "pub fn a() -> i32 { 1 }\n").unwrap();
        let cfg_path = dir.join("fract.toml");
        std::fs::write(&cfg_path, format!("project_root = \"{}\"\n", dir.display())).unwrap();
        let args = Args {
            config: Some(cfg_path),
            command: Command::Index,
            format: Some("json".to_string()),
            color: None,
            verbosity: 0,
        };
        run(args).await.unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
