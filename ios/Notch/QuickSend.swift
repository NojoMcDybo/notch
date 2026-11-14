import Photos
import SwiftUI

/// Datei fuer „Schnell senden“: aus „Dateien“ gewaehlt (gemerkt per Lesezeichen) oder vom PC geholt (liegt im App-Ordner)
struct RecentFile: Codable, Identifiable {
    var id: String
    var name: String
    var date: Date
    /// Lesezeichen auf eine Datei ausserhalb der App
    var bookmark: Data?
    /// Dateiname im Ordner der App (vom PC geholt)
    var local: String?
}

/// Was in der Zwischenablage liegt (ohne den Inhalt zu lesen, das loest sonst die iOS-Einfuegen-Abfrage aus)
enum ClipKind {
    case text, link, image

    var title: String {
        switch self {
        case .text: return "Text"
        case .link: return "Link"
        case .image: return "Bild"
        }
    }
    var icon: String {
        switch self {
        case .text: return "text.alignleft"
        case .link: return "link"
        case .image: return "photo"
        }
    }
}

@MainActor
final class QuickModel: ObservableObject {
    @Published private(set) var assets: [PHAsset] = []
    @Published private(set) var status = PHPhotoLibrary.authorizationStatus(for: .readWrite)
    @Published private(set) var files: [RecentFile] = []
    @Published private(set) var clip: ClipKind?
    @Published private(set) var selection: Set<String> = []

    private var remembered: [RecentFile] = []

    var canReadPhotos: Bool { status == .authorized || status == .limited }

    init() {
        if let d = UserDefaults.standard.data(forKey: "recentFiles"), let r = try? JSONDecoder().decode([RecentFile].self, from: d) {
            remembered = r
        }
        refresh()
    }

    func refresh() {
        status = PHPhotoLibrary.authorizationStatus(for: .readWrite)
        loadAssets()
        loadFiles()
        let pb = UIPasteboard.general
        if pb.hasImages { clip = .image }
        else if pb.hasURLs { clip = .link }
        else if pb.hasStrings { clip = .text }
        else { clip = nil }
    }

    func requestPhotos() async {
        _ = await PHPhotoLibrary.requestAuthorization(for: .readWrite)
        refresh()
    }

    func toggle(_ id: String) {
        if selection.contains(id) { selection.remove(id) } else { selection.insert(id) }
    }

    // MARK: Laden

    private func loadAssets() {
        guard canReadPhotos else { assets = []; return }
        let o = PHFetchOptions()
        o.sortDescriptors = [NSSortDescriptor(key: "creationDate", ascending: false)]
        o.fetchLimit = 15
        o.predicate = NSPredicate(format: "mediaType == %d OR mediaType == %d", PHAssetMediaType.image.rawValue, PHAssetMediaType.video.rawValue)
        var list: [PHAsset] = []
        PHAsset.fetchAssets(with: o).enumerateObjects { a, _, _ in list.append(a) }
        assets = list
        selection = selection.filter { s in !s.hasPrefix("a:") || list.contains { "a:" + $0.localIdentifier == s } }
    }

