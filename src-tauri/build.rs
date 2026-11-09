fn main() {
    // Mac: CoreAudio schwach linken — die Tap-Funktionen (Systemton, ab macOS 14.2) fehlen auf aelteren Systemen;
    // sonst startete die App dort gar nicht. mac/loopback.rs ruft sie erst ab 14.6 auf.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-Wl,-weak_framework,CoreAudio");
    }
    tauri_build::build()
}
