import CoreTransferable
import Photos
import PhotosUI
import SwiftUI
import UniformTypeIdentifiers

/// Kopplung mit dem PC. Der Schluessel steckt in der Adresse (`…/k/<schluessel>`), wie beim Kurzbefehl.
struct Pairing: Codable, Equatable {
    var token: String
    /// `http://<PC>.local:53317/k/<s>` (uebersteht eine neue IP) und `http://<IP>:53317/k/<s>`
    var bases: [String]
    var pc: String
}

enum NotchError: LocalizedError {
    case offline(String), status(Int, String), badAddress

    var errorDescription: String? {
        switch self {
        case .offline(let d): return "PC nicht erreichbar" + (d.isEmpty ? "" : " (\(d))")
        case .status(let c, let m): return m.isEmpty ? "PC antwortet mit \(c)" : m
        case .badAddress: return "Das ist keine Notch-Adresse"
        }
    }
}

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

/// Prozent-Kodierung fuer Werte in der Adresse; `+` und `&` muessen mit, sonst liest der PC sie falsch
private func enc(_ s: String) -> String {
    s.addingPercentEncoding(withAllowedCharacters: CharacterSet(charactersIn: "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-._~")) ?? ""
}

private func firstGroup(_ pattern: String, in text: String) -> String? {
    guard let re = try? NSRegularExpression(pattern: pattern, options: [.dotMatchesLineSeparators]),
          let m = re.firstMatch(in: text, range: NSRange(text.startIndex..., in: text)),
          m.numberOfRanges > 1, let r = Range(m.range(at: 1), in: text) else { return nil }
    return String(text[r])
}

@MainActor
final class NotchModel: ObservableObject {
    @Published private(set) var link: Pairing?
    @Published private(set) var online = false
    /// laufende Arbeit, z. B. „Sende 2 von 3 …“
    @Published private(set) var working: String?
    @Published private(set) var history: [String] = []
    /// Kamera-Adresse; leer, solange die Kamera am PC aus ist
    @Published private(set) var camURL: URL?

    private var base: String?
    /// Warum die letzte Suche scheiterte (Fehlercode je Adresse), zur Fehlersuche
    private var detail = ""
    private let session: URLSession

    init() {
        let c = URLSessionConfiguration.ephemeral
        c.timeoutIntervalForRequest = 15          // Pause zwischen Daten
        c.timeoutIntervalForResource = 60 * 60    // grosse Videos
        session = URLSession(configuration: c)
        if let d = UserDefaults.standard.data(forKey: "link"), let l = try? JSONDecoder().decode(Pairing.self, from: d) {
            link = l
        }
    }

    // MARK: Kopplung

