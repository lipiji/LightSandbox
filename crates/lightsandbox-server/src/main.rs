use std::path::PathBuf;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "lightsandbox-server")]
struct Args {
    #[arg(long, default_value = "config.example.toml")]
    config: PathBuf,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let args = Args::parse();

    if let Err(e) = lightsandbox_server::run(&args.config).await {
        eprintln!("server error: {e}");
        std::process::exit(1);
    }
}
