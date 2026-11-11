// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // einmalige Einrichtung der iPhone-Kamera mit Administratorrechten (vcam.rs)
    if std::env::args().any(|a| a == "--vcam-setup") {
        std::process::exit(notch_lib::vcam_setup());
    }
    notch_lib::run()
}
