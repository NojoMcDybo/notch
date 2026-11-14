import AVFoundation
import CoreImage
import SwiftUI

enum Lens: String, CaseIterable, Identifiable {
    case ultra, wide, tele

    var id: String { rawValue }
    var title: String {
        switch self {
        case .ultra: return "Ultraweit"
        case .wide: return "Weit"
        case .tele: return "Tele"
        }
    }
    var type: AVCaptureDevice.DeviceType {
        switch self {
        case .ultra: return .builtInUltraWideCamera
        case .wide: return .builtInWideAngleCamera
        case .tele: return .builtInTelephotoCamera
        }
    }
}

/// iPhone als Webcam: Kamerabild als JPEG (16:9) an `POST <Kamera-Adresse>frame`, ein Bild nach dem anderen.
/// Die Notch auf dem PC macht daraus die virtuelle Kamera „Notch iPhone“.
final class CameraStreamer: NSObject, ObservableObject, AVCaptureVideoDataOutputSampleBufferDelegate, URLSessionDelegate {
    @Published private(set) var running = false
    @Published private(set) var fps = 0
    @Published private(set) var error: String?
    /// Das gesendete Bild, damit man sieht, was der PC sieht
    @Published private(set) var preview: UIImage?
    @Published private(set) var maxZoom = 5.0
    @Published private(set) var hasTorch = false
    /// Hinten verfuegbare Objektive (iPhone 13: ultraweit und weit)
    let lenses: [Lens]

    // Einstellungen, bleiben erhalten
    @Published var front: Bool { didSet { store(); reconfigure() } }
    @Published var lens: Lens { didSet { zoom = 1; torch = false; store(); reconfigure() } }
    @Published var quality: Int { didSet { store(); queue.async { [q = quality] in self.jpegQuality = [0.4, 0.6, 0.8][min(max(q, 0), 2)] } } }
    @Published var rate: Int { didSet { store(); adjust() } }
    @Published var mirror: Bool { didSet { store(); queue.async { [m = mirror] in self.mirrored = m } } }
    @Published var zoom = 1.0 { didSet { adjust() } }
    @Published var torch = false { didSet { adjust() } }

    private let capture = AVCaptureSession()
    private let queue = DispatchQueue(label: "notch.cam")
    private let ctx = CIContext()
    private var target: URL?
    private lazy var net = URLSession(configuration: .ephemeral, delegate: self, delegateQueue: nil)
    // nur auf `queue`
    private var device: AVCaptureDevice?
    private var jpegQuality = 0.6
    private var mirrored = false
    private var busy = false
    private var frames = 0
    private var since = Date()
    private var shown = 0
    private var orientation = AVCaptureVideoOrientation.landscapeRight
    private var applied: AVCaptureVideoOrientation?

    override init() {
        let d = UserDefaults.standard
        lenses = Lens.allCases.filter {
            AVCaptureDevice.DiscoverySession(deviceTypes: [$0.type], mediaType: .video, position: .back).devices.first != nil
        }
        front = d.bool(forKey: "cam.front")
        lens = Lens(rawValue: d.string(forKey: "cam.lens") ?? "") ?? .wide
        quality = d.object(forKey: "cam.quality") as? Int ?? 1
        rate = d.object(forKey: "cam.rate") as? Int ?? 30
        mirror = d.bool(forKey: "cam.mirror")
        super.init()
        queue.async { [q = quality, m = mirror] in
            self.jpegQuality = [0.4, 0.6, 0.8][min(max(q, 0), 2)]
            self.mirrored = m
        }
    }

    private func store() {
        let d = UserDefaults.standard
        d.set(front, forKey: "cam.front")
        d.set(lens.rawValue, forKey: "cam.lens")
        d.set(quality, forKey: "cam.quality")
        d.set(rate, forKey: "cam.rate")
        d.set(mirror, forKey: "cam.mirror")
    }

