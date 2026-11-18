import SwiftUI
import UIKit

/// Zeigt das gesendete Bild (dasselbe, was der PC sieht). Der Streamer setzt `layer.contents` selbst,
/// damit 30 Bilder pro Sekunde nicht jedes Mal die SwiftUI-Oberflaeche neu aufbauen.
struct CameraPreview: UIViewRepresentable {
    let cam: CameraStreamer

    func makeUIView(context: Context) -> UIView {
        let v = UIView()
        v.backgroundColor = .black
        v.layer.contentsGravity = .resizeAspect
        cam.previewView = v
        return v
    }

    func updateUIView(_ v: UIView, context: Context) {}
}

/// Schalter als Kachel mit Symbol
private struct Chip: View {
    let icon: String
    let title: String
    @Binding var isOn: Bool

    var body: some View {
        Button { isOn.toggle() } label: {
            VStack(spacing: 6) {
                Image(systemName: icon).font(.title3)
                Text(title).font(.caption)
            }
            .frame(maxWidth: .infinity)
            .padding(.vertical, 12)
            .background(isOn ? Color.accentColor : Color(.secondarySystemGroupedBackground), in: RoundedRectangle(cornerRadius: 14, style: .continuous))
            .foregroundStyle(isOn ? Color.white : Color.primary)
        }
        .buttonStyle(.plain)
    }
}

struct CameraView: View {
    @EnvironmentObject var n: NotchModel
    @StateObject private var cam = CameraStreamer()

    var body: some View {
        ScrollView {
            VStack(spacing: 16) {
                preview
                notice
                startButton
                HStack(spacing: 10) {
                    Chip(icon: "camera.rotate", title: "Frontkamera", isOn: $cam.front)
                    Chip(icon: "flashlight.on.fill", title: "Licht", isOn: $cam.torch)
                        .disabled(cam.front || !cam.hasTorch)
                        .opacity(cam.front || !cam.hasTorch ? 0.4 : 1)
                    Chip(icon: "arrow.left.and.right", title: "Spiegeln", isOn: $cam.mirror)
                }
                card("Bildausschnitt") {
                    if !cam.front && cam.lenses.count > 1 {
                        Picker("Objektiv", selection: $cam.lens) {
                            ForEach(cam.lenses) { Text($0.title).tag($0) }
                        }
                        .pickerStyle(.segmented)
                    }
                    HStack(spacing: 10) {
                        Image(systemName: "minus.magnifyingglass").foregroundStyle(.secondary)
                        Slider(value: $cam.zoom, in: 1...cam.maxZoom)
                        Image(systemName: "plus.magnifyingglass").foregroundStyle(.secondary)
                    }
                    Text("Zoom " + String(format: "%.1f", cam.zoom) + "×").font(.footnote).monospacedDigit().foregroundStyle(.secondary)
                }
                card("Übertragung") {
                    LabeledContent("Qualität") {
                        Picker("Qualität", selection: $cam.quality) {
                            Text("Niedrig").tag(0)
                            Text("Normal").tag(1)
                            Text("Hoch").tag(2)
                        }
                        .pickerStyle(.segmented)
                        .frame(maxWidth: 210)
                    }
                    LabeledContent("Bildrate") {
                        Picker("Bildrate", selection: $cam.rate) {
                            Text("15").tag(15)
                            Text("30").tag(30)
                        }
                        .pickerStyle(.segmented)
                        .frame(maxWidth: 210)
                    }
                }
                Text("Das Bild folgt der Haltung des iPhones, quer ist für Videoanrufe am besten. Die App offen lassen; am PC erscheint die Kamera „Notch iPhone“ in Teams, Zoom und Chrome.")
                    .font(.footnote).foregroundStyle(.secondary).multilineTextAlignment(.center)
                    .padding(.horizontal, 8)
            }
            .padding(16)
        }
        .background(Color(.systemGroupedBackground).ignoresSafeArea())
        .navigationTitle("Webcam")
        .navigationBarTitleDisplayMode(.inline)
        .task { await n.refreshCam() }
        .onDisappear { cam.stop() }
    }

    // MARK: Teile

    private var preview: some View {
        ZStack {
            CameraPreview(cam: cam)
            if !cam.hasFrame {
                VStack(spacing: 8) {
                    Image(systemName: cam.running ? "video" : "video.slash").font(.system(size: 34))
                    Text(cam.running ? "Kamera startet …" : "Kamera aus").font(.footnote)
                }
                .foregroundStyle(Color.white.opacity(0.7))
            }
        }
        .aspectRatio(16.0 / 9.0, contentMode: .fit)
        .clipShape(RoundedRectangle(cornerRadius: 16, style: .continuous))
        .overlay(alignment: .topLeading) {
            if cam.running && cam.hasFrame {
                HStack(spacing: 6) {
                    Circle().fill(cam.fps > 0 ? Color.red : Color.orange).frame(width: 8, height: 8)
                    Text(cam.fps > 0 ? "LIVE · \(cam.fps) fps" : "Keine Verbindung")
                        .font(.caption.bold()).monospacedDigit()
                }
                .foregroundStyle(Color.white)
                .padding(.horizontal, 10)
                .padding(.vertical, 5)
                .background(Color.black.opacity(0.55), in: Capsule())
                .padding(10)
            }
        }
    }

    /// Hinweis oder Fehler unter der Vorschau
    @ViewBuilder private var notice: some View {
        if let e = cam.error {
            banner(e, icon: "exclamationmark.triangle.fill", color: .red)
        } else if n.camURL == nil && !cam.running {
            banner("Am PC ist die Kamera aus: Notch › Einstellungen › iPhone › Kamera einschalten.", icon: "info.circle.fill", color: .orange)
        }
    }

    private func banner(_ text: String, icon: String, color: Color) -> some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: icon).foregroundStyle(color)
            Text(text).font(.footnote)
            Spacer(minLength: 0)
        }
        .padding(12)
        .background(color.opacity(0.14), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
    }

    private var startButton: some View {
        Button {
            if cam.running {
                cam.stop()
            } else {
                Task {
                    await n.refreshCam()   // vielleicht gerade erst am PC eingeschaltet
                    if let u = n.camURL { cam.start(u) }
                }
            }
        } label: {
            Label(cam.running ? "Stoppen" : "Starten", systemImage: cam.running ? "stop.fill" : "play.fill")
                .font(.headline)
                .frame(maxWidth: .infinity)
                .padding(.vertical, 6)
        }
        .buttonStyle(.borderedProminent)
        .tint(cam.running ? Color.red : Color.accentColor)
        .controlSize(.large)
    }

    private func card<C: View>(_ title: String, @ViewBuilder _ content: () -> C) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(title).font(.footnote).foregroundStyle(.secondary).textCase(.uppercase)
            content()
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color(.secondarySystemGroupedBackground), in: RoundedRectangle(cornerRadius: 14, style: .continuous))
    }
}
