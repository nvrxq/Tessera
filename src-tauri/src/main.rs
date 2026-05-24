// Prevent the launcher console on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    super_linux_lib::run();
}
