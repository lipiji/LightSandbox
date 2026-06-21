use std::sync::Arc;

use lightsandbox_core::SandboxRuntime;

pub struct AppState {
    pub runtime: Arc<dyn SandboxRuntime>,
}
