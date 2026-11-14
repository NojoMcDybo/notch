import PhotosUI
import SwiftUI
import UniformTypeIdentifiers

@main
struct NotchApp: App {
    @StateObject private var notch = NotchModel()
    @Environment(\.scenePhase) private var phase

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environmentObject(notch)
                // offen: beim Start und alle paar Sekunden schauen, ob am PC etwas bereitliegt
                .task(id: phase) {
                    guard phase == .active else { return }
                    await notch.onActive()
                    while !Task.isCancelled {
                        try? await Task.sleep(nanoseconds: 4_000_000_000)
                        await notch.poll()
                    }
                }
        }
    }
}

struct ContentView: View {
    @EnvironmentObject var n: NotchModel
    @State private var scanning = false
    @State private var picks: [PhotosPickerItem] = []
    @State private var importing = false
    @State private var text = ""

    var body: some View {
        NavigationStack {
            List {
                if n.link == nil { pairSection } else { connectedSections }
            }
            .navigationTitle("Notch")
            .sheet(isPresented: $scanning) {
                ZStack(alignment: .topTrailing) {
                    QRScanner { code in
                        scanning = false
                        Task { await n.pair(code) }
                    }
                    .ignoresSafeArea()
                    Button("Abbrechen") { scanning = false }
                        .padding(12)
                        .background(.ultraThinMaterial, in: Capsule())
                        .padding()
                }
            }
            .fileImporter(isPresented: $importing, allowedContentTypes: [.item], allowsMultipleSelection: true) { r in
                if case .success(let urls) = r { Task { await n.send(files: urls) } }
            }
            .onChange(of: picks) { items in
                guard !items.isEmpty else { return }
                picks = []
                Task { await n.send(items) }
            }
        }
    }

    // MARK: Noch nicht gekoppelt

    @ViewBuilder private var pairSection: some View {
        Section {
            Button { scanning = true } label: { Label("QR-Code scannen", systemImage: "qrcode.viewfinder") }
            Button {
                if let s = UIPasteboard.general.string { Task { await n.pair(s) } }
            } label: { Label("Adresse aus Zwischenablage", systemImage: "doc.on.clipboard") }
        } header: {
            Text("Mit dem PC koppeln")
        } footer: {
            Text("Am PC: Notch › Einstellungen › iPhone › Kurzbefehl. Den QR-Code scannen oder die Adresse kopieren. PC und iPhone im selben WLAN.")
        }
        historySection
    }

    // MARK: Gekoppelt

    @ViewBuilder private var connectedSections: some View {
        Section {
            HStack {
                Circle().fill(n.online ? Color.green : Color.orange).frame(width: 10, height: 10)
                VStack(alignment: .leading) {
                    Text(n.link?.pc ?? "PC").font(.headline)
                    Text(n.online ? "Verbunden" : "Nicht erreichbar").font(.caption).foregroundStyle(.secondary)
                }
                Spacer()
                if let w = n.working {
                    HStack(spacing: 6) { ProgressView(); Text(w).font(.caption).foregroundStyle(.secondary) }
                } else {
                    Button("Prüfen") { Task { await n.check() } }.buttonStyle(.borderless)
                }
            }
        }

        Section("An den PC senden") {
            PhotosPicker(selection: $picks, matching: .any(of: [.images, .videos])) {
                Label("Fotos und Videos", systemImage: "photo.on.rectangle")
            }
            Button { importing = true } label: { Label("Dateien", systemImage: "folder") }
            Button { Task { await n.sendText(UIPasteboard.general.string ?? "", clip: true) } } label: {
                Label("Zwischenablage senden", systemImage: "doc.on.clipboard")
            }
            HStack {
                TextField("Text oder Link", text: $text)
                Button("Senden") {
                    let t = text
                    text = ""
                    Task { await n.sendText(t, clip: false) }
                }
                .buttonStyle(.borderless)
                .disabled(text.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
        .disabled(n.working != nil)

        Section {
            Button { Task { await n.fetch() } } label: { Label("Vom PC abholen", systemImage: "arrow.down.circle") }
                .disabled(n.working != nil)
        } header: {
            Text("Vom PC")
        } footer: {
            Text("Was du am PC auf „iPhone“ ablegst, holt die App beim Öffnen von selbst. Fotos und Videos landen in Fotos, anderes in Dateien › Notch.")
        }

        Section {
            NavigationLink { CameraView() } label: { Label("iPhone als Webcam", systemImage: "video") }
        }

        historySection

        Section {
            Button("Kopplung lösen", role: .destructive) { n.unpair() }
        }
    }

    @ViewBuilder private var historySection: some View {
        if !n.history.isEmpty {
            Section("Verlauf") {
                ForEach(Array(n.history.enumerated()), id: \.offset) { _, line in
                    Text(line).font(.callout)
                }
            }
        }
    }
}
