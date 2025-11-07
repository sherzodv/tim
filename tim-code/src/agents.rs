pub mod chatgpt;

use tracing::info;

pub fn spawn_all(endpoint: &str) {
    if chatgpt::spawn(endpoint) {
        info!("ChatGPT agent initialized (endpoint: {endpoint})");
    }
}
