import CoreTransferable
import Photos
import PhotosUI
import SwiftUI
import UniformTypeIdentifiers

/// Aus dem Fotos-Dialog: Datei mit Originalnamen in den Temp-Ordner kopieren (grosse Videos nie in den Speicher laden)
struct PickedFile: Transferable {
    let url: URL

    static var transferRepresentation: some TransferRepresentation {
        FileRepresentation(importedContentType: .movie) { try PickedFile(url: copy($0.file)) }
        FileRepresentation(importedContentType: .image) { try PickedFile(url: copy($0.file)) }
    }

    private static func copy(_ src: URL) throws -> URL {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let dest = dir.appendingPathComponent(src.lastPathComponent)
        try FileManager.default.copyItem(at: src, to: dest)
        return dest
    }
}

/// Eintrag im Verlauf; bleibt nach dem Schliessen der App erhalten
struct HistoryItem: Codable, Identifiable {
    var id = UUID()
    var date = Date()
    var text: String
    var ok = true
    /// Gescheiterter Versand: Pfad der aufgehobenen Datei im Ordner „Outbox“ (`<Ordner>/<Name>`), dann gibt es „Erneut senden“
    var retry: String?
}

@MainActor
final class NotchModel: ObservableObject {
    @Published private(set) var link: Pairing?
    @Published private(set) var online = false
    /// laufende Arbeit, z. B. „Sende 2 von 3 …“
    @Published private(set) var working: String?
    /// 0…1 der laufenden Datei
    @Published private(set) var progress: Double?
    @Published private(set) var history: [HistoryItem] = []
    /// Kamera-Adresse; leer, solange die Kamera am PC aus ist
    @Published private(set) var camURL: URL?
    @Published private(set) var lastCheck: Date?

    private var base: String?
    private var polling = false
    /// Warum die letzte Suche scheiterte (Fehlercode je Adresse), zur Fehlersuche
    private var detail = ""

