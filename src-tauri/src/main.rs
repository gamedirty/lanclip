#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--selftest") {
        lanclip_lib::selftest::run();
        return;
    }
    lanclip_lib::run();
}
