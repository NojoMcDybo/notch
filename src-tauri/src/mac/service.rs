//! Finder: Rechtsklick › Dienste (bzw. Schnelle Aktionen) › „Fürs iPhone bereitlegen“ — das Mac-Gegenstueck zu
//! Explorer › Senden an › iPhone. Der Eintrag steht in der Info.plist (NSServices, Nachricht `sendToIphone`);
//! macOS ruft dann diese Methode mit den markierten Dateien auf und die Notch legt sie wie beim Senden bereit.

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSPasteboard};
use objc2_foundation::NSString;
use tauri::AppHandle;

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "NotchServices"]
    #[thread_kind = MainThreadOnly]
    #[ivars = AppHandle]
    struct Services;

    impl Services {
        #[unsafe(method(sendToIphone:userData:error:))]
        fn send_to_iphone(&self, pb: &NSPasteboard, _data: Option<&NSString>, _err: *mut *mut AnyObject) {
            let paths = crate::clipboard_sys::file_paths(pb);
            if !paths.is_empty() {
                if let Err(e) = crate::iphone::offer(self.ivars(), paths) {
                    eprintln!("[notch] Finder-Dienst: {e}");
                }
            }
        }
    }
);

/// Beim Start auf dem Hauptfaden: Dienst anmelden (und macOS die Liste neu einlesen lassen)
pub fn register(app: &AppHandle) {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let this = mtm.alloc::<Services>().set_ivars(app.clone());
    let s: Retained<Services> = unsafe { msg_send![super(this), init] };
    unsafe { NSApplication::sharedApplication(mtm).setServicesProvider(Some(&s)) };
    // der Anbieter muss so lange leben wie die App
    std::mem::forget(s);
    objc2_app_kit::NSUpdateDynamicServices();
}
