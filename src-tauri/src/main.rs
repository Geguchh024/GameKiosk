#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Write a marker so we know the process at least started
    let log_path = std::env::current_exe()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join("gamekiosk_debug.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&log_path) {
        use std::io::Write;
        let _ = writeln!(f, "=== main() entered ===");
    }

    gamekiosk_lib::run();
}
