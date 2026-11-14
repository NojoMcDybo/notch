import UIKit

// Gemeinsam fuer die App und die Teilen-Erweiterung: Kopplung, Adressen, Senden an den PC.

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

/// Prozent-Kodierung fuer Werte in der Adresse; `+` und `&` muessen mit, sonst liest der PC sie falsch
func enc(_ s: String) -> String {
    s.addingPercentEncoding(withAllowedCharacters: CharacterSet(charactersIn: "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-._~")) ?? ""
}

func firstGroup(_ pattern: String, in text: String) -> String? {
    guard let re = try? NSRegularExpression(pattern: pattern, options: [.dotMatchesLineSeparators]),
          let m = re.firstMatch(in: text, range: NSRange(text.startIndex..., in: text)),
          m.numberOfRanges > 1, let r = Range(m.range(at: 1), in: text) else { return nil }
    return String(text[r])
}

/// Kopplung ablegen, damit auch die Teilen-Erweiterung sie kennt. Die gemeinsame Gruppe heisst `group.de.nojo.notch`;
/// manche Signierwerkzeuge haengen Kennungen an, darum zusaetzlich `group.<Bundle-ID der App>`.
enum PairingStore {
    private static var hostBundleID: String {
        let id = Bundle.main.bundleIdentifier ?? ""
        // Erweiterung: de.nojo.notch.phone.share → App: de.nojo.notch.phone
        return Bundle.main.bundlePath.hasSuffix(".appex") ? id.split(separator: ".").dropLast().joined(separator: ".") : id
    }

    static var container: URL? {
        for g in ["group.de.nojo.notch", "group.\(hostBundleID)"] {
            if let u = FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: g) { return u }
        }
        return nil
    }

    static func save(_ p: Pairing?) {
        let file = container?.appendingPathComponent("pairing.json")
        if let p, let d = try? JSONEncoder().encode(p) {
            UserDefaults.standard.set(d, forKey: "link")
            if let file { try? d.write(to: file) }
        } else {
            UserDefaults.standard.removeObject(forKey: "link")
            if let file { try? FileManager.default.removeItem(at: file) }
        }
    }

    static func load() -> Pairing? {
        let d = container.flatMap { try? Data(contentsOf: $0.appendingPathComponent("pairing.json")) }
            ?? UserDefaults.standard.data(forKey: "link")
        return d.flatMap { try? JSONDecoder().decode(Pairing.self, from: $0) }
    }
}

/// Meldet Fortschritt einer Uebertragung (Senden und Holen), ohne dass die Aufrufer eine Sitzung bauen muessen
final class ProgressReporter: NSObject, URLSessionTaskDelegate {
    private var observation: NSKeyValueObservation?
    private let report: (Double) -> Void

    init(_ report: @escaping (Double) -> Void) { self.report = report }

    func urlSession(_ session: URLSession, didCreateTask task: URLSessionTask) {
        observation = task.progress.observe(\.fractionCompleted) { [report] p, _ in report(p.fractionCompleted) }
    }
}

@MainActor
enum NotchNet {
    static let session: URLSession = {
        let c = URLSessionConfiguration.ephemeral
        c.timeoutIntervalForRequest = 15          // Pause zwischen Daten
        c.timeoutIntervalForResource = 60 * 60    // grosse Videos
        return URLSession(configuration: c)
    }()

    /// Adresse mit Akkustand und Geraetename (die Notch zeigt beides an)
    static func endpoint(_ base: String, _ path: String, _ extra: [String: String] = [:]) -> URL? {
        var items = extra
        UIDevice.current.isBatteryMonitoringEnabled = true
        let level = UIDevice.current.batteryLevel
        if level >= 0 { items["akku"] = String(format: "%.2f", level) }
        items["laedt"] = [.charging, .full].contains(UIDevice.current.batteryState) ? "1" : "0"
        items["geraet"] = UIDevice.current.name
        let q = items.sorted { $0.key < $1.key }.map { "\($0.key)=\(enc($0.value))" }.joined(separator: "&")
        return URL(string: "\(base)/\(path)?\(q)")
    }

