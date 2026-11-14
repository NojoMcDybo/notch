import UIKit
import UniformTypeIdentifiers

/// Teilen-Menue: „Notch“ in der App-Reihe. Schickt Fotos, Videos, Dateien, Links und Text an den PC,
/// mit der Kopplung aus der App (gemeinsame Gruppe, siehe PairingStore).
final class ShareViewController: UIViewController {
    private let label = UILabel()
    private let bar = UIProgressView(progressViewStyle: .default)
    private let spinner = UIActivityIndicatorView(style: .medium)

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = UIColor.black.withAlphaComponent(0.35)
        let box = UIView()
        box.backgroundColor = .secondarySystemBackground
        box.layer.cornerRadius = 16
        box.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(box)

        label.font = .preferredFont(forTextStyle: .headline)
        label.textAlignment = .center
        label.numberOfLines = 0
        label.text = "Notch"
        spinner.startAnimating()
        bar.isHidden = true
        let stack = UIStackView(arrangedSubviews: [label, spinner, bar])
        stack.axis = .vertical
        stack.spacing = 14
        stack.translatesAutoresizingMaskIntoConstraints = false
        box.addSubview(stack)

        NSLayoutConstraint.activate([
            box.centerXAnchor.constraint(equalTo: view.centerXAnchor),
            box.centerYAnchor.constraint(equalTo: view.centerYAnchor),
            box.widthAnchor.constraint(equalToConstant: 280),
            stack.topAnchor.constraint(equalTo: box.topAnchor, constant: 22),
            stack.bottomAnchor.constraint(equalTo: box.bottomAnchor, constant: -22),
            stack.leadingAnchor.constraint(equalTo: box.leadingAnchor, constant: 20),
            stack.trailingAnchor.constraint(equalTo: box.trailingAnchor, constant: -20),
        ])
        Task { await run() }
    }

    private func run() async {
        guard let pairing = PairingStore.load() else {
            return finish("Erst die Notch-App öffnen und mit dem PC koppeln.", ok: false)
        }
        label.text = "Verbinde …"
        let r = await NotchNet.resolve(pairing, preferred: nil)
        guard let base = r.base else {
            return finish("PC nicht erreichbar.\n\(r.detail)", ok: false)
        }
        let providers = (extensionContext?.inputItems as? [NSExtensionItem])?.flatMap { $0.attachments ?? [] } ?? []
        guard !providers.isEmpty else { return finish("Nichts zu senden.", ok: false) }

        var sent = 0
        var lastError = ""
        for (i, p) in providers.enumerated() {
            label.text = providers.count > 1 ? "Sende \(i + 1) von \(providers.count) …" : "Sende …"
            bar.progress = 0
            do {
                try await send(p, base: base)
                sent += 1
            } catch {
                lastError = error.localizedDescription
            }
        }
        if sent == providers.count {
            finish(sent == 1 ? "An den PC gesendet" : "\(sent) an den PC gesendet", ok: true)
        } else {
            finish("\(sent) von \(providers.count) gesendet.\n\(lastError)", ok: false)
        }
    }

    private func send(_ p: NSItemProvider, base: String) async throws {
        let ids = p.registeredTypeIdentifiers
        func id(_ t: UTType) -> String? { ids.first { UTType($0)?.conforms(to: t) == true } }

        if let t = id(.movie) ?? id(.image) {
            return try await upload(copyOut(p, type: t), base: base)
        }
        if p.hasItemConformingToTypeIdentifier(UTType.fileURL.identifier),
           let url = try await p.loadItem(forTypeIdentifier: UTType.fileURL.identifier) as? URL {
            let scoped = url.startAccessingSecurityScopedResource()
            defer { if scoped { url.stopAccessingSecurityScopedResource() } }
            return try await upload(url, base: base, cleanup: false)
        }
        if p.hasItemConformingToTypeIdentifier(UTType.url.identifier),
           let url = try await p.loadItem(forTypeIdentifier: UTType.url.identifier) as? URL {
            return try await NotchNet.upload(text: url.absoluteString, clip: false, base: base)
        }
        if p.hasItemConformingToTypeIdentifier(UTType.plainText.identifier),
           let text = try await p.loadItem(forTypeIdentifier: UTType.plainText.identifier) as? String {
            return try await NotchNet.upload(text: text, clip: false, base: base)
        }
        if let t = id(.data) {
            return try await upload(copyOut(p, type: t), base: base)
        }
        throw NotchError.status(0, "Dieser Inhalt lässt sich nicht senden")
    }

    private func upload(_ file: URL, base: String, cleanup: Bool = true) async throws {
        defer { if cleanup { try? FileManager.default.removeItem(at: file.deletingLastPathComponent()) } }
        bar.isHidden = false
        try await NotchNet.upload(file: file, name: file.lastPathComponent, base: base) { [weak self] p in
            Task { @MainActor in self?.bar.progress = Float(p) }
        }
    }

    /// Die Datei, die das System nur waehrend des Aufrufs bereithaelt, in den Temp-Ordner kopieren
    private func copyOut(_ p: NSItemProvider, type: String) async throws -> URL {
        try await withCheckedThrowingContinuation { cont in
            p.loadFileRepresentation(forTypeIdentifier: type) { url, err in
                guard let url else {
                    cont.resume(throwing: err ?? NotchError.status(0, "Datei nicht lesbar"))
                    return
                }
                do {
                    let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
                    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
                    let dest = dir.appendingPathComponent(url.lastPathComponent)
                    try FileManager.default.copyItem(at: url, to: dest)
                    cont.resume(returning: dest)
                } catch {
                    cont.resume(throwing: error)
                }
            }
        }
    }

    private func finish(_ text: String, ok: Bool) {
        spinner.stopAnimating()
        bar.isHidden = true
        label.text = (ok ? "✓ " : "⚠︎ ") + text
        DispatchQueue.main.asyncAfter(deadline: .now() + (ok ? 0.9 : 3)) { [weak self] in
            if ok {
                self?.extensionContext?.completeRequest(returningItems: nil)
            } else {
                self?.extensionContext?.cancelRequest(withError: NotchError.status(0, text))
            }
        }
    }
}
