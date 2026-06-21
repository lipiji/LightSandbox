//! Optional on-disk store for sandbox metadata, used only to survive process
//! restarts. It is a best-effort write-through layer sitting next to the
//! in-memory `DashMap` in `LocalProcessRuntime` — never the source of truth
//! while the server is running, consulted once at startup to repopulate
//! memory. Disabled by default (`RuntimeConfig::persistence_db_path` is
//! `None`), so v0.1's zero-database guarantee still holds unless an operator
//! opts in.
//!
//! Backed by [`redb`](https://crates.io/crates/redb), a pure-Rust embedded
//! key-value store. This is a deliberate choice over SQLite: `libsqlite3-sys`
//! ships SQLite's C amalgamation and needs a working C toolchain (gcc/clang/
//! MSVC) to compile, which this project does not want to require — especially
//! on a Windows-gnu host without MinGW/MSVC installed. `redb` is 100% Rust and
//! keeps the build self-contained, matching the project's "lighter than
//! Docker" identity.

use std::collections::HashMap;
use std::path::Path;

use chrono::{DateTime, Utc};
use lightsandbox_core::{LightSandboxError, SandboxInfo, SandboxStatus};
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};

/// `redb` table mapping sandbox id → serialized [`PersistedRecord`].
const SANDBOXES: TableDefinition<&str, &[u8]> = TableDefinition::new("sandboxes");

/// A sandbox record loaded back from the store at startup, paired with its
/// exec environment (not part of `SandboxInfo`, but needed to fully
/// reconstruct an in-memory `SandboxEntry`).
pub struct PersistedSandbox {
    pub info: SandboxInfo,
    pub env: HashMap<String, String>,
}

/// The on-disk representation of one sandbox. Timestamps and status are stored
/// as strings to keep (de)serialization trivial and forward-compatible; the
/// in-memory types round-trip through [`status_to_str`]/[`parse_timestamp`].
#[derive(Serialize, Deserialize)]
struct PersistedRecord {
    id: String,
    status: String,
    workspace_path: String,
    created_at: String,
    expires_at: Option<String>,
    metadata: HashMap<String, String>,
    env: HashMap<String, String>,
}

/// A handle to the on-disk metadata store. Cheap to share via `Arc`; `redb`
/// is internally thread-safe (write transactions are exclusive, read
/// transactions are concurrent) so no extra `Mutex` is needed.
pub struct MetadataStore {
    db: Database,
}

