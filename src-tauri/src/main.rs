// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // The uninstaller's one-shot: take tug's own `bin` folder off the user's PATH, then exit
    // without starting the app.
    if let Some(code) = tug_lib::run_uninstall_step(std::env::args().skip(1)) {
        std::process::exit(code);
    }
    tug_lib::run()
}