    private var docs: URL? {
        try? FileManager.default.url(for: .documentDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
    }

    private func loadFiles() {
        var all = remembered
        if let docs, let names = try? FileManager.default.contentsOfDirectory(at: docs, includingPropertiesForKeys: [.contentModificationDateKey], options: [.skipsHiddenFiles]) {
            for u in names {
                let date = (try? u.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate) ?? .distantPast
                all.append(RecentFile(id: "l:" + u.lastPathComponent, name: u.lastPathComponent, date: date, bookmark: nil, local: u.lastPathComponent))
            }
        }
        files = Array(all.sorted { $0.date > $1.date }.prefix(10))
    }

    /// Aus „Dateien“ gewaehlte Dateien fuers naechste Mal merken
    func remember(_ urls: [URL]) {
        for u in urls {
            let ok = u.startAccessingSecurityScopedResource()
            defer { if ok { u.stopAccessingSecurityScopedResource() } }
            guard let b = try? u.bookmarkData() else { continue }
            remembered.removeAll { $0.name == u.lastPathComponent }
            remembered.insert(RecentFile(id: "b:" + UUID().uuidString, name: u.lastPathComponent, date: Date(), bookmark: b, local: nil), at: 0)
        }
        remembered = Array(remembered.prefix(8))
        if let d = try? JSONEncoder().encode(remembered) { UserDefaults.standard.set(d, forKey: "recentFiles") }
        loadFiles()
    }

    // MARK: Senden

    /// Alles Angetippte senden (Fotos und Videos im Original, Dateien wie sie sind)
    func sendSelected(_ n: NotchModel) async {
        let sel = selection
        selection = []
        var exported: [URL] = []
        for a in assets where sel.contains("a:" + a.localIdentifier) {
            do { exported.append(try await export(a)) } catch { n.note("Foto oder Video nicht lesbar: \(error.localizedDescription)", ok: false) }
        }
        var others: [URL] = []
        for f in files where sel.contains("f:" + f.id) {
            if let l = f.local, let docs {
                others.append(docs.appendingPathComponent(l))
            } else if let b = f.bookmark {
                var stale = false
                if let u = try? URL(resolvingBookmarkData: b, bookmarkDataIsStale: &stale) { others.append(u) }
                else { n.note("\(f.name): Datei nicht mehr da", ok: false) }
            }
        }
        if !exported.isEmpty { await n.sendTemporary(exported) }
        if !others.isEmpty { await n.send(files: others) }
        refresh()
    }

    /// Das Original aus der Mediathek in eine Datei schreiben (ohne es in den Speicher zu laden; iCloud-Originale werden geholt)
    private func export(_ a: PHAsset) async throws -> URL {
        let res = PHAssetResource.assetResources(for: a)
        let want: PHAssetResourceType = a.mediaType == .video ? .video : .photo
        guard let r = res.first(where: { $0.type == want }) ?? res.first else { throw NotchError.status(0, "Keine Datei zum Senden") }
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let dest = dir.appendingPathComponent(r.originalFilename)
        let o = PHAssetResourceRequestOptions()
        o.isNetworkAccessAllowed = true
        try await PHAssetResourceManager.default().writeData(for: r, toFile: dest, options: o)
        return dest
    }

    /// Zwischenablage an den PC: Text und Links in dessen Zwischenablage, ein Bild als Datei
    func sendClipboard(_ n: NotchModel) async {
        let pb = UIPasteboard.general
        if pb.hasImages, let png = pb.image?.pngData() {
            let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
            let file = dir.appendingPathComponent("Zwischenablage.png")
            if (try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)) != nil,
               (try? png.write(to: file)) != nil {
                await n.sendTemporary([file])
                return
            }
        }
        await n.sendText(pb.string ?? pb.url?.absoluteString ?? "", clip: true)
    }
}

// MARK: Oberflaeche

private struct AssetThumb: View {
    let asset: PHAsset
    let selected: Bool
    @State private var image: UIImage?

    var body: some View {
        ZStack(alignment: .bottomLeading) {
            Color(.tertiarySystemFill)
            if let image { Image(uiImage: image).resizable().scaledToFill() }
            if asset.mediaType == .video {
                Text(duration).font(.caption2.bold()).foregroundStyle(Color.white).padding(4)
                    .shadow(color: .black, radius: 2)
            }
        }
        .frame(width: 78, height: 78)
        .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).stroke(Color.accentColor, lineWidth: selected ? 3 : 0))
        .overlay(alignment: .topTrailing) {
            if selected {
                Image(systemName: "checkmark.circle.fill").foregroundStyle(Color.white, Color.accentColor).padding(4)
            }
        }
        .task { image = await load() }
    }

    private var duration: String {
        let s = Int(asset.duration.rounded())
        return String(format: "%d:%02d", s / 60, s % 60)
    }

    private func load() async -> UIImage? {
        await withCheckedContinuation { cont in
            let o = PHImageRequestOptions()
            o.deliveryMode = .highQualityFormat   // nur ein Aufruf, sonst wuerde die Fortsetzung doppelt aufgerufen
            o.resizeMode = .fast
            PHImageManager.default().requestImage(for: asset, targetSize: CGSize(width: 240, height: 240), contentMode: .aspectFill, options: o) { img, _ in
                cont.resume(returning: img)
            }
        }
    }
}

