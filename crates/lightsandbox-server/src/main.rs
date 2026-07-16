use std::path::PathBuf;

use clap::Parser;
use lightsandbox_server::config::AppConfig;

#[derive(Parser, Debug)]
#[command(
    name = "lightsandbox-server",
    about = "Self-hosted sandbox execution for AI agents",
    long_about = "Starts the LightSandbox REST API server.\n\nConfig discovery order:\n  1. --config <path> if provided\n  2. ./lightsandbox.toml in the current directory\n  3. Built-in defaults (no file needed)"
)]
struct Args {
    /// Path to a TOML config file. If omitted, auto-discovers lightsandbox.toml
    /// or falls back to built-in defaults — no file is required to start.
    #[arg(long)]
    config: Option<PathBuf>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let args = Args::parse();

    let config_path = args.config.or_else(|| {
        let local = PathBuf::from("lightsandbox.toml");
        if local.exists() {
            Some(local)
        } else {
            None
        }
    });

    match &config_path {
        Some(p) => tracing::info!(path = %p.display(), "loading config file"),
        None => tracing::info!("no config file — using built-in defaults"),
    }

    let config = match AppConfig::load_or_default(config_path.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };

    let workspace = PathBuf::from(&config.runtime.workspace_root);
    if !workspace.exists() {
        if let Err(e) = std::fs::create_dir_all(&workspace) {
            eprintln!(
                "error: could not create workspace directory {}: {e}",
                workspace.display()
            );
            std::process::exit(1);
        }
        tracing::info!(path = %workspace.display(), "created workspace directory");
    }

    if let Err(e) = lightsandbox_server::run(config).await {
        eprintln!("server error: {e}");
        std::process::exit(1);
    }
}
