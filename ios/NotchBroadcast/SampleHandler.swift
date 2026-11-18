import AVFoundation
import CoreImage
import ReplayKit
import UIKit

/// Bildschirmuebertragung an den PC: Bilder als JPEG an `…/screen`, Ton (Ton der Apps) als 16-bit-PCM an `…/audio`.
/// Die Notch zeigt das Bild als virtuelle Kamera „Notch iPhone“ und spielt den Ton am Standardausgang.
/// Die Adresse legt die App beim Start in der gemeinsamen Gruppe ab (PairingStore.saveCam).
class SampleHandler: RPBroadcastSampleHandler, URLSessionDelegate {
    private var base: URL?
    private lazy var session = URLSession(configuration: .ephemeral, delegate: self, delegateQueue: nil)
    private let ctx = CIContext()
    private let lock = NSLock()
    // alles Folgende nur unter `lock`
    private var videoBusy = false
    private var lastFrame = Date.distantPast
    private var failures = 0
    private var lastOK = Date()
    private var lastError = ""
    private var audioBusy = false
    private var pending: [Int16] = []
    private var rate = 0
    private var channels = 0

    override func broadcastStarted(withSetupInfo setupInfo: [String: NSObject]?) {
        guard let b = PairingStore.loadCam() else {
            finishBroadcastWithError(NSError(domain: "Notch", code: 1, userInfo: [
                NSLocalizedDescriptionKey: "Kamera am PC aus oder nicht gekoppelt. Notch-App öffnen und neu starten."]))
            return
        }
        base = b
    }

    override func processSampleBuffer(_ sampleBuffer: CMSampleBuffer, with type: RPSampleBufferType) {
        switch type {
        case .video: video(sampleBuffer)
        case .audioApp: audio(sampleBuffer)
        default: break   // Mikrofon bewusst nicht
        }
    }

    // MARK: Bild

    private func video(_ sb: CMSampleBuffer) {
        lock.lock()
        // hoechstens 15 Bilder pro Sekunde und nie zwei gleichzeitig (Reihenfolge, Speicher der Erweiterung ist knapp)
        guard !videoBusy, Date().timeIntervalSince(lastFrame) > 0.066 else { lock.unlock(); return }
        videoBusy = true
        lastFrame = Date()
        lock.unlock()
        guard let px = CMSampleBufferGetImageBuffer(sb), let base else { done(video: true, ok: false); return }
        var img = CIImage(cvPixelBuffer: px)
        if let o = CMGetAttachment(sb, key: RPVideoSampleOrientationKey as CFString, attachmentModeOut: nil) as? NSNumber,
           let ori = CGImagePropertyOrientation(rawValue: o.uint32Value) {
            img = img.oriented(ori)
        }
        // in 1280x720 einpassen (der PC fuellt den Rest schwarz)
        let s = min(1280 / img.extent.width, 720 / img.extent.height, 1)
        img = img.transformed(by: CGAffineTransform(scaleX: s, y: s))
        guard let jpeg = ctx.jpegRepresentation(of: img, colorSpace: CGColorSpaceCreateDeviceRGB(),
                                                options: [kCGImageDestinationLossyCompressionQuality as CIImageRepresentationOption: 0.6]) else {
            done(video: true, ok: false)
            return
        }
        send(base.appendingPathComponent("screen"), body: jpeg) { [weak self] ok in self?.done(video: true, ok: ok) }
    }

    // MARK: Ton