    func start(_ url: URL) {
        target = url
        Task { @MainActor [self] in
            guard await AVCaptureDevice.requestAccess(for: .video) else {
                self.error = "Kein Kamerazugriff (iPhone-Einstellungen › Notch › Kamera)"
                return
            }
            UIDevice.current.beginGeneratingDeviceOrientationNotifications()
            NotificationCenter.default.removeObserver(self)
            NotificationCenter.default.addObserver(self, selector: #selector(turned), name: UIDevice.orientationDidChangeNotification, object: nil)
            turned()
            queue.async { [self] in
                configure()
                if !capture.isRunning { capture.startRunning() }
                DispatchQueue.main.async {
                    self.running = true
                    UIApplication.shared.isIdleTimerDisabled = true   // Bildschirm an, sonst haelt iOS die App an
                }
            }
        }
    }

    func stop() {
        NotificationCenter.default.removeObserver(self)
        UIDevice.current.endGeneratingDeviceOrientationNotifications()
        torch = false
        queue.async { [self] in
            if capture.isRunning { capture.stopRunning() }
            busy = false
        }
        running = false
        fps = 0
        UIApplication.shared.isIdleTimerDisabled = false
    }

    private func reconfigure() {
        guard running else { return }
        queue.async { self.configure() }
    }

    /// Auf `queue`: Kamera und Ausgabe nach den Einstellungen aufbauen
    private func configure() {
        let (front, lens) = DispatchQueue.main.sync { (self.front, self.lens) }
        let dev = front
            ? AVCaptureDevice.default(.builtInWideAngleCamera, for: .video, position: .front)
            : (AVCaptureDevice.default(lens.type, for: .video, position: .back) ?? AVCaptureDevice.default(.builtInWideAngleCamera, for: .video, position: .back))
        capture.beginConfiguration()
        capture.sessionPreset = .hd1280x720
        capture.inputs.forEach { capture.removeInput($0) }
        guard let dev, let input = try? AVCaptureDeviceInput(device: dev), capture.canAddInput(input) else {
            capture.commitConfiguration()
            DispatchQueue.main.async { self.error = "Kamera nicht verfügbar" }
            return
        }
        capture.addInput(input)
        if capture.outputs.isEmpty {
            let out = AVCaptureVideoDataOutput()
            out.alwaysDiscardsLateVideoFrames = true
            out.videoSettings = [kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA]
            out.setSampleBufferDelegate(self, queue: queue)
            if capture.canAddOutput(out) { capture.addOutput(out) }
        }
        capture.commitConfiguration()
        applied = nil
        device = dev
        let maxZ = min(Double(dev.activeFormat.videoMaxZoomFactor), 8)
        let torchOK = dev.hasTorch && dev.isTorchModeSupported(.on)
        DispatchQueue.main.async {
            self.error = nil
            self.maxZoom = max(maxZ, 1.01)
            self.hasTorch = torchOK
        }
        applyDevice(dev)
    }

    /// Zoom, Bildrate und Taschenlampe aendern, waehrend die Kamera laeuft
    private func adjust() {
        queue.async { [self] in
            if let device { applyDevice(device) }
        }
    }

    /// Auf `queue`
    private func applyDevice(_ dev: AVCaptureDevice) {
        let (rate, zoom, torch) = DispatchQueue.main.sync { (self.rate, self.zoom, self.torch) }
        guard (try? dev.lockForConfiguration()) != nil else { return }
        defer { dev.unlockForConfiguration() }
        // nur Bildraten, die das Format kennt (sonst wirft AVFoundation eine Ausnahme)
        if dev.activeFormat.videoSupportedFrameRateRanges.contains(where: { Double(rate) >= $0.minFrameRate && Double(rate) <= $0.maxFrameRate }) {
            dev.activeVideoMinFrameDuration = CMTime(value: 1, timescale: CMTimeScale(rate))
            dev.activeVideoMaxFrameDuration = CMTime(value: 1, timescale: CMTimeScale(rate))
        }
        dev.videoZoomFactor = min(max(zoom, 1), min(Double(dev.activeFormat.videoMaxZoomFactor), 8))
        if dev.hasTorch, dev.isTorchModeSupported(.on) {
            dev.torchMode = torch ? .on : .off
        }
    }

    /// Geraet quer = Bild quer, hochkant = Bild hochkant (der PC schneidet es auf 16:9 zu). Die Achsen sind bei
    /// Geraet und Aufnahme vertauscht (Geraet links gedreht = Aufnahme rechts).
    @objc private func turned() {
        let o: AVCaptureVideoOrientation?
        switch UIDevice.current.orientation {
        case .portrait: o = .portrait
        case .portraitUpsideDown: o = .portraitUpsideDown
        case .landscapeLeft: o = .landscapeRight
        case .landscapeRight: o = .landscapeLeft
        default: o = nil   // flach liegend: letzte Lage behalten
        }
        if let o { queue.async { self.orientation = o } }
    }

    func captureOutput(_ output: AVCaptureOutput, didOutput sampleBuffer: CMSampleBuffer, from connection: AVCaptureConnection) {
        if applied != orientation, connection.isVideoOrientationSupported {
            connection.videoOrientation = orientation
            applied = orientation
        }
        guard !busy, let target, let pb = CMSampleBufferGetImageBuffer(sampleBuffer) else { return }
        var img = CIImage(cvPixelBuffer: pb)
        if mirrored {
            img = img.transformed(by: CGAffineTransform(scaleX: -1, y: 1)).transformed(by: CGAffineTransform(translationX: img.extent.width, y: 0))
        }
        // auf 16:9 zuschneiden (hochkant: mittiger Streifen)
        let e = img.extent
        let (w, h) = (e.width, e.height)
        if w * 9 > h * 16 {
            let nw = h * 16 / 9
            img = img.cropped(to: CGRect(x: e.minX + (w - nw) / 2, y: e.minY, width: nw, height: h))
        } else if w * 9 < h * 16 {
            let nh = w * 9 / 16
            img = img.cropped(to: CGRect(x: e.minX, y: e.minY + (h - nh) / 2, width: w, height: nh))
        }
        let q = CIImageRepresentationOption(rawValue: kCGImageDestinationLossyCompressionQuality as String)
        guard let jpeg = ctx.jpegRepresentation(of: img, colorSpace: CGColorSpace(name: CGColorSpace.sRGB)!, options: [q: jpegQuality]) else { return }

        shown += 1
        if shown % 8 == 0, let cg = ctx.createCGImage(img, from: img.extent) {
            let ui = UIImage(cgImage: cg)
            DispatchQueue.main.async { self.preview = ui }
        }

        busy = true
        var r = URLRequest(url: target)
        r.httpMethod = "POST"
        r.timeoutInterval = 5
        r.setValue("image/jpeg", forHTTPHeaderField: "Content-Type")
        net.uploadTask(with: r, from: jpeg) { [weak self] _, resp, err in
            guard let self else { return }
            let code = (resp as? HTTPURLResponse)?.statusCode ?? 0
            self.queue.async {
                self.busy = false
                self.frames += 1
                let dt = Date().timeIntervalSince(self.since)
                if dt >= 1 {
                    let f = Int((Double(self.frames) / dt).rounded())
                    (self.frames, self.since) = (0, Date())
                    let msg: String? = err != nil || !(200..<300).contains(code)
                        ? (code == 403 ? "PC lehnt ab: Kamera am PC aus oder Kopplung erneuert" : "PC nicht erreichbar")
                        : nil
                    DispatchQueue.main.async { self.fps = msg == nil ? f : 0; self.error = msg }
                }
            }
        }.resume()
    }

    /// Die Notch nutzt ein eigenes Zertifikat (kein Aussteller): fuer die gekoppelte PC-Adresse annehmen
    func urlSession(_ session: URLSession, didReceive challenge: URLAuthenticationChallenge,
                    completionHandler: @escaping (URLSession.AuthChallengeDisposition, URLCredential?) -> Void) {
        let p = challenge.protectionSpace
        if p.authenticationMethod == NSURLAuthenticationMethodServerTrust, p.host == target?.host, let trust = p.serverTrust {
            completionHandler(.useCredential, URLCredential(trust: trust))
        } else {
            completionHandler(.performDefaultHandling, nil)
        }
    }
}

struct CameraView: View {
    @EnvironmentObject var n: NotchModel
    @StateObject private var cam = CameraStreamer()

