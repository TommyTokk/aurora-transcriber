#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// Starts the desktop application and propagates initialization failures.
fn main() -> anyhow::Result<()> {
    aurora_lib::run()
}
