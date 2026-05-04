// Hide the Windows console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    maeclyprint_lib::run();
}
