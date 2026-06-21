use std::io::Write as _;
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use futures_util::StreamExt;
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
        /// Name of a template (subdir of templates_dir) to pre-populate the workspace.
        #[arg(long)]
        template: Option<String>,
    },
    /// List all sandboxes.
    List,
    /// Execute a command inside a sandbox.
    Exec {
        id: String,
        cmd: String,
        #[arg(long)]
        timeout_seconds: Option<u64>,
        /// Print stdout/stderr incrementally as the command runs instead of
        /// waiting for it to finish.
        #[arg(long)]
        stream: bool,
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
        Commands::Create {
            ttl_seconds,
            template,
        } => {
            let spec = SandboxSpec {
                ttl_seconds,
                metadata: None,
                env: None,
                template,
            };
            post_json(&client, &cli.base_url, "/v1/sandboxes", &spec).await
        }
        Commands::List => get_json(&client, &cli.base_url, "/v1/sandboxes").await,
        Commands::Exec {
            id,
            cmd,
            timeout_seconds,
            stream,
        } => {
            if stream {
                if let Err(e) =
                    run_exec_stream(&client, &cli.base_url, &id, &cmd, timeout_seconds, cli.json)
                        .await
                {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
                return;
            }
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

/// Hits `.../exec/stream` and prints stdout/stderr chunks as they arrive
/// instead of buffering the whole result like `post_json` does.
async fn run_exec_stream(
    client: &reqwest::Client,
    base_url: &str,
    id: &str,
    cmd: &str,
    timeout_seconds: Option<u64>,
    json_mode: bool,
) -> Result<(), String> {
    let req = ExecRequest {
        cmd: cmd.to_string(),
        timeout_seconds,
        env: None,
    };
    let response = client
        .post(format!("{base_url}/v1/sandboxes/{id}/exec/stream"))
        .json(&req)
        .send()
        .await
        .map_err(|e| format!("connection error: {e}"))?;

    if !response.status().is_success() {
        let value: Value = response
            .json()
            .await
            .map_err(|e| format!("invalid response body: {e}"))?;
        return Err(value
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .unwrap_or("request failed")
            .to_string());
    }

    let mut byte_stream = response.bytes_stream();
    let mut buf = String::new();
    let mut done_value: Option<Value> = None;

    while let Some(chunk) = byte_stream.next().await {
        let chunk = chunk.map_err(|e| format!("stream error: {e}"))?;
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(idx) = buf.find("\n\n") {
            let frame = buf[..idx].to_string();
            buf.drain(..idx + 2);
            let Some((event, data)) = parse_sse_frame(&frame) else {
                continue;
            };
            match event.as_str() {
                "stdout" => {
                    print!("{data}");
                    let _ = std::io::stdout().flush();
                }
                "stderr" => {
                    eprint!("{data}");
                    let _ = std::io::stderr().flush();
                }
                "done" => done_value = serde_json::from_str(&data).ok(),
                "error" => return Err(data),
                _ => {}
            }
        }
    }

    if let Some(done) = done_value {
        if json_mode {
            println!("{done}");
        } else {
            eprintln!(
                "exit_code={} timed_out={} duration_ms={}",
                done.get("exit_code").and_then(Value::as_i64).unwrap_or(-1),
                done.get("timed_out")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                done.get("duration_ms")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
            );
        }
    }

    Ok(())
}

/// Parses one blank-line-delimited SSE frame into `(event, data)`. Per the
/// SSE spec, multiple `data:` lines within one frame join with `\n`.
fn parse_sse_frame(frame: &str) -> Option<(String, String)> {
    let mut event = String::new();
    let mut data_lines = Vec::new();
    for line in frame.lines() {
        if let Some(rest) = line.strip_prefix("event:") {
            event = rest.trim_start().to_string();
        } else if let Some(rest) = line.strip_prefix("data:") {
            data_lines.push(rest.strip_prefix(' ').unwrap_or(rest).to_string());
        }
    }
    if event.is_empty() && data_lines.is_empty() {
        None
    } else {
        Some((event, data_lines.join("\n")))
    }
}

fn print_result(value: &Value, json_mode: bool) {
    if json_mode {
        println!("{value}");
    } else {
        println!("{}", serde_json::to_string_pretty(value).unwrap());
    }
}
