import AppKit
import SwiftUI

// Rendert die Sperrbildschirm-Karte (NotchWidget/LiveCard.swift) als PNG, damit man sie vor dem Laden aufs iPhone ansehen kann.
@MainActor func render(_ name: String, _ card: LiveCard) {
    let v = card.frame(width: 360).background(Color(red: 0.05, green: 0.05, blue: 0.07)).environment(\.colorScheme, .dark)
    let r = ImageRenderer(content: v)
    r.scale = 2
    guard let img = r.nsImage, let tiff = img.tiffRepresentation, let rep = NSBitmapImageRep(data: tiff),
          let png = rep.representation(using: .png, properties: [:]) else { print("FEHLER", name); return }
    try? png.write(to: URL(fileURLWithPath: CommandLine.arguments[1]).appendingPathComponent(name + ".png"))
}

MainActor.assumeIsolated {
    render("senden", LiveCard(kind: "send", title: "Sende 2 von 3", detail: "", progress: 0.42, pc: "NOJO-PC"))
    render("holen", LiveCard(kind: "fetch", title: "Hole 1 von 2", detail: "", progress: 0.8, pc: "NOJO-PC"))
    render("webcam", LiveCard(kind: "camera", title: "Webcam läuft", detail: "30 Bilder pro Sekunde", progress: nil, pc: "NOJO-PC"))
    render("fertig", LiveCard(kind: "done", title: "Fertig", detail: "3 Dateien an den PC gesendet", progress: nil, pc: "NOJO-PC"))
    render("fehler", LiveCard(kind: "error", title: "Fehlgeschlagen", detail: "PC nicht erreichbar", progress: nil, pc: "NOJO-PC"))
}