    static func check(_ resp: URLResponse, _ data: Data = Data()) throws {
        let c = (resp as? HTTPURLResponse)?.statusCode ?? 0
        if !(200..<300).contains(c) { throw NotchError.status(c, String(decoding: data.prefix(200), as: UTF8.self)) }
    }

    static func text(from url: URL?, timeout: TimeInterval = 15) async throws -> String {
        guard let url else { throw NotchError.badAddress }
        var r = URLRequest(url: url)
        r.timeoutInterval = timeout
        let (data, resp) = try await session.data(for: r)
        try check(resp, data)
        return String(decoding: data, as: UTF8.self)
    }

    /// Erste erreichbare Adresse (die zuletzt gemerkte zuerst); meldet dem PC nebenbei „iPhone da“.
    /// `detail` nennt je Adresse den Fehlercode, wenn keine antwortet.
    static func resolve(_ p: Pairing, preferred: String?) async -> (base: String?, pc: String?, detail: String) {
        var detail = ""
        for b in ([preferred].compactMap { $0 } + p.bases) {
            guard let u = endpoint(b, "hallo") else { continue }
            var r = URLRequest(url: u)
            r.timeoutInterval = 3
            do {
                let (data, resp) = try await session.data(for: r)
                let status = (resp as? HTTPURLResponse)?.statusCode ?? 0
                guard status == 200 else { detail += "\(u.host ?? b): HTTP \(status) "; continue }
                let pc = (try? JSONSerialization.jsonObject(with: data) as? [String: Any])?["pc"] as? String
                return (b, pc, "")
            } catch {
                let code = (error as? URLError)?.code.rawValue ?? 0
                detail += "\(u.host ?? b): \(code) \(error.localizedDescription) "
            }
        }
        return (nil, nil, detail)
    }

    static func upload(file: URL, name: String, base: String, progress: ((Double) -> Void)? = nil) async throws {
        guard let u = endpoint(base, "senden", ["name": name]) else { throw NotchError.badAddress }
        var r = URLRequest(url: u)
        r.httpMethod = "POST"
        r.setValue(mime(for: file), forHTTPHeaderField: "Content-Type")
        let reporter = progress.map { ProgressReporter($0) }   // die Sitzung haelt den Beobachter nicht sicher fest
        let (data, resp) = try await session.upload(for: r, fromFile: file, delegate: reporter)
        withExtendedLifetime(reporter) {}
        try check(resp, data)
    }

    /// `clip`: landet in der Zwischenablage des PCs; sonst als Text/Link in der Notch (ein Link wird dort geoeffnet)
    static func upload(text: String, clip: Bool, base: String) async throws {
        guard let u = endpoint(base, clip ? "clip" : "senden", clip ? [:] : ["name": "Text"]) else { throw NotchError.badAddress }
        var r = URLRequest(url: u)
        r.httpMethod = "POST"
        r.setValue("text/plain; charset=utf-8", forHTTPHeaderField: "Content-Type")
        let (data, resp) = try await session.upload(for: r, from: Data(text.utf8))
        try check(resp, data)
    }

    private static func mime(for file: URL) -> String {
        let ext = file.pathExtension.lowercased()
        let table = ["jpg": "image/jpeg", "jpeg": "image/jpeg", "png": "image/png", "heic": "image/heic", "heif": "image/heif",
                     "gif": "image/gif", "mov": "video/quicktime", "mp4": "video/mp4", "m4v": "video/x-m4v", "pdf": "application/pdf"]
        return table[ext] ?? "application/octet-stream"
    }

    /// Fehler, bei denen eine neue Adresssuche hilft (PC hat die IP gewechselt)
    static func isUnreachable(_ e: Error) -> Bool {
        guard let u = e as? URLError else { return false }
        return [.cannotConnectToHost, .timedOut, .cannotFindHost, .networkConnectionLost, .dnsLookupFailed, .notConnectedToInternet].contains(u.code)
    }
}
