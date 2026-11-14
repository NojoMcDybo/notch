import ActivityKit
import Foundation

/// Live-Aktivitaet der Notch auf dem Sperrbildschirm. Gehoert zu beiden Programmteilen: die App startet und
/// aktualisiert sie (LiveManager), die Widget-Erweiterung zeichnet sie (NotchWidget).
struct NotchActivityAttributes: ActivityAttributes {
    struct ContentState: Codable, Hashable {
        /// "send" | "fetch" | "camera" | "done" | "error"
        var kind: String
        var title: String
        var detail: String
        /// 0…1, nil = kein Balken
        var progress: Double?
    }

    var pc: String
}
