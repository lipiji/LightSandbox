use std::sync::Arc;
use std::time::Duration;

use crate::state::AppState;

pub fn spawn(state: Arc<AppState>, interval_seconds: u64, enabled: bool) {
    if !enabled {
        tracing::info!("GC disabled by config");
        return;
    }
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(interval_seconds.max(1)));
        loop {
            interval.tick().await;
            match state.runtime.cleanup_expired().await {
                Ok(removed) if removed > 0 => {
                    tracing::info!(removed, "GC removed expired sandboxes");
                }
                Ok(_) => {}
                Err(e) => tracing::warn!(error = %e, "GC cleanup failed"),
            }
        }
    });
}
