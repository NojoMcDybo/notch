import ReplayKit
import SwiftUI

/// Unsichtbarer System-Knopf „Bildschirmaufnahme“; `trigger` antippt ihn von aussen, weil sich der Knopf nicht gestalten laesst.
/// Die Erweiterung wird im Paket gesucht, denn Signierwerkzeuge haengen der Kennung etwas an.
struct BroadcastPicker: UIViewRepresentable {
    let trigger: Int

    final class Coordinator { var last = 0 }

    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeUIView(context: Context) -> RPSystemBroadcastPickerView {
        let v = RPSystemBroadcastPickerView(frame: CGRect(x: 0, y: 0, width: 44, height: 44))
        v.preferredExtension = Self.extensionID
        v.showsMicrophoneButton = false
        return v
    }

    func updateUIView(_ v: RPSystemBroadcastPickerView, context: Context) {
        guard context.coordinator.last != trigger else { return }
        context.coordinator.last = trigger
        v.subviews.compactMap { $0 as? UIButton }.first?.sendActions(for: .touchUpInside)
    }

    static var extensionID: String? {
        guard let dir = Bundle.main.builtInPlugInsURL,
              let items = try? FileManager.default.contentsOfDirectory(at: dir, includingPropertiesForKeys: nil) else { return nil }
        return items.compactMap { Bundle(url: $0) }
            .first { ($0.infoDictionary?["NSExtension"] as? [String: Any])?["NSExtensionPointIdentifier"] as? String == "com.apple.broadcast-services-upload" }?
            .bundleIdentifier
    }
}

/// Bildschirm des iPhones samt Ton an den PC streamen
struct ScreenView: View {
    @EnvironmentObject var n: NotchModel
    @State private var trigger = 0
    @State private var live = UIScreen.main.isCaptured

    var body: some View {
        List {
            Section {
                Button {
                    Task {
                        await n.refreshCam()   // vielleicht gerade erst am PC eingeschaltet
                        if n.camURL != nil { trigger += 1 }
                    }
                } label: {
                    Label(live ? "Übertragung beenden" : "Übertragung starten", systemImage: live ? "stop.circle.fill" : "record.circle")
                        .foregroundStyle(live ? Color.red : Color.accentColor)
                }
                .background(BroadcastPicker(trigger: trigger).frame(width: 1, height: 1).opacity(0.02).allowsHitTesting(false))
            } header: {
                Text(live ? "Läuft" : "Bildschirm teilen")
            } footer: {
                if n.camURL == nil {
                    Text("Am PC ist die Kamera aus: Notch › Einstellungen › iPhone › Kamera einschalten. Der Bildschirm läuft über dieselbe virtuelle Kamera „Notch iPhone“.")
                } else if let id = BroadcastPicker.extensionID, !id.isEmpty {
                    Text("Im Fenster „Notch“ wählen und „Übertragung starten“ tippen. Beenden geht hier, über die rote Anzeige oben oder das Kontrollzentrum.")
                } else {
                    Text("Die Bildschirm-Erweiterung fehlt in der installierten App (iLoader hat sie nicht mitinstalliert).")
                }
            }
            Section {
                Label("Am PC: Kamera „Notch iPhone“ (Teams, OBS, Chrome …)", systemImage: "video")
                Label("Ton der Apps (Musik, Spiele, Videos) kommt am Standardausgang des PCs", systemImage: "speaker.wave.2")
                Label("Hochformat erscheint schmal mit schwarzen Rändern, quer füllt das Bild", systemImage: "iphone.landscape")
            } header: {
                Text("So funktioniert es")
            } footer: {
                Text("Nicht übertragen werden Mikrofon, Telefonate und kopiergeschützte Videos (Netflix und Co. zeigen schwarz). Der Ton hängt wenige Zehntelsekunden hinter dem Bild.")
            }
        }
        .navigationTitle("Bildschirm")
        .navigationBarTitleDisplayMode(.inline)
        .task { await n.refreshCam() }
        .onReceive(NotificationCenter.default.publisher(for: UIScreen.capturedDidChangeNotification)) { _ in
            live = UIScreen.main.isCaptured
        }
    }
}
