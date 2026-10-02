//! Selbst-Update ueber GitHub Releases. Prueft 20 s nach dem Start und dann alle 6 h
//! (oder per Tray "Nach Updates suchen"). Neue Version -> Activity "notch:update" mit
//! Installieren / Spaeter. Installiert wird nur, was mit dem Schluessel aus tauri.conf.json
//! (plugins.updater.pubkey) signiert ist; das erledigt das Updater-Plugin.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use base64::Engine;
use tauri::AppHandle;
use tauri_plugin_updater::UpdaterExt;

use crate::activities::{self, Action, Activity};

pub const ID: &str = "notch:update";
const RELEASES: &str = "https://github.com/NojoMcDybo/notch/releases/latest";

/// laeuft gerade ein Download/Installation -> keine weiteren Pruefungen
static BUSY: AtomicBool = AtomicBool::new(false);

fn icon() -> String {
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#0a84ff" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="9"/><path d="M12 7.5v8M8.5 12l3.5 3.5 3.5-3.5"/></svg>"##;
    format!("data:image/svg+xml;base64,{}", base64::engine::general_purpose::STANDARD.encode(svg))
}

fn a(id: &str, label: &str, icon: &str) -> Action {
    Action { id: id.into(), label: label.into(), icon: Some(icon.into()), ..Default::default() }
}

fn show(app: &AppHandle, title: &str, subtitle: Option<String>, actions: Vec<Action>, progress: Option<f64>, ttl: Option<u64>) {
    activities::upsert(
        app,
        Activity {
            id: ID.into(),
            app: "Notch".into(),
            title: title.into(),
            subtitle,
            icon: Some(icon()),
            color: Some("#0a84ff".into()),
            progress,
            ttl,
            priority: 1,
            actions,
            ..Default::default()
        },
    );
}

pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(20));
        loop {
            tauri::async_runtime::block_on(check(&app, false));
            std::thread::sleep(Duration::from_secs(6 * 3600));
        }
    });
}

/// `manual`: auch "ist aktuell" bzw. Fehler kurz anzeigen (Tray-Menue).
pub async fn check(app: &AppHandle, manual: bool) {
    if BUSY.load(Ordering::SeqCst) {
        return;
    }
    let res = match app.updater() {
        Ok(u) => u.check().await.map_err(|e| e.to_string()),
        Err(e) => Err(e.to_string()),
    };
    match res {
        Ok(Some(u)) => show(
            app,
            "Update verfügbar",
            Some(format!("Notch {} (installiert: {})", u.version, u.current_version)),
            vec![a("install", "Installieren", "check"), a("later", "Später", "close")],
            None,
            None,
        ),
        Ok(None) if manual => show(app, "Notch ist aktuell", Some(format!("Version {}", app.package_info().version)), vec![], None, Some(5)),
        Ok(None) => {
            activities::remove(app, ID);
        }
        // offline o. Ae.: beim automatischen Pruefen still bleiben
        Err(e) if manual => show(app, "Update-Prüfung fehlgeschlagen", Some(e), vec![a("open", "Releases", "open")], None, Some(30)),
        Err(e) => eprintln!("[notch] Update-Pruefung: {e}"),
    }
}

/// Knopf in der Update-Zeile.
pub fn action(app: &AppHandle, action: &str) {
    match action {
        "install" | "retry" => {
            if BUSY.swap(true, Ordering::SeqCst) {
                return;
            }
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = install(&app).await {
                    BUSY.store(false, Ordering::SeqCst);
                    show(&app, "Update fehlgeschlagen", Some(e), vec![a("retry", "Nochmal", "restart"), a("open", "Releases", "open")], None, None);
                }
            });
        }
        "open" => {
            let _ = crate::open::open_target(app, RELEASES);
        }
        _ => {
            activities::remove(app, ID);
        }
    }
}

async fn install(app: &AppHandle) -> Result<(), String> {
    show(app, "Update wird geladen …", None, vec![], Some(-1.0), None);
    let update = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Kein Update mehr verfügbar")?;
    update.download_and_install(|_, _| {}, || {}).await.map_err(|e| e.to_string())?;
    // Windows: der Installer beendet Notch selbst und startet sie danach neu
    app.restart();
}
