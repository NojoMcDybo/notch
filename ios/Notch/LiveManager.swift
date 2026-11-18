import ActivityKit
import Foundation

/// Live-Aktivitaet auf dem Sperrbildschirm: zeigt Senden, Holen und Webcam und meldet das Ergebnis.
/// Ohne Push-Dienst (App sideloaded) kann nur die laufende App sie aendern; das reicht fuer alles hier.
@MainActor
final class LiveManager: ObservableObject {
    static let shared = LiveManager()
    typealias State = NotchActivityAttributes.ContentState

    /// Letztes Ergebnis von ActivityKit, zur Fehlersuche in den Einstellungen
    @Published var status = ""
    private var activity: Activity<NotchActivityAttributes>?
    private var pc = "PC"
    private var transfer: State?
    private var cameraFps: Int?
    private var lastPush = Date.distantPast
    private var holdUntil = Date.distantPast
    private var lastKey = ""
    private var transferSince: Date?

    /// Schalter in der App; ohne Eintrag an
    var enabled: Bool {
        get { UserDefaults.standard.object(forKey: "live") as? Bool ?? true }
        set {
            UserDefaults.standard.set(newValue, forKey: "live")
            if !newValue { stopAll() }
        }
    }

    /// Knopf „Testen“: 15 Sekunden lang eine Beispiel-Aktivitaet
    func demo() {
        guard ActivityAuthorizationInfo().areActivitiesEnabled else {
            status = "Live-Aktivitäten sind für Notch in den iOS-Einstellungen aus"
            return
        }
        let content = ActivityContent(state: State(kind: "send", title: "Test", detail: "Beispiel", progress: 0.4), staleDate: nil)
        do {
            let a = try Activity<NotchActivityAttributes>.request(attributes: NotchActivityAttributes(pc: pc), content: content, pushType: nil)
            let ext = (Bundle.main.builtInPlugInsPath.flatMap { try? FileManager.default.contentsOfDirectory(atPath: $0) } ?? []).joined(separator: ", ")
            status = "Test gestartet. Erweiterungen im Paket: \(ext.isEmpty ? "keine" : ext)"
            Task {
                for await st in a.activityStateUpdates { status += " · Zustand: \(st)" }
            }
            Task {
                try? await Task.sleep(nanoseconds: 15_000_000_000)
                await a.end(nil, dismissalPolicy: .immediate)
            }
        } catch {
            status = "Start gescheitert: \(error)"
        }
    }

    func setPC(_ name: String) { pc = name }

    /// Beim Start: Aktivitaeten eines frueheren Laufs beenden
    func endOrphans() {
        for a in Activity<NotchActivityAttributes>.activities { Task { await a.end(nil, dismissalPolicy: .immediate) } }
    }

    // MARK: Zustaende

    func transfer(kind: String, title: String, progress: Double?) {
        if transfer == nil {
            // kurze Vorgaenge (ein Text) sollen keine Aktivitaet aufblitzen lassen: erst nach 1,2 s zeigen
            transferSince = Date()
            Task {
                try? await Task.sleep(nanoseconds: 1_300_000_000)
                refresh()
            }
        }
        transfer = State(kind: kind, title: title, detail: "", progress: progress)
        refresh()
    }

    /// Uebertragung vorbei: Ergebnis kurz zeigen (mit Hinweis-Banner), dann Kamera oder Ende
    func finish(text: String, ok: Bool) {
        guard transfer != nil else { return }
        transfer = nil
        guard activity != nil else { refresh(force: true); return }
        holdUntil = Date().addingTimeInterval(6)
        show(State(kind: ok ? "done" : "error", title: ok ? "Fertig" : "Fehlgeschlagen", detail: text, progress: nil), alert: text, force: true)
        Task {
            try? await Task.sleep(nanoseconds: 6_200_000_000)
            refresh(force: true)
        }
    }

    func camera(fps: Int?) {
        cameraFps = fps
        refresh()
    }

    private func refresh(force: Bool = false) {
        guard Date() >= holdUntil else { return }
        if let t = transfer {
            if activity == nil, let s = transferSince, Date().timeIntervalSince(s) < 1.2 { return }
            show(t, alert: nil, force: force)
        } else if let f = cameraFps {
            show(State(kind: "camera", title: "Webcam läuft", detail: f > 0 ? "\(f) Bilder pro Sekunde" : "Keine Verbindung", progress: nil), alert: nil, force: force)
        } else {
            stopAll()
        }
    }

    // MARK: ActivityKit

    private func show(_ s: State, alert: String?, force: Bool) {
        guard enabled else { return }
        guard ActivityAuthorizationInfo().areActivitiesEnabled else {
            status = "Live-Aktivitäten sind für Notch in den iOS-Einstellungen aus"
            return
        }
        // Fortschritt hoechstens alle 0,8 s; Wechsel der Art oder Texte sofort
        let key = s.kind + s.title + s.detail
        if !force, key == lastKey, Date().timeIntervalSince(lastPush) < 0.8 { return }
        lastKey = key
        lastPush = Date()
        let content = ActivityContent(state: s, staleDate: nil)
        if let a = activity {
            Task {
                if let alert {
                    await a.update(content, alertConfiguration: AlertConfiguration(title: "Notch", body: LocalizedStringResource(stringLiteral: alert), sound: .default))
                } else {
                    await a.update(content)
                }
            }
        } else {
            do {
                activity = try Activity<NotchActivityAttributes>.request(attributes: NotchActivityAttributes(pc: pc), content: content, pushType: nil)
                status = "Gestartet (\(s.kind))"
            } catch {
                status = "Start gescheitert: \(error)"
            }
        }
    }

    private func stopAll() {
        let a = activity
        activity = nil
        lastKey = ""
        transfer = nil
        for x in Activity<NotchActivityAttributes>.activities where x.id == a?.id || a == nil {
            Task { await x.end(nil, dismissalPolicy: .immediate) }
        }
    }
}
