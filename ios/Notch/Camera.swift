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

/// iPhone als Webcam: Kamerabild als JPEG (16:9, hoechstens 1280x720) an `POST <Kamera-Adresse>frame`. Bis zu
/// `maxInflight` Bilder sind gleichzeitig unterwegs (sonst bestimmte die WLAN-Laufzeit die Bildrate); jedes traegt eine
/// Nummer, ein ueberholtes verwirft die Notch. Die Notch auf dem PC macht daraus die virtuelle Kamera „Notch iPhone“.
final class CameraStreamer: NSObject, ObservableObject, AVCaptureVideoDataOutputSampleBufferDelegate, URLSessionDelegate {
    @Published private(set) var running = false { didSet { live() } }
    @Published private(set) var fps = 0 { didSet { live() } }
    @Published private(set) var error: String?
    /// Wird true, sobald das erste Bild gezeigt wird (davor zeigt die Oberflaeche einen Platzhalter)
    @Published private(set) var hasFrame = false
    /// Hier wird das gesendete Bild gezeigt (dasselbe, was der PC sieht), unabhaengig von SwiftUI
    weak var previewView: UIView?
    @Published private(set) var maxZoom = 5.0
    @Published private(set) var hasTorch = false
    /// hoechste Bildrate der Kamera bei mindestens 1280x720 (60 auf iPhones, die es koennen, sonst 30)
    @Published private(set) var maxRate = 30
    /// Hinten verfuegbare Objektive (iPhone 13: ultraweit und weit)
    let lenses: [Lens]

    /// Dieses Geraet ist am PC live (in der Kamera „Notch iPhone“); sonst sieht man es nur als Vorschau in der Notch
    @Published private(set) var isLive = true

    // Einstellungen, bleiben erhalten; beidseitig mit der Notch abgeglichen (syncOut / applyRemote)
    @Published var front: Bool { didSet { store(); reconfigure(); syncOut() } }
    @Published var lens: Lens { didSet { zoom = 1; torch = false; store(); reconfigure(); syncOut() } }
    @Published var quality: Int { didSet { store(); queue.async { [q = quality] in self.jpegQuality = [0.4, 0.6, 0.8][min(max(q, 0), 2)] }; syncOut() } }
    @Published var rate: Int { didSet { store(); adjust(); syncOut() } }
    @Published var mirror: Bool { didSet { store(); queue.async { [m = mirror] in self.mirrored = m }; syncOut() } }
    /// Zoom aus der Notch gleitet (ramp), am iPhone folgt er direkt dem Finger
    @Published var zoom = 1.0 { didSet { adjust(ramp: applyingRemote); syncOut() } }
    @Published var torch = false { didSet { adjust(); syncOut() } }

    private let capture = AVCaptureSession()
    private let queue = DispatchQueue(label: "notch.cam")
    private let ctx = CIContext()
    private var target: URL?
    private lazy var net = URLSession(configuration: .ephemeral, delegate: self, delegateQueue: nil)
    // nur auf `queue`
    private var device: AVCaptureDevice?
    private var jpegQuality = 0.6
    private var mirrored = false
    /// Bilder unterwegs, laufende Bildnummer (nur auf `queue`)
    private var inflight = 0
    private var frameNo = 0
    private let maxInflight = 3
    private var frames = 0
    private var since = Date()
    private var previewPending = false
    private var orientation = AVCaptureVideoOrientation.landscapeRight
    private var applied: AVCaptureVideoOrientation?
    /// Kennung dieses Starts und Name in der Notch: mehrere Geraete duerfen gleichzeitig senden, die Notch zeigt sie
    /// als Vorschau und schaltet eines live
    private var cid = ""
    private var deviceName = "iPhone · App"
    /// Stand der Einstellungen, den die App kennt (nur auf `queue`); ist der der Notch hoeher, holt sie sie ab
    private var knownRev = 0
    private var pulling = false
    // nur auf main
    private var applyingRemote = false
    private var postWork: DispatchWorkItem?

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

