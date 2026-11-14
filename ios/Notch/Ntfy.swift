import BackgroundTasks
import SwiftUI
import UserNotifications

/// Mitteilungen vom PC. Der PC schickt sie weiter an ntfy.sh (Thema steht auf der Einrichtungsseite, dort bleibt
/// alles unveraendert); die App liest dasselbe Thema selbst mit, solange sie laeuft. Geschlossen kann sie niemand
/// wecken: das braucht den Apple-Push-Dienst, den nur ein App-Store-Eintrag hat. Bis dahin bleibt die ntfy-App richtig.
extension NotchModel {
    static let ntfySession: URLSession = {
        let c = URLSessionConfiguration.ephemeral
        c.timeoutIntervalForRequest = 90   // ntfy meldet sich alle 45 s, auch ohne Nachricht
        c.timeoutIntervalForResource = 24 * 3600
        return URLSession(configuration: c)
    }()

    /// Thema aus der Einrichtungsseite; nur Buchstaben, Ziffern, - und _ (sonst waere die Adresse manipulierbar)
    static func topic(_ page: String) -> String? {
        guard let t = firstGroup(#"<textarea id="topic" readonly>(.*?)</textarea>"#, in: page), validTopic(t) else { return nil }
        return t
    }

    nonisolated static func validTopic(_ t: String) -> Bool {
        !t.isEmpty && t.count <= 64 && t.allSatisfy { $0.isASCII && ($0.isLetter || $0.isNumber || $0 == "-" || $0 == "_") }
    }

    /// Id der letzten gesehenen Nachricht (auch fuer die Hintergrundpruefung), vorher: Startzeit
    var ntfySince: String {
        get { UserDefaults.standard.string(forKey: "ntfy.since") ?? "" }
        set { UserDefaults.standard.set(newValue, forKey: "ntfy.since") }
    }

    var ntfyOn: Bool {
        get { UserDefaults.standard.object(forKey: "ntfy.on") as? Bool ?? true }
        set {
            UserDefaults.standard.set(newValue, forKey: "ntfy.on")
            if newValue {
                UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound]) { _, _ in }
                startNtfy()
            } else {
                stopNtfy()
            }
            objectWillChange.send()
        }
    }

    func startNtfy() {
        ntfyTask?.cancel()
        ntfyTask = nil
        ntfyConnected = false
        guard ntfyOn, let topic = link?.ntfy else { return }
        if ntfySince.isEmpty { ntfySince = String(Int(Date().timeIntervalSince1970)) }
        ntfyTask = Task { await ntfyLoop(topic) }
    }

    func stopNtfy() {
        ntfyTask?.cancel()
        ntfyTask = nil
        ntfyConnected = false
    }

    private func ntfyLoop(_ topic: String) async {
        var wait = 2.0
        while !Task.isCancelled {
            do {
                guard let url = URL(string: "https://ntfy.sh/\(topic)/json?since=\(enc(ntfySince))") else { return }
                let (bytes, resp) = try await Self.ntfySession.bytes(from: url)
                guard (resp as? HTTPURLResponse)?.statusCode == 200 else { throw URLError(.badServerResponse) }
                ntfyConnected = true
                wait = 2
                for try await line in bytes.lines {
                    guard let d = line.data(using: .utf8),
                          let o = try? JSONSerialization.jsonObject(with: d) as? [String: Any],
                          o["event"] as? String == "message" else { continue }
                    if let id = o["id"] as? String { ntfySince = id }
                    await ntfyMessage(o["message"] as? String ?? "")
                }
            } catch {}
            ntfyConnected = false
            if Task.isCancelled { return }
            try? await Task.sleep(nanoseconds: UInt64(wait * 1_000_000_000))
            wait = min(wait * 2, 30)
        }
    }

    /// Der PC meldet „etwas liegt bereit“: gleich holen; im Hintergrund zusaetzlich eine Mitteilung
    private func ntfyMessage(_ text: String) async {
        if !appActive, !text.isEmpty {
            let c = UNMutableNotificationContent()
            c.title = "Notch"
            c.body = text
            c.sound = .default
            try? await UNUserNotificationCenter.current().add(UNNotificationRequest(identifier: UUID().uuidString, content: c, trigger: nil))
        }
        if working == nil { await fetch(quiet: true) }
    }
}