    /// QR-Inhalt oder eingefuegte Adresse: `http://<ip>:53317/k/<schluessel>/`
    func pair(_ raw: String) async {
        guard let addr = firstGroup(#"(https?://[^\s"'<>]+?/k/[0-9A-Za-z]+)"#, in: raw),
              let token = addr.components(separatedBy: "/k/").last else {
            note("⚠︎ " + NotchError.badAddress.localizedDescription)
            return
        }
        working = "Verbinde …"
        defer { working = nil }
        var bases = [addr]
        // Die Einrichtungsseite nennt den Rechnernamen; ihr Abruf meldet das iPhone dem PC zugleich als gekoppelt.
        if let page = try? await text(from: endpoint(addr, "", [:]), timeout: 6),
           let host = firstGroup(#"<textarea id="hostbase" readonly>(.*?)</textarea>"#, in: page), !host.isEmpty {
            bases.insert(host, at: 0)
        }
        let l = Pairing(token: token, bases: bases, pc: "PC")
        link = l
        base = nil
        save()
        if await resolve() {
            note("Gekoppelt mit \(link?.pc ?? "PC")")
        } else {
            note("⚠︎ Gekoppelt, aber der PC antwortet nicht: \(detail)")
        }
    }

    func unpair() {
        link = nil
        base = nil
        online = false
        camURL = nil
        save()
    }

    private func save() {
        if let l = link, let d = try? JSONEncoder().encode(l) { UserDefaults.standard.set(d, forKey: "link") }
        else { UserDefaults.standard.removeObject(forKey: "link") }
    }

    // MARK: Verbindung

    /// Erste erreichbare Adresse merken; meldet dem PC nebenbei „iPhone da“ mit Akkustand
    @discardableResult
    func resolve() async -> Bool {
        guard let l = link else { return false }
        detail = ""
        for b in ([base].compactMap { $0 } + l.bases) {
            guard let u = endpoint(b, "hallo", [:]) else { continue }
            let result: (Data, URLResponse)
            do {
                result = try await probe(u)
            } catch {
                let code = (error as? URLError)?.code.rawValue ?? 0
                detail += "\(u.host ?? b): \(code) \(error.localizedDescription) "
                continue
            }
            let (data, resp) = result
            let status = (resp as? HTTPURLResponse)?.statusCode ?? 0
            guard status == 200 else { detail += "\(u.host ?? b): HTTP \(status) "; continue }
            base = b
            online = true
            if let j = try? JSONSerialization.jsonObject(with: data) as? [String: Any], let pc = j["pc"] as? String, pc != l.pc {
                link?.pc = pc
                save()
            }
            return true
        }
        base = nil
        online = false
        return false
    }

    private func probe(_ u: URL) async throws -> (Data, URLResponse) {
        var r = URLRequest(url: u)
        r.timeoutInterval = 3
        return try await session.data(for: r)
    }

    /// Wird die gemerkte Adresse ungueltig (neue IP), einmal neu suchen und wiederholen
    private func perform<T>(_ op: (String) async throws -> T) async throws -> T {
        if base == nil { await resolve() }
        guard let b = base else { throw NotchError.offline(detail) }
        do {
            return try await op(b)
        } catch let e as URLError where [.cannotConnectToHost, .timedOut, .cannotFindHost, .networkConnectionLost, .dnsLookupFailed, .notConnectedToInternet].contains(e.code) {
            base = nil
            guard await resolve(), let b2 = base else { throw NotchError.offline(detail) }
            return try await op(b2)
        }
    }

    /// Adresse mit Akkustand und Geraetename (die Notch zeigt beides an)
    private func endpoint(_ base: String, _ path: String, _ extra: [String: String]) -> URL? {
        var items = extra
        UIDevice.current.isBatteryMonitoringEnabled = true
        let level = UIDevice.current.batteryLevel
        if level >= 0 { items["akku"] = String(format: "%.2f", level) }
        items["laedt"] = [.charging, .full].contains(UIDevice.current.batteryState) ? "1" : "0"
        items["geraet"] = UIDevice.current.name
        let q = items.sorted { $0.key < $1.key }.map { "\($0.key)=\(enc($0.value))" }.joined(separator: "&")
        return URL(string: "\(base)/\(path)?\(q)")
    }

    private func text(from url: URL?, timeout: TimeInterval = 15) async throws -> String {
        guard let url else { throw NotchError.badAddress }
        var r = URLRequest(url: url)
        r.timeoutInterval = timeout
        let (data, resp) = try await session.data(for: r)
        try check(resp, data)
        return String(decoding: data, as: UTF8.self)
    }

    private func check(_ resp: URLResponse, _ data: Data = Data()) throws {
        let c = (resp as? HTTPURLResponse)?.statusCode ?? 0
        if !(200..<300).contains(c) { throw NotchError.status(c, String(decoding: data.prefix(200), as: UTF8.self)) }
    }

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

    private func upload(_ files: [URL], scoped: Bool) async {
        var sent = 0
        for (i, f) in files.enumerated() {
            working = files.count > 1 ? "Sende \(i + 1) von \(files.count) …" : "Sende …"
            let ok = scoped && f.startAccessingSecurityScopedResource()
            defer { if ok { f.stopAccessingSecurityScopedResource() } }
            let name = f.lastPathComponent
            do {
                try await perform { b in
                    var r = URLRequest(url: endpoint(b, "senden", ["name": name])!)
                    r.httpMethod = "POST"
                    r.setValue(UTType(filenameExtension: f.pathExtension)?.preferredMIMEType ?? "application/octet-stream", forHTTPHeaderField: "Content-Type")
                    let (data, resp) = try await session.upload(for: r, fromFile: f)
                    try check(resp, data)
                }
                sent += 1
            } catch {
                note("⚠︎ \(name): \(error.localizedDescription)")
            }
        }
        working = nil
        if sent > 0 { note(sent == 1 ? "1 Datei an den PC gesendet" : "\(sent) Dateien an den PC gesendet") }
    }

    /// `clip`: landet in der Zwischenablage des PCs; sonst als Text/Link in der Notch (ein Link wird dort geoeffnet)
    func sendText(_ s: String, clip: Bool) async {
        let t = s.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !t.isEmpty else { note("Nichts zu senden"); return }
        working = "Sende …"
        defer { working = nil }
        do {
            try await perform { b in
                var r = URLRequest(url: endpoint(b, clip ? "clip" : "senden", clip ? [:] : ["name": "Text"])!)
                r.httpMethod = "POST"
                r.setValue("text/plain; charset=utf-8", forHTTPHeaderField: "Content-Type")
                let (data, resp) = try await session.upload(for: r, from: Data(t.utf8))
                try check(resp, data)
            }
            note(clip ? "Zwischenablage an den PC gesendet" : "Text an den PC gesendet")
        } catch {
            note("⚠︎ \(error.localizedDescription)")
        }
    }

    // MARK: PC → iPhone

    /// Alles abholen, was am PC bereitliegt. `quiet`: beim Oeffnen der App, ohne Meldung wenn nichts da ist
    func fetch(quiet: Bool = false) async {
        do {
            let list = try await perform { b in try await text(from: endpoint(b, "abholen", [:])) }
            let lines = list.split(whereSeparator: \.isNewline).map(String.init)
            guard lines.first == "abholen" else {
                if !quiet { note("Am PC liegt nichts bereit") }
                return
            }
            var got = 0
            for (i, l) in lines.dropFirst().enumerated() {
                working = "Hole \(i + 1) von \(lines.count - 1) …"
                guard let u = URL(string: l) else { continue }
                do { try await download(u); got += 1 } catch { note("⚠︎ \(error.localizedDescription)") }
            }
            let t = try await perform { b in try await text(from: endpoint(b, "text", [:])) }
            if !t.isEmpty {
                UIPasteboard.general.string = t
                note("Text vom PC in der Zwischenablage")
            }
            if got > 0 { note(got == 1 ? "1 Datei vom PC geholt" : "\(got) Dateien vom PC geholt") }
        } catch {
            if !quiet { note("⚠︎ \(error.localizedDescription)") }
        }
        working = nil
    }

    /// Fotos und Videos in die Fotos-App, alles andere nach Dateien › Notch
    private func download(_ u: URL) async throws {
        let (tmp, resp) = try await session.download(from: u)
        try check(resp)
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
        let page = try? await text(from: endpoint(b, "", [:]), timeout: 6)
        let href = page.flatMap { firstGroup(#"id="camrow" href="([^"]*)""#, in: $0) } ?? ""
        camURL = href.isEmpty ? nil : URL(string: href + "frame")
    }

    // MARK: Verlauf

    func note(_ s: String) {
        history.insert(s, at: 0)
        if history.count > 15 { history.removeLast() }
    }

    /// Knopf „Prüfen“: Ergebnis im Verlauf zeigen
    func check() async {
        working = "Prüfe …"
        let ok = await resolve()
        working = nil
        note(ok ? "Verbunden mit \(link?.pc ?? "PC")" : "⚠︎ " + NotchError.offline(detail).localizedDescription)
    }

    /// Im Vordergrund alle paar Sekunden: bereitgelegte Dateien holen, ohne laufende Arbeit zu stoeren
    func poll() async {
        guard link != nil, working == nil else { return }
        if base == nil { await resolve() }
        if online { await fetch(quiet: true) }
    }

    /// App wird geoeffnet: Verbindung pruefen (meldet das iPhone dem PC) und Bereitliegendes holen
    func onActive() async {
        guard link != nil else { return }
        if await resolve() { await fetch(quiet: true) }
    }
}