    private func live() {
        let f: Int? = running ? fps : nil
        Task { @MainActor in LiveManager.shared.camera(fps: f) }
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
        cid = UUID().uuidString.replacingOccurrences(of: "-", with: "")
        deviceName = "\(UIDevice.current.model) · App"
        isLive = true
        queue.async { self.knownRev = 0 }
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
            inflight = 0
        }
        running = false
        hasFrame = false
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
        applyDevice(dev)
        let maxZ = min(Double(dev.activeFormat.videoMaxZoomFactor), 8)
        let torchOK = dev.hasTorch && dev.isTorchModeSupported(.on)
        let best = bestRate(dev)
        DispatchQueue.main.async {
            self.error = nil
            self.maxZoom = max(maxZ, 1.01)
            self.hasTorch = torchOK
            self.maxRate = best
            // Zoombereich und Licht haengen an der Kamera: der Notch melden
            self.postSettings()
        }
    }

    /// Formate mit mindestens 1280x720 im Seitenverhaeltnis 16:9
    private func wideFormats(_ dev: AVCaptureDevice) -> [AVCaptureDevice.Format] {
        dev.formats.filter { f in
            let d = CMVideoFormatDescriptionGetDimensions(f.formatDescription)
            return d.width >= 1280 && d.height >= 720 && Int(d.width) * 9 == Int(d.height) * 16
        }
    }

    private func bestRate(_ dev: AVCaptureDevice) -> Int {
        let m = wideFormats(dev).flatMap { $0.videoSupportedFrameRateRanges }.map { $0.maxFrameRate }.max() ?? 30
        return m >= 59 ? 60 : 30
    }

    /// kleinstes 16:9-Format ab 1280x720, das diese Bildrate kann (wenig Pixel = schnelles Kodieren)
    private func format(for dev: AVCaptureDevice, rate: Int) -> AVCaptureDevice.Format? {
        let area = { (f: AVCaptureDevice.Format) -> Int in
            let d = CMVideoFormatDescriptionGetDimensions(f.formatDescription)
            return Int(d.width) * Int(d.height)
        }
        return wideFormats(dev)
            .filter { $0.videoSupportedFrameRateRanges.contains { $0.maxFrameRate >= Double(rate) } }
            .min { area($0) < area($1) }
    }

    // MARK: Abgleich mit der Notch
    // Beidseitig: was hier geaendert wird, geht an die Notch (POST settings); aendert man es in der Notch, meldet die
    // Antwort auf das naechste Bild einen hoeheren Stand (X-Notch-Rev) und die App holt die Einstellungen ab.

    /// Adresse neben `frame` (settings, frame) mit Kennung und Name
    private func endpoint(_ name: String, _ extra: [URLQueryItem] = []) -> URL? {
        guard let target, var c = URLComponents(url: target.deletingLastPathComponent().appendingPathComponent(name), resolvingAgainstBaseURL: false) else { return nil }
        c.queryItems = [URLQueryItem(name: "c", value: cid), URLQueryItem(name: "n", value: deviceName)] + extra
        return c.url
    }

    /// Auf main: eine Aenderung am iPhone an die Notch melden (nicht, waehrend Werte aus der Notch uebernommen werden)
    private func syncOut() {
        if !applyingRemote && running { postSettings() }
    }

    /// Auf main: gebuendelt melden (beim Ziehen am Zoom nicht bei jedem Schritt)
    private func postSettings() {
        postWork?.cancel()
        let w = DispatchWorkItem { [weak self] in self?.sendSettings() }
        postWork = w
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.15, execute: w)
    }

    private func sendSettings() {
        guard running, let u = endpoint("settings") else { return }
        let body: [String: Any] = [
            "settings": ["front": front, "lens": lens.rawValue, "quality": quality, "rate": rate, "mirror": mirror, "zoom": zoom, "torch": torch] as [String: Any],
            "caps": ["lenses": lenses.map(\.rawValue), "zoom": ["min": 1.0, "max": maxZoom] as [String: Double], "torch": hasTorch, "maxRate": maxRate] as [String: Any],
        ]
        var r = URLRequest(url: u)
        r.httpMethod = "POST"
        r.timeoutInterval = 5
        r.setValue("application/json", forHTTPHeaderField: "Content-Type")
        r.httpBody = try? JSONSerialization.data(withJSONObject: body)
        net.dataTask(with: r) { [weak self] data, _, _ in
            guard let self, let data,
                  let obj = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
                  let rev = obj["rev"] as? Int else { return }
            self.queue.async { self.knownRev = max(self.knownRev, rev) }
        }.resume()
    }

    /// Auf `queue`: in der Notch geaenderte Einstellungen abholen
    private func pullSettings() {
        guard !pulling, let u = endpoint("settings") else { return }
        pulling = true
        var r = URLRequest(url: u)
        r.timeoutInterval = 5
        net.dataTask(with: r) { [weak self] data, _, _ in
            guard let self else { return }
            let obj = data.flatMap { (try? JSONSerialization.jsonObject(with: $0)) as? [String: Any] }
            self.queue.async {
                self.pulling = false
                guard let obj else { return }
                if let rev = obj["rev"] as? Int { self.knownRev = max(self.knownRev, rev) }
                let s = obj["settings"] as? [String: Any] ?? [:]
                DispatchQueue.main.async { self.applyRemote(s) }
            }
        }.resume()
    }

    /// Auf main: Einstellungen aus der Notch uebernehmen, ohne sie gleich wieder zurueckzumelden
    private func applyRemote(_ s: [String: Any]) {
        applyingRemote = true
        defer { applyingRemote = false }
        if let v = s["front"] as? Bool, v != front { front = v }
        if let v = (s["lens"] as? String).flatMap(Lens.init(rawValue:)), v != lens, lenses.contains(v) { lens = v }
        if let v = s["quality"] as? Int, (0...2).contains(v), v != quality { quality = v }
        if let v = s["rate"] as? Int, [15, 30, 60].contains(v), v != rate { rate = min(v, maxRate) }
        if let v = s["mirror"] as? Bool, v != mirror { mirror = v }
        if let v = s["zoom"] as? Double {
            let z = min(max(v, 1), maxZoom)
            if abs(z - zoom) > 0.01 { zoom = z }
        }
        if let v = s["torch"] as? Bool {
            let t = v && hasTorch && !front
            if t != torch { torch = t }
        }
    }

    /// Zoom, Bildrate und Taschenlampe aendern, waehrend die Kamera laeuft
    private func adjust(ramp: Bool = false) {
        queue.async { [self] in
            if let device { applyDevice(device, ramp: ramp) }
        }
    }

    /// Auf `queue`. `ramp`: Zoom gleitet zum neuen Wert (aus der Notch), sonst sofort (Finger am iPhone)
    private func applyDevice(_ dev: AVCaptureDevice, ramp: Bool = false) {
        let (rate, zoom, torch) = DispatchQueue.main.sync { (self.rate, self.zoom, self.torch) }
        guard (try? dev.lockForConfiguration()) != nil else { return }
        defer { dev.unlockForConfiguration() }
        let fits = { (f: AVCaptureDevice.Format) -> Bool in
            f.videoSupportedFrameRateRanges.contains { Double(rate) >= $0.minFrameRate && Double(rate) <= $0.maxFrameRate }
        }
        // 60 Bilder/s gibt es nur in bestimmten Formaten: dann dorthin wechseln (die Voreinstellung 720p kann oft nur 30)
        if !fits(dev.activeFormat), let f = format(for: dev, rate: rate) {
            dev.activeFormat = f
        }
        // nur Bildraten, die das Format kennt (sonst wirft AVFoundation eine Ausnahme)
        if fits(dev.activeFormat) {
            dev.activeVideoMinFrameDuration = CMTime(value: 1, timescale: CMTimeScale(rate))
            dev.activeVideoMaxFrameDuration = CMTime(value: 1, timescale: CMTimeScale(rate))
        }
        let z = CGFloat(min(max(zoom, 1), min(Double(dev.activeFormat.videoMaxZoomFactor), 8)))
        if ramp {
            dev.ramp(toVideoZoomFactor: z, withRate: 6)
        } else {
            if dev.isRampingVideoZoom { dev.cancelVideoZoomRamp() }
            dev.videoZoomFactor = z
        }
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
        // alle Plaetze belegt: dieses Kamerabild auslassen (das naechste kommt gleich), statt sich zu stauen
        guard inflight < maxInflight, let target, let pb = CMSampleBufferGetImageBuffer(sampleBuffer) else { return }
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
        // groessere Formate (60 Bilder/s gibt es oft nur in 1080p) hier verkleinern: weniger zu kodieren und zu
        // senden, und der PC muss nicht skalieren
        if img.extent.width > 1280.5 {
            let s = 1280 / img.extent.width
            img = img.transformed(by: CGAffineTransform(scaleX: s, y: s))
        }
        let q = CIImageRepresentationOption(rawValue: kCGImageDestinationLossyCompressionQuality as String)
        guard let jpeg = ctx.jpegRepresentation(of: img, colorSpace: CGColorSpace(name: CGColorSpace.sRGB)!, options: [q: jpegQuality]) else { return }

        // Vorschau mit halber Aufloesung bei jedem Bild; ein neues erst, wenn das letzte angezeigt ist
        if !previewPending, let view = previewView {
            let small = img.transformed(by: CGAffineTransform(scaleX: 0.5, y: 0.5))
            if let cg = ctx.createCGImage(small, from: small.extent) {
                previewPending = true
                DispatchQueue.main.async { [weak self] in
                    view.layer.contents = cg
                    if self?.hasFrame == false { self?.hasFrame = true }
                    self?.queue.async { self?.previewPending = false }
                }
            }
        }

        inflight += 1
        frameNo += 1
        let extra = [URLQueryItem(name: "s", value: String(frameNo)), URLQueryItem(name: "r", value: String(knownRev))]
        var r = URLRequest(url: endpoint("frame", extra) ?? target)
        r.httpMethod = "POST"
        r.timeoutInterval = 5
        r.setValue("image/jpeg", forHTTPHeaderField: "Content-Type")
        net.uploadTask(with: r, from: jpeg) { [weak self] data, resp, err in
            guard let self else { return }
            let http = resp as? HTTPURLResponse
            let code = http?.statusCode ?? 0
            // Stand vom PC: ist dieses Geraet live, wurden Einstellungen in der Notch geaendert
            let liveNow = http?.value(forHTTPHeaderField: "X-Notch-Live").map { $0 == "1" }
            let rev = Int(http?.value(forHTTPHeaderField: "X-Notch-Rev") ?? "") ?? 0
            // neuere Einstellungen aus der Notch kommen gleich mit der Antwort (200 mit JSON)
            let news: [String: Any]? = code == 200 ? data.flatMap { (try? JSONSerialization.jsonObject(with: $0)) as? [String: Any] } : nil
            if let liveNow {
                DispatchQueue.main.async { if self.isLive != liveNow { self.isLive = liveNow } }
            }
            self.queue.async {
                if let news, let nrev = news["rev"] as? Int, nrev > self.knownRev {
                    self.knownRev = nrev
                    let s = news["settings"] as? [String: Any] ?? [:]
                    DispatchQueue.main.async { self.applyRemote(s) }
                } else if rev > self.knownRev {
                    self.pullSettings()
                }
                self.inflight = max(0, self.inflight - 1)
                self.frames += 1
                let dt = Date().timeIntervalSince(self.since)
                if dt >= 1 {
                    let f = Int((Double(self.frames) / dt).rounded())
                    (self.frames, self.since) = (0, Date())
                    let msg: String? = err != nil || !(200..<300).contains(code)
                        ? (code == 403 ? "PC lehnt ab: Kamera am PC aus oder Kopplung erneuert"
                            : code == 409 ? "Am PC sendet schon ein anderes Gerät oder ein Safari-Fenster"
                            : "PC nicht erreichbar")
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