/// Abschnitt „Benachrichtigungen“ in der App
struct NotifySection: View {
    @EnvironmentObject var n: NotchModel
    @State private var live = LiveManager.shared.enabled

    var body: some View {
        Section {
            Toggle("Mitteilungen vom PC", isOn: Binding(get: { n.ntfyOn }, set: { n.ntfyOn = $0 }))
            if n.ntfyOn {
                if let t = n.link?.ntfy {
                    HStack {
                        Circle().fill(n.ntfyConnected ? Color.green : Color.orange).frame(width: 8, height: 8)
                        Text((n.ntfyConnected ? "Hört auf " : "Verbinde mit ") + t)
                            .font(.footnote).foregroundStyle(.secondary).lineLimit(1)
                        Spacer()
                        Button { UIPasteboard.general.string = t } label: { Image(systemName: "doc.on.doc") }
                            .buttonStyle(.borderless)
                    }
                } else {
                    Text("Am PC aus: Notch › Einstellungen › iPhone › Mitteilungen einschalten.")
                        .font(.footnote).foregroundStyle(.secondary)
                }
            }
            Toggle("Live-Aktivität", isOn: Binding(get: { live }, set: { live = $0; LiveManager.shared.enabled = $0 }))
        } header: {
            Text("Benachrichtigungen")
        } footer: {
            Text("Die App hört auf das ntfy-Thema des PCs, solange sie offen oder kurz im Hintergrund ist, und holt dann sofort ab. Ist sie ganz geschlossen, weckt iOS sie nicht (dafür braucht es den App Store); dafür die ntfy-App mit demselben Thema behalten. Die Live-Aktivität zeigt Senden, Holen und Webcam auf dem Sperrbildschirm.")
        }
    }
}

/// Bei geschlossener App schaut iOS gelegentlich (Minuten bis Stunden, nicht planbar) kurz herein; dann fragt die App
/// ntfy einmal ab und zeigt eine eigene Mitteilung. Abgeholt wird erst beim Oeffnen.
enum NtfyBackground {
    static let id = "de.nojo.notch.phone.refresh"

    static func register() {
        BGTaskScheduler.shared.register(forTaskWithIdentifier: id, using: nil) { task in
            schedule()
            let job = Task { task.setTaskCompleted(success: await check()) }
            task.expirationHandler = { job.cancel() }
        }
    }

    static func schedule() {
        let r = BGAppRefreshTaskRequest(identifier: id)
        r.earliestBeginDate = Date(timeIntervalSinceNow: 15 * 60)
        try? BGTaskScheduler.shared.submit(r)
    }

    private static func check() async -> Bool {
        let d = UserDefaults.standard
        guard d.object(forKey: "ntfy.on") as? Bool ?? true, let topic = PairingStore.load()?.ntfy, NotchModel.validTopic(topic) else { return true }
        var since = d.string(forKey: "ntfy.since") ?? ""
        if since.isEmpty { since = String(Int(Date().timeIntervalSince1970) - 3600) }
        guard let url = URL(string: "https://ntfy.sh/\(topic)/json?poll=1&since=\(enc(since))"),
              let (data, _) = try? await URLSession.shared.data(from: url) else { return false }
        for line in String(decoding: data, as: UTF8.self).split(whereSeparator: \.isNewline) {
            guard let o = try? JSONSerialization.jsonObject(with: Data(line.utf8)) as? [String: Any],
                  o["event"] as? String == "message" else { continue }
            if let id = o["id"] as? String { d.set(id, forKey: "ntfy.since") }
            let c = UNMutableNotificationContent()
            c.title = "Notch"
            c.body = o["message"] as? String ?? "Am PC liegt etwas bereit"
            c.sound = .default
            try? await UNUserNotificationCenter.current().add(UNNotificationRequest(identifier: UUID().uuidString, content: c, trigger: nil))
        }
        return true
    }
}
