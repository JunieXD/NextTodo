// A release build on Windows is a window app, without a console window of its own.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    nexttodo_desktop::run()
}
