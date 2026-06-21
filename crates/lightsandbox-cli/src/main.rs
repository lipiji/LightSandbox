use std::path::PathBuf;

use clap::{Parser, Subcommand};
use lightsandbox_core::{ExecRequest, SandboxSpec};
use serde_json::{json, Value};

#[derive(Parser, Debug)]
#[command(name = "lightsandbox", about = "LightSandbox CLI")]
struct Cli {
    /// Base URL of a running lightsandbox-server.
    #[arg(long, default_value = "http://127.0.0.1:8080", global = true)]
    base_url: String,

    /// Print raw JSON instead of a human-readable summary.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Run the lightsandbox-server (foreground).
    Server {
        #[arg(long, default_value = "config.example.toml")]
        config: PathBuf,
    },
    /// Create a new sandbox.
    Create {
        #[arg(long)]
        ttl_seconds: Option<u64>,
    },
    /// List all sandboxes.
    List,
    /// Execute a command inside a sandbox.
    Exec {
        id: String,
        cmd: String,
        #[arg(long)]
        timeout_seconds: Option<u64>,
    },
    /// Write a local file into a sandbox's workspace.
    Write {
        id: String,
        local_path: PathBuf,
        remote_path: String,
    },
    /// Read a file from a sandbox's workspace and print its content.
    Read { id: String, remote_path: String },
    /// Remove a sandbox.
    Rm { id: String },
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();
    let client = reqwest::Client::new();

    let result = match cli.command {
        Commands::Server { config } => {
            if let Err(e) = lightsandbox_server::run(&config).await {
                eprintln!("server error: {e}");
                std::process::exit(1);
            }
            return;
        }
        Commands::Create { ttl_seconds } => {
            let spec = SandboxSpec {
                ttl_seconds,
                metadata: None,
                env: None,
            };
            post_json(&client, &cli.base_url, "/v1/sandboxes", &spec).await
        }
        Commands::List => get_json(&client, &cli.base_url, "/v1/sandboxes").await,
        Commands::Exec {
            id,
            cmd,
            timeout_seconds,
        } => {
            let req = ExecRequest {
                cmd,
                timeout_seconds,
                env: None,
            };
            post_json(
                &client,
                &cli.base_url,
                &format!("/v1/sandboxes/{id}/exec"),
                &req,
            )
            .await
        }
        Commands::Write {
            id,
            local_path,
            remote_path,
        } => {
            let content = match std::fs::read_to_string(&local_path) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("failed to read {}: {e}", local_path.display());
                    std::process::exit(1);
                }
            };
            put_json(
                &client,
                &cli.base_url,
                &format!("/v1/sandboxes/{id}/files"),
                &json!({"path": remote_path, "content": content}),
            )
            .await
        }
        Commands::Read { id, remote_path } => {
            get_json_with_query(
                &client,
                &cli.base_url,
                &format!("/v1/sandboxes/{id}/files"),
                &[("path", remote_path.as_str())],
            )
            .await
        }
        Commands::Rm { id } => {
            delete_json(&client, &cli.base_url, &format!("/v1/sandboxes/{id}")).await
        }
    };

    match result {
        Ok(value) => print_result(&value, cli.json),
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

async fn post_json(
    client: &reqwest::Client,
    base_url: &str,
    path: &str,
    body: &impl serde::Serialize,
) -> Result<Value, String> {
    send(client.post(format!("{base_url}{path}")).json(body)).await
}

async fn put_json(
    client: &reqwest::Client,
    base_url: &str,
    path: &str,
    body: &impl serde::Serialize,
) -> Result<Value, String> {
    send(client.put(format!("{base_url}{path}")).json(body)).await
}

async fn get_json(client: &reqwest::Client, base_url: &str, path: &str) -> Result<Value, String> {
    send(client.get(format!("{base_url}{path}"))).await
}

async fn get_json_with_query(
    client: &reqwest::Client,
    base_url: &str,
    path: &str,
    query: &[(&str, &str)],
) -> Result<Value, String> {
    send(client.get(format!("{base_url}{path}")).query(query)).await
}

async fn delete_json(
    client: &reqwest::Client,
    base_url: &str,
    path: &str,
) -> Result<Value, String> {
    send(client.delete(format!("{base_url}{path}"))).await
}

async fn send(builder: reqwest::RequestBuilder) -> Result<Value, String> {
    let response = builder
        .send()
        .await
        .map_err(|e| format!("connection error: {e}"))?;
    let status = response.status();
    let value: Value = response
        .json()
        .await
        .map_err(|e| format!("invalid response body: {e}"))?;
    if !status.is_success() {
        return Err(value
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .unwrap_or("request failed")
            .to_string());
    }
    Ok(value)
}

fn print_result(value: &Value, json_mode: bool) {
    if json_mode {
        println!("{value}");
    } else {
        println!("{}", serde_json::to_string_pretty(value).unwrap());
    }
}