    private let outbox: URL = {
        let dir = (try? FileManager.default.url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)) ?? FileManager.default.temporaryDirectory
        return dir.appendingPathComponent("Outbox", isDirectory: true)
    }()

    init() {
        // offen: alle 4 Sekunden schauen, ob am PC etwas bereitliegt (im Hintergrund haelt iOS den Timer an)
        Timer.scheduledTimer(withTimeInterval: 4, repeats: true) { [weak self] _ in
            Task { @MainActor in await self?.poll() }
        }
        link = PairingStore.load()
        PairingStore.save(link)   // auch in die gemeinsame Gruppe, falls die Erweiterung erst jetzt dazukam
        if let d = UserDefaults.standard.data(forKey: "history"), let h = try? JSONDecoder().decode([HistoryItem].self, from: d) {
            history = h
        }
    }

    // MARK: Kopplung

    /// QR-Inhalt oder eingefuegte Adresse: `http://<ip>:53317/k/<schluessel>/`
    func pair(_ raw: String) async {
        guard let addr = firstGroup(#"(https?://[^\s"'<>]+?/k/[0-9A-Za-z]+)"#, in: raw),
              let token = addr.components(separatedBy: "/k/").last else {
            note(NotchError.badAddress.localizedDescription, ok: false)
            return
        }
        working = "Verbinde …"
        defer { working = nil }
        var bases = [addr]
        // Die Einrichtungsseite nennt den Rechnernamen; ihr Abruf meldet das iPhone dem PC zugleich als gekoppelt.
        if let page = try? await NotchNet.text(from: NotchNet.endpoint(addr, ""), timeout: 6),
           let host = firstGroup(#"<textarea id="hostbase" readonly>(.*?)</textarea>"#, in: page), !host.isEmpty {
            bases.insert(host, at: 0)
        }
        link = Pairing(token: token, bases: bases, pc: "PC")
        base = nil
        PairingStore.save(link)
        if await resolve() {
            note("Gekoppelt mit \(link?.pc ?? "PC")")
        } else {
            note("Gekoppelt, aber der PC antwortet nicht: \(detail)", ok: false)
        }
    }

    func unpair() {
        link = nil
        base = nil
        online = false
        camURL = nil
        PairingStore.save(nil)
    }

    // MARK: Verbindung

    /// Erste erreichbare Adresse merken; meldet dem PC nebenbei „iPhone da“ mit Akkustand
    @discardableResult
    func resolve() async -> Bool {
        guard let l = link else { return false }
        let r = await NotchNet.resolve(l, preferred: base)
        detail = r.detail
        guard let b = r.base else {
            base = nil
            online = false
            return false
        }
        base = b
        online = true
        if let pc = r.pc, pc != l.pc {
            link?.pc = pc
            PairingStore.save(link)
        }
        return true
    }

    /// Wird die gemerkte Adresse ungueltig (neue IP), einmal neu suchen und wiederholen
    private func perform<T>(_ op: (String) async throws -> T) async throws -> T {
        if base == nil { await resolve() }
        guard let b = base else { throw NotchError.offline(detail) }
        do {
            return try await op(b)
        } catch where NotchNet.isUnreachable(error) {
            base = nil
            guard await resolve(), let b2 = base else { throw NotchError.offline(detail) }
            return try await op(b2)
        }
    }

    private func setProgress(_ p: Double) { progress = p }

    // MARK: iPhone → PC

    func send(_ items: [PhotosPickerItem]) async {
        var files: [URL] = []
        for it in items {
            if let f = try? await it.loadTransferable(type: PickedFile.self) { files.append(f.url) }
        }
        await upload(files, scoped: false)
        files.forEach { try? FileManager.default.removeItem(at: $0.deletingLastPathComponent()) }
    }

    func send(files: [URL]) async { await upload(files, scoped: true) }

    /// Fuer den Versand angelegte Temp-Dateien: nach dem Senden samt Ordner loeschen
    func sendTemporary(_ files: [URL]) async {
        await upload(files, scoped: false)
        files.forEach { try? FileManager.default.removeItem(at: $0.deletingLastPathComponent()) }
    }

    private func upload(_ files: [URL], scoped: Bool) async {
        var sent = 0
        for (i, f) in files.enumerated() {
            working = files.count > 1 ? "Sende \(i + 1) von \(files.count) …" : "Sende …"
            progress = 0
            let ok = scoped && f.startAccessingSecurityScopedResource()
            defer { if ok { f.stopAccessingSecurityScopedResource() } }
            let name = f.lastPathComponent
            do {
                try await push(f, name: name)
                sent += 1
            } catch {
                note("\(name): \(error.localizedDescription)", ok: false, retry: keep(f, name: name))
            }
        }
        working = nil
        progress = nil
        if sent > 0 { note(sent == 1 ? "1 Datei an den PC gesendet" : "\(sent) Dateien an den PC gesendet") }
    }

    private func push(_ file: URL, name: String) async throws {
        try await perform { b in
            try await NotchNet.upload(file: file, name: name, base: b) { [weak self] p in
                Task { @MainActor in self?.setProgress(p) }
            }
        }
    }

    /// Gescheiterte Datei aufheben, damit „Erneut senden“ sie noch hat (die Quelle ist dann meist schon weg)
    private func keep(_ file: URL, name: String) -> String? {
        let dir = UUID().uuidString
        let target = outbox.appendingPathComponent(dir, isDirectory: true)
        do {
            try FileManager.default.createDirectory(at: target, withIntermediateDirectories: true)
            try FileManager.default.copyItem(at: file, to: target.appendingPathComponent(name))
            return "\(dir)/\(name)"
        } catch {
            return nil
        }
    }

    /// „Erneut senden“ am Verlaufseintrag
    func retry(_ item: HistoryItem) async {
        guard let rel = item.retry, let i = history.firstIndex(where: { $0.id == item.id }) else { return }
        let file = outbox.appendingPathComponent(rel)
        guard FileManager.default.fileExists(atPath: file.path) else {
            history[i].retry = nil
            history[i].text += " (Datei nicht mehr da)"
            saveHistory()
            return
        }
        working = "Sende …"
        progress = 0
        defer { working = nil; progress = nil }
        do {
            try await push(file, name: file.lastPathComponent)
            try? FileManager.default.removeItem(at: file.deletingLastPathComponent())
            if let j = history.firstIndex(where: { $0.id == item.id }) {
                history[j].ok = true
                history[j].retry = nil
                history[j].text = "Nachträglich gesendet: \(file.lastPathComponent)"
                saveHistory()
            }
        } catch {
            note("\(file.lastPathComponent): \(error.localizedDescription)", ok: false)
        }
    }

    /// `clip`: landet in der Zwischenablage des PCs; sonst als Text/Link in der Notch (ein Link wird dort geoeffnet)
    func sendText(_ s: String, clip: Bool) async {
        let t = s.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !t.isEmpty else { note("Nichts zu senden"); return }
        working = "Sende …"
        defer { working = nil }
        do {
            try await perform { b in try await NotchNet.upload(text: t, clip: clip, base: b) }
            note(clip ? "Zwischenablage an den PC gesendet" : "Text an den PC gesendet")
        } catch {
            note(error.localizedDescription, ok: false)
        }
    }

    // MARK: PC → iPhone

    /// Alles abholen, was am PC bereitliegt. `quiet`: beim Oeffnen der App, ohne Meldung wenn nichts da ist
    func fetch(quiet: Bool = false) async {
        do {
            let list = try await perform { b in try await NotchNet.text(from: NotchNet.endpoint(b, "abholen")) }
            let lines = list.split(whereSeparator: \.isNewline).map(String.init)
            guard lines.first == "abholen" else {
                if !quiet { note("Am PC liegt nichts bereit") }
                return
            }
            var got = 0
            for (i, l) in lines.dropFirst().enumerated() {
                working = lines.count > 2 ? "Hole \(i + 1) von \(lines.count - 1) …" : "Hole …"
                progress = 0
                guard let u = URL(string: l) else { continue }
                do { try await download(u); got += 1 } catch { note(error.localizedDescription, ok: false) }
            }
            let t = try await perform { b in try await NotchNet.text(from: NotchNet.endpoint(b, "text")) }
            if !t.isEmpty {
                UIPasteboard.general.string = t
                note("Text vom PC in der Zwischenablage")
            }
            if got > 0 { note(got == 1 ? "1 Datei vom PC geholt" : "\(got) Dateien vom PC geholt") }
        } catch {
            if !quiet { note(error.localizedDescription, ok: false) }
        }
        working = nil
        progress = nil
    }

    /// Fotos und Videos in die Fotos-App, alles andere nach Dateien › Notch
    private func download(_ u: URL) async throws {
        let reporter = ProgressReporter { [weak self] p in Task { @MainActor in self?.setProgress(p) } }
        let (tmp, resp) = try await NotchNet.session.download(from: u, delegate: reporter)
        withExtendedLifetime(reporter) {}
        try NotchNet.check(resp)
        let name = Self.filename(resp as? HTTPURLResponse) ?? "Notch-\(Int(Date().timeIntervalSince1970))"
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let file = dir.appendingPathComponent(name)
        try FileManager.default.moveItem(at: tmp, to: file)
        defer { try? FileManager.default.removeItem(at: dir) }

        let kind = u.lastPathComponent   // foto | video | datei, so nennt es der PC
        var allowed = false
        if kind == "foto" || kind == "video" {
            let st = await PHPhotoLibrary.requestAuthorization(for: .addOnly)
            allowed = st == .authorized || st == .limited
        }
        if allowed {
            do {
                try await PHPhotoLibrary.shared().performChanges {
                    if kind == "video" { _ = PHAssetChangeRequest.creationRequestForAssetFromVideo(atFileURL: file) }
                    else { _ = PHAssetChangeRequest.creationRequestForAssetFromImage(atFileURL: file) }
                }
                return
            } catch {
                // Format, das Fotos nicht mag: lieber als Datei behalten als verlieren
            }
        }
        let docs = try FileManager.default.url(for: .documentDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
        var dest = docs.appendingPathComponent(name)
        var n = 1
        while FileManager.default.fileExists(atPath: dest.path) {
            dest = docs.appendingPathComponent("\(n)-\(name)")
            n += 1
        }
        try FileManager.default.copyItem(at: file, to: dest)
    }

    /// `attachment; filename*=UTF-8''IMG%201.heic`; der Name darf keinen Ordner enthalten
    private static func filename(_ r: HTTPURLResponse?) -> String? {
        guard let d = r?.value(forHTTPHeaderField: "Content-Disposition"),
              let raw = firstGroup(#"filename\*=UTF-8''([^;]+)"#, in: d),
              let name = raw.removingPercentEncoding?.split(whereSeparator: { $0 == "/" || $0 == "\\" }).last,
              !name.isEmpty, name != ".." else { return nil }
        return String(name)
    }

    // MARK: Kamera

    /// Die Einrichtungsseite nennt die Kamera-Adresse (Port und Schluessel); leer = am PC aus
    func refreshCam() async {
        guard let l = link, let b = base ?? l.bases.first else { camURL = nil; return }
        let page = try? await NotchNet.text(from: NotchNet.endpoint(b, ""), timeout: 6)
        let href = page.flatMap { firstGroup(#"id="camrow" href="([^"]*)""#, in: $0) } ?? ""
        camURL = href.isEmpty ? nil : URL(string: href + "frame")
    }

    // MARK: Verlauf

    func note(_ s: String, ok: Bool = true, retry: String? = nil) {
        history.insert(HistoryItem(text: s, ok: ok, retry: retry), at: 0)
        if history.count > 40 { dropOld() }
        saveHistory()
    }

    private func dropOld() {
        for h in history.suffix(from: 40) {
            if let r = h.retry { try? FileManager.default.removeItem(at: outbox.appendingPathComponent(r).deletingLastPathComponent()) }
        }
        history.removeLast(history.count - 40)
    }

    func clearHistory() {
        try? FileManager.default.removeItem(at: outbox)
        history = []
        saveHistory()
    }

    private func saveHistory() {
        if let d = try? JSONEncoder().encode(history) { UserDefaults.standard.set(d, forKey: "history") }
    }

    /// Knopf „Prüfen“: Ergebnis im Verlauf zeigen
    func check() async {
        working = "Prüfe …"
        let ok = await resolve()
        working = nil
        note(ok ? "Verbunden mit \(link?.pc ?? "PC")" : NotchError.offline(detail).localizedDescription, ok: ok)
    }

    /// Im Vordergrund alle paar Sekunden: bereitgelegte Dateien holen, ohne laufende Arbeit zu stoeren
    func poll() async {
        guard link != nil, working == nil, !polling else { return }
        polling = true
        defer { polling = false }
        if base == nil { await resolve() }
        if online { await fetch(quiet: true) }
        lastCheck = Date()
    }

    /// App wird geoeffnet: Verbindung pruefen (meldet das iPhone dem PC) und Bereitliegendes holen
    func onActive() async {
        guard link != nil else { return }
        if await resolve() { await fetch(quiet: true) }
    }
}