    private func audio(_ sb: CMSampleBuffer) {
        guard base != nil, let fd = CMSampleBufferGetFormatDescription(sb),
              let asbd = CMAudioFormatDescriptionGetStreamBasicDescription(fd)?.pointee else { return }
        let src = Int(asbd.mChannelsPerFrame)
        let ch = min(max(src, 1), 2)
        let frames = CMSampleBufferGetNumSamples(sb)
        let list = AudioBufferList.allocate(maximumBuffers: 2)
        defer { free(list.unsafeMutablePointer) }
        var block: CMBlockBuffer?
        let st = CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer(
            sb, bufferListSizeNeededOut: nil, bufferListOut: list.unsafeMutablePointer,
            bufferListSize: AudioBufferList.sizeInBytes(maximumBuffers: 2), blockBufferAllocator: nil,
            blockBufferMemoryAllocator: nil, flags: 0, blockBufferOut: &block)
        guard st == noErr, frames > 0 else { return }
        let isFloat = asbd.mFormatFlags & kAudioFormatFlagIsFloat != 0
        let planar = asbd.mFormatFlags & kAudioFormatFlagIsNonInterleaved != 0
        var out = [Int16](repeating: 0, count: frames * ch)
        for c in 0..<ch {
            let b = list[planar ? min(c, list.count - 1) : 0]
            guard let p = b.mData else { continue }
            let stride = planar ? 1 : src
            let offset = planar ? 0 : c
            for i in 0..<frames {
                let idx = i * stride + offset
                if isFloat {
                    let v = p.assumingMemoryBound(to: Float.self)[idx]
                    out[i * ch + c] = Int16(max(-1, min(1, v)) * 32767)
                } else {
                    out[i * ch + c] = p.assumingMemoryBound(to: Int16.self)[idx]
                }
            }
        }
        lock.lock()
        if rate != Int(asbd.mSampleRate) || channels != ch { pending.removeAll(); rate = Int(asbd.mSampleRate); channels = ch }
        pending.append(contentsOf: out)
        // hoechstens 1 s aufstauen, falls der PC nicht antwortet
        if pending.count > rate * ch { pending.removeFirst(pending.count - rate * ch) }
        lock.unlock()
        flushAudio()
    }

    /// Ton in Stuecken von ~0,1 s, immer nur eines unterwegs, damit die Reihenfolge stimmt
    private func flushAudio() {
        lock.lock()
        guard !audioBusy, let base, rate > 0, pending.count >= rate * channels / 10 else { lock.unlock(); return }
        audioBusy = true
        let data = pending.withUnsafeBytes { Data($0) }
        let (r, c) = (rate, channels)
        pending.removeAll(keepingCapacity: true)
        lock.unlock()
        var comps = URLComponents(url: base.appendingPathComponent("audio"), resolvingAgainstBaseURL: false)!
        comps.queryItems = [URLQueryItem(name: "rate", value: String(r)), URLQueryItem(name: "ch", value: String(c))]
        send(comps.url!, body: data) { [weak self] ok in
            self?.done(video: false, ok: ok)
            self?.flushAudio()
        }
    }

    // MARK: Netz

    private func send(_ url: URL, body: Data, _ finished: @escaping (Bool) -> Void) {
        var r = URLRequest(url: url)
        r.httpMethod = "POST"
        r.timeoutInterval = 3
        session.uploadTask(with: r, from: body) { [weak self] _, resp, err in
            let code = (resp as? HTTPURLResponse)?.statusCode
            if err != nil || code != 204 { self?.lock.lock(); self?.lastError = err.map { "\(($0 as NSError).code) \($0.localizedDescription)" } ?? "HTTP \(code ?? 0)"; self?.lock.unlock() }
            finished(err == nil && code == 204)
        }.resume()
    }

    private func done(video: Bool, ok: Bool) {
        lock.lock()
        if video { videoBusy = false } else { audioBusy = false }
        if ok { lastOK = Date(); failures = 0 } else { failures += 1 }
        // aufgeben erst, wenn 8 s lang nichts mehr ankam (kurze WLAN-Aussetzer verzeihen)
        let giveUp = failures > 10 && Date().timeIntervalSince(lastOK) > 8
        let why = lastError
        lock.unlock()
        if giveUp {
            finishBroadcastWithError(NSError(domain: "Notch", code: 2, userInfo: [
                NSLocalizedDescriptionKey: "Der PC antwortet nicht (Notch am PC aktuell? Kamera an? Gleiches WLAN?) – \(why)"]))
        }
    }

    func urlSession(_ session: URLSession, didReceive challenge: URLAuthenticationChallenge,
                    completionHandler: @escaping (URLSession.AuthChallengeDisposition, URLCredential?) -> Void) {
        let p = challenge.protectionSpace
        if p.authenticationMethod == NSURLAuthenticationMethodServerTrust, p.host == base?.host, let trust = p.serverTrust {
            completionHandler(.useCredential, URLCredential(trust: trust))
        } else {
            completionHandler(.performDefaultHandling, nil)
        }
    }
}