private struct FileChip: View {
    let file: RecentFile
    let selected: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Image(systemName: selected ? "checkmark.circle.fill" : "doc.fill")
                .foregroundStyle(selected ? Color.accentColor : Color.secondary)
            Text(file.name).font(.caption).lineLimit(2)
            Text(file.local != nil ? "vom PC" : "zuletzt gewählt").font(.caption2).foregroundStyle(.secondary)
        }
        .padding(8)
        .frame(width: 110, height: 78, alignment: .topLeading)
        .background(Color(.tertiarySystemFill), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).stroke(Color.accentColor, lineWidth: selected ? 3 : 0))
    }
}

/// „Schnell senden“: Zwischenablage, letzte Fotos und Videos, letzte Dateien; antippen und gesammelt senden
struct QuickSendSections: View {
    @ObservedObject var quick: QuickModel
    @EnvironmentObject var n: NotchModel

    var body: some View {
        Section {
            if let c = quick.clip { clipboardRow(c) }
            mediaRow
            filesRow
            if !quick.selection.isEmpty {
                Button { Task { await quick.sendSelected(n) } } label: {
                    Label("\(quick.selection.count) senden", systemImage: "paperplane.fill").frame(maxWidth: .infinity)
                }
                .buttonStyle(.borderedProminent)
                .disabled(n.working != nil)
                .listRowBackground(Color.clear)
            }
        } header: {
            Text("Schnell senden")
        }
        .onAppear { quick.refresh() }
        .onReceive(NotificationCenter.default.publisher(for: UIApplication.didBecomeActiveNotification)) { _ in quick.refresh() }
        .onReceive(NotificationCenter.default.publisher(for: UIPasteboard.changedNotification)) { _ in quick.refresh() }
    }

    private func clipboardRow(_ c: ClipKind) -> some View {
        HStack {
            Image(systemName: c.icon).frame(width: 24)
            VStack(alignment: .leading, spacing: 2) {
                Text(c.title + " in der Zwischenablage")
                Text("Beim Senden fragt iOS nach dem Einfügen").font(.caption2).foregroundStyle(.secondary)
            }
            Spacer()
            Button("Senden") { Task { await quick.sendClipboard(n) } }
                .buttonStyle(.borderless)
                .disabled(n.working != nil)
        }
    }

    @ViewBuilder private var mediaRow: some View {
        if quick.canReadPhotos {
            if !quick.assets.isEmpty {
                strip("Zuletzt in Fotos") {
                    ForEach(quick.assets, id: \.localIdentifier) { a in
                        let id = "a:" + a.localIdentifier
                        AssetThumb(asset: a, selected: quick.selection.contains(id))
                            .onTapGesture { quick.toggle(id) }
                    }
                }
            }
        } else if quick.status == .notDetermined {
            Button { Task { await quick.requestPhotos() } } label: { Label("Letzte Fotos und Videos anzeigen", systemImage: "photo.on.rectangle") }
        } else {
            Text("Zugriff auf Fotos: iPhone-Einstellungen › Notch › Fotos").font(.footnote).foregroundStyle(.secondary)
        }
    }

    @ViewBuilder private var filesRow: some View {
        if !quick.files.isEmpty {
            strip("Zuletzt: Dateien") {
                ForEach(quick.files) { f in
                    let id = "f:" + f.id
                    FileChip(file: f, selected: quick.selection.contains(id))
                        .onTapGesture { quick.toggle(id) }
                }
            }
        }
    }

    private func strip<C: View>(_ title: String, @ViewBuilder _ content: () -> C) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(title).font(.caption).foregroundStyle(.secondary).padding(.horizontal, 16)
            ScrollView(.horizontal, showsIndicators: false) {
                LazyHStack(spacing: 8) { content() }.padding(.horizontal, 16)
            }
        }
        .padding(.vertical, 8)
        .listRowInsets(EdgeInsets())
    }
}