    var body: some View {
        List {
            Section {
                if let img = cam.preview, cam.running {
                    Image(uiImage: img).resizable().scaledToFit().clipShape(RoundedRectangle(cornerRadius: 10))
                        .listRowInsets(EdgeInsets())
                } else {
                    Text(n.camURL == nil ? "Am PC ist die Kamera aus: Notch › Einstellungen › iPhone › Kamera." : "Starte die Kamera, der PC bekommt dann „Notch iPhone“ als Webcam.")
                        .foregroundStyle(.secondary)
                }
            }
            Section {
                Button(cam.running ? "Stoppen" : "Starten") {
                    if cam.running { cam.stop() } else {
                        Task {
                            await n.refreshCam()   // vielleicht gerade erst am PC eingeschaltet
                            if let u = n.camURL { cam.start(u) }
                        }
                    }
                }
                if cam.running { LabeledContent("Bilder pro Sekunde", value: "\(cam.fps)") }
                if let e = cam.error { Text(e).foregroundStyle(.red) }
            } footer: {
                Text("Quer halten und die App offen lassen. Am PC erscheint die Kamera „Notch iPhone“ in Teams, Zoom und Chrome.")
            }
            Section("Bild") {
                Toggle("Frontkamera", isOn: $cam.front)
                if !cam.front && cam.lenses.count > 1 {
                    Picker("Objektiv", selection: $cam.lens) {
                        ForEach(cam.lenses) { Text($0.title).tag($0) }
                    }
                    .pickerStyle(.segmented)
                }
                VStack(alignment: .leading) {
                    Text("Zoom \(String(format: "%.1f", cam.zoom))×").font(.callout)
                    Slider(value: $cam.zoom, in: 1...cam.maxZoom)
                }
                Toggle("Taschenlampe", isOn: $cam.torch).disabled(cam.front || !cam.hasTorch)
                Toggle("Bild spiegeln", isOn: $cam.mirror)
            }
            Section("Übertragung") {
                Picker("Qualität", selection: $cam.quality) {
                    Text("Niedrig").tag(0)
                    Text("Normal").tag(1)
                    Text("Hoch").tag(2)
                }
                .pickerStyle(.segmented)
                Picker("Bildrate", selection: $cam.rate) {
                    Text("15").tag(15)
                    Text("30").tag(30)
                }
                .pickerStyle(.segmented)
            }
        }
        .navigationTitle("Webcam")
        .task { await n.refreshCam() }
        .onDisappear { cam.stop() }
    }
}
