use fract::cli::{Args, Command};
use fract::config::Config;
use fract::error::Result;
use fract::report::{ColorChoice, OutputFormat, Report, Style, Verbosity};
use fract::{daemon::Daemon, web};
use std::path::PathBuf;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

fn main() {
    // Parse before building the runtime so `--help`/`--version` can exit cleanly
    // and so the verbosity flag can size the log level.
    let cli = match Args::parse() {
        Ok(args) => args,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(2);
        }
    };

    init_tracing(cli.verbosity);

    let rt = tokio::runtime::Runtime::new().expect("build tokio runtime");
    if let Err(e) = rt.block_on(run(cli)) {
        // Print the human-readable Display chain, never the `{:?}` Debug dump.
        eprintln!("error: {e}");
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
            println!("Created {}", out.display());
            Ok(())
        }
        Command::Index => {
            let cfg = load_config(cli.config.clone())?;
            let (format, color, verbosity) = resolve_output(&cli, &cfg)?;
            let indexer =
                fract::indexer::Indexer::new(cfg.project_root.clone(), cfg.ignore_patterns.clone());
            let modules = indexer.index()?;
            let mut report =
                Report::from_modules(&cfg.project_root, &modules, cfg.entropy_threshold, false);
            report.apply_budget(cfg.output.max_findings);
            let style = Style::detect(color);
            let rendered = report.render(format, &style, verbosity);
            if rendered.ends_with('\n') {
                print!("{rendered}");
            } else {
                println!("{rendered}");
            }
            Ok(())
        }
        Command::Run => {
            let cfg = load_config(cli.config.clone())?;
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
