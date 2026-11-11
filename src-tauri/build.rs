fn main() {
    // Mac: CoreAudio schwach linken — die Tap-Funktionen (Systemton, ab macOS 14.2) fehlen auf aelteren Systemen;
    // sonst startete die App dort gar nicht. mac/loopback.rs ruft sie erst ab 14.6 auf.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-Wl,-weak_framework,CoreAudio");
    }
    // Windows: die virtuelle Kamera (vcam/, laeuft im Windows-Frame-Server) wird als eigenes Paket gebaut und in
    // notch.exe eingebettet (vcam.rs legt sie beim Einschalten der iPhone-Kamera ab)
    println!("cargo:rerun-if-changed=vcam/src");
    println!("cargo:rerun-if-changed=vcam/Cargo.toml");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let out = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
        let target = out.join("vcam-target");
        let ok = std::process::Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
            .args(["build", "--release", "--manifest-path", "vcam/Cargo.toml", "--target-dir"])
            .arg(&target)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        assert!(ok, "vcam (virtuelle Kamera) konnte nicht gebaut werden");
        std::fs::copy(target.join("release").join("notch_vcam.dll"), out.join("notch_vcam.dll")).expect("notch_vcam.dll");
        // Kennung fuer den Dateinamen im Programme-Ordner: FNV-1a der DLL, damit niemand eine Versionsnummer pflegen muss
        let dll = std::fs::read(out.join("notch_vcam.dll")).expect("notch_vcam.dll lesen");
        let hash = dll.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x100_0000_01b3));
        std::fs::write(out.join("vcam_rev.txt"), format!("{hash:016x}")).expect("vcam_rev.txt");
    }
    tauri_build::build()
}
