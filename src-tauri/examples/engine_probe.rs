//! Headless readiness probe for reproducible debug/release startup validation.
use aurora_lib::engine::TranscriberEngine;

/// Loads resources once and prints only safe readiness or failure diagnostics.
fn main() {
    let result = std::env::var_os("AURORA_ASR_MODEL_DIR")
        .ok_or(aurora_lib::engine::EngineError::Configuration)
        .and_then(|path| TranscriberEngine::new(std::path::Path::new(&path)));
    match result {
        Ok(engine) => println!("ready: {:?}", engine.provider()),
        Err(error) => println!("unavailable: {error}"),
    }
}
