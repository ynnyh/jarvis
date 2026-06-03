// Prevents additional console window on Windows, DO NOT REMOVE!!
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    if let Err(e) = jarvis_lib::run() {
        eprintln!("Jarvis 启动失败: {}", e);
    }
}
