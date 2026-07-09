use fract::cli::{Args, Command};
use fract::error::{Context, Result};
use fract::{config::Config, daemon::Daemon, web};
use std::path::PathBuf;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .init();

    let cli = Args::parse()?;

    match cli.command {
        Command::Init { path } => {
            let cfg = Config::default_for(path.canonicalize().unwrap_or(path));
            let text = toml::to_string_pretty(&cfg)?;
            std::fs::write("fract.toml", text)?;
            println!("Created fract.toml");
            Ok(())
        }
        Command::Index => {
            let cfg = load_config(cli.config)?;
            let indexer =
                fract::indexer::Indexer::new(cfg.project_root.clone(), cfg.ignore_patterns.clone());
            let modules = indexer.index()?;
            println!(
                "{:<40} {:>8} {:>8} {:>8} {:>10}",
                "module", "lines", "funcs", "entropy", "health"
            );
            for m in modules {
                println!(
                    "{:<40} {:>8} {:>8} {:>8.2} {:>10}",
                    m.path.display(),
                    m.lines,
                    m.functions,
                    m.entropy,
                    format!("{:?}", m.health)
                );
            }
            Ok(())
        }
        Command::Run => {
            let cfg = load_config(cli.config)?;
            let daemon = Arc::new(Daemon::new(cfg));
            let daemon_clone = Arc::clone(&daemon);
            daemon.run().await?;
            // Start web dashboard.
            web::serve(&daemon_clone.config().clone(), daemon_clone).await?;
            Ok(())
        }
    }
}

fn load_config(path: Option<PathBuf>) -> Result<Config> {
    if let Some(p) = path {
        Config::load(p)
    } else if std::path::Path::new("fract.toml").exists() {
        Config::load("fract.toml")
    } else {
        Ok(Config::default_for(
            std::env::current_dir().context("no working directory")?,
        ))
    }
}