impl MetadataStore {
    /// Opens (creating if absent) the database file at `path`, ensuring its
    /// parent directory exists and the `sandboxes` table is initialized so a
    /// fresh database behaves like an empty one.
    pub fn open(path: &Path) -> Result<Self, LightSandboxError> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    LightSandboxError::ConfigError(format!("creating persistence directory: {e}"))
                })?;
            }
        }

        // redb separates create-vs-open; branch so the first run creates the
        // file and subsequent runs reopen it. (A plain `Database::create`
        // would error on an already-existing file.)
        let db = if path.exists() {
            Database::open(path)
        } else {
            Database::create(path)
        }
        .map_err(|e| LightSandboxError::ConfigError(format!("opening persistence db: {e}")))?;

        // Eagerly create the table so `load_all` on a fresh database returns
        // an empty vec rather than a "table not found" error.
        let txn = db
            .begin_write()
            .map_err(|e| LightSandboxError::ConfigError(format!("init persistence db: {e}")))?;
        {
            let _ = txn
                .open_table(SANDBOXES)
                .map_err(|e| LightSandboxError::ConfigError(format!("init persistence db: {e}")))?;
        }
        txn.commit()
            .map_err(|e| LightSandboxError::ConfigError(format!("init persistence db: {e}")))?;

        Ok(Self { db })
    }

    /// Loads every persisted sandbox record. Called exactly once, at startup.
    pub fn load_all(&self) -> Result<Vec<PersistedSandbox>, LightSandboxError> {
        let txn = self.db.begin_read().map_err(|e| perr("begin read", e))?;
        let table = match txn.open_table(SANDBOXES) {
            Ok(t) => t,
            // A freshly created db already has the table (see `open`), but
            // defend against an externally-truncated file by treating a
            // missing table as empty rather than fatal.
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(e) => return Err(perr("open table", e)),
        };

        let mut out = Vec::new();
        for item in table.iter().map_err(|e| perr("iter", e))? {
            let (_, value) = item.map_err(|e| perr("read row", e))?;
            let record: PersistedRecord =
                serde_json::from_slice(value.value()).map_err(|e| perr("decode row", e))?;
            out.push(PersistedSandbox {
                info: SandboxInfo {
                    id: record.id,
                    status: status_from_str(&record.status)?,
                    workspace_path: record.workspace_path,
                    created_at: parse_timestamp(&record.created_at)?,
                    expires_at: record
                        .expires_at
                        .as_deref()
                        .map(parse_timestamp)
                        .transpose()?,
                    metadata: record.metadata,
                },
                env: record.env,
            });
        }
        Ok(out)
    }

    /// Inserts or replaces the record for `info.id` with the current in-memory
    /// state. Whole-record upsert keeps the on-disk copy simple to reason
    /// about: the in-memory `DashMap` is authoritative, so we just mirror it.
    pub fn upsert(
        &self,
        info: &SandboxInfo,
        env: &HashMap<String, String>,
    ) -> Result<(), LightSandboxError> {
        let record = PersistedRecord {
            id: info.id.clone(),
            status: status_to_str(info.status).to_string(),
            workspace_path: info.workspace_path.clone(),
            created_at: info.created_at.to_rfc3339(),
            expires_at: info.expires_at.map(|t| t.to_rfc3339()),
            metadata: info.metadata.clone(),
            env: env.clone(),
        };
        let bytes = serde_json::to_vec(&record).map_err(|e| perr("encode record", e))?;

        let txn = self.db.begin_write().map_err(|e| perr("begin write", e))?;
        {
            let mut table = txn
                .open_table(SANDBOXES)
                .map_err(|e| perr("open table", e))?;
            table
                .insert(record.id.as_str(), bytes.as_slice())
                .map_err(|e| perr("insert", e))?;
        }
        txn.commit().map_err(|e| perr("commit", e))?;
        Ok(())
    }

    /// Removes the record for `id`, if any. Idempotent: removing a missing id
    /// is not an error.
    pub fn delete(&self, id: &str) -> Result<(), LightSandboxError> {
        let txn = self.db.begin_write().map_err(|e| perr("begin write", e))?;
        {
            let mut table = txn
                .open_table(SANDBOXES)
                .map_err(|e| perr("open table", e))?;
            table.remove(id).map_err(|e| perr("delete", e))?;
        }
        txn.commit().map_err(|e| perr("commit", e))?;
        Ok(())
    }
}

/// Lifts any `redb` error implementing `Display` into a `RuntimeError` with a
/// short context tag. Keeps the call sites readable without one bespoke
/// converter per `redb` error type.
fn perr<E: std::fmt::Display>(ctx: &'static str, e: E) -> LightSandboxError {
    LightSandboxError::RuntimeError(format!("persistence {ctx}: {e}"))
}

fn parse_timestamp(s: &str) -> Result<DateTime<Utc>, LightSandboxError> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| LightSandboxError::RuntimeError(format!("invalid persisted timestamp: {e}")))
}

fn status_to_str(status: SandboxStatus) -> &'static str {
    match status {
        SandboxStatus::Creating => "creating",
        SandboxStatus::Running => "running",
        SandboxStatus::Stopped => "stopped",
        SandboxStatus::Failed => "failed",
        SandboxStatus::Expired => "expired",
        SandboxStatus::Removed => "removed",
    }
}

fn status_from_str(s: &str) -> Result<SandboxStatus, LightSandboxError> {
    match s {
        "creating" => Ok(SandboxStatus::Creating),
        "running" => Ok(SandboxStatus::Running),
        "stopped" => Ok(SandboxStatus::Stopped),
        "failed" => Ok(SandboxStatus::Failed),
        "expired" => Ok(SandboxStatus::Expired),
        "removed" => Ok(SandboxStatus::Removed),
        other => Err(LightSandboxError::RuntimeError(format!(
            "unknown persisted sandbox status: {other}"
        ))),
    }
}
