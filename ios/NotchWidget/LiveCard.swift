import SwiftUI

/// Farbe und Symbol je Art der Live-Aktivitaet
enum LiveStyle {
    static func icon(_ kind: String) -> String {
        switch kind {
        case "send": return "arrow.up.circle.fill"
        case "fetch": return "arrow.down.circle.fill"
        case "camera": return "video.fill"
        case "error": return "exclamationmark.triangle.fill"
        default: return "checkmark.circle.fill"
        }
    }

    static func tint(_ kind: String) -> Color {
        switch kind {
        case "send": return Color(red: 0.04, green: 0.52, blue: 1.0)
        case "fetch": return Color(red: 0.19, green: 0.82, blue: 0.35)
        case "camera": return Color(red: 1.0, green: 0.27, blue: 0.23)
        case "error": return Color(red: 1.0, green: 0.62, blue: 0.04)
        default: return Color(red: 0.19, green: 0.82, blue: 0.35)
        }
    }

    /// Rechts: Prozent bei Uebertragungen, Bildrate steckt bei der Kamera im Detailtext
    static func trailing(kind: String, progress: Double?) -> String {
        guard let p = progress, kind == "send" || kind == "fetch" else { return "" }
        return "\(Int((min(max(p, 0), 1) * 100).rounded())) %"
    }
}

/// Schmaler Fortschrittsbalken (ProgressView(value:) ist in Live-Aktivitaeten unzuverlaessig)
struct LiveBar: View {
    let value: Double
    let color: Color

    var body: some View {
        GeometryReader { g in
            ZStack(alignment: .leading) {
                Capsule().fill(Color.white.opacity(0.18))
                Capsule().fill(color).frame(width: max(6, g.size.width * min(max(value, 0), 1)))
            }
        }
        .frame(height: 6)
    }
}

/// Sperrbildschirm-Karte. Ohne ActivityKit, damit das CI sie auch auf dem Mac als Bild rendern kann.
struct LiveCard: View {
    let kind: String
    let title: String
    let detail: String
    let progress: Double?
    let pc: String

    var body: some View {
        VStack(spacing: 12) {
            HStack(spacing: 12) {
                Image(systemName: LiveStyle.icon(kind))
                    .font(.system(size: 20, weight: .semibold))
                    .foregroundStyle(Color.white)
                    .frame(width: 40, height: 40)
                    .background(LiveStyle.tint(kind), in: RoundedRectangle(cornerRadius: 11, style: .continuous))
                VStack(alignment: .leading, spacing: 2) {
                    Text(title).font(.system(size: 16, weight: .semibold)).foregroundStyle(Color.white).lineLimit(1)
                    Text(detail.isEmpty ? pc : detail + " · " + pc)
                        .font(.system(size: 12)).foregroundStyle(Color.white.opacity(0.65)).lineLimit(1)
                }
                Spacer(minLength: 8)
                let t = LiveStyle.trailing(kind: kind, progress: progress)
                if !t.isEmpty {
                    Text(t).font(.system(size: 15, weight: .semibold).monospacedDigit()).foregroundStyle(Color.white)
                } else if kind == "camera" {
                    Text("LIVE").font(.system(size: 11, weight: .bold)).foregroundStyle(Color.white)
                        .padding(.horizontal, 8).padding(.vertical, 4)
                        .background(LiveStyle.tint("camera"), in: Capsule())
                }
            }
            if let p = progress, kind == "send" || kind == "fetch" {
                LiveBar(value: p, color: LiveStyle.tint(kind))
            }
        }
        .padding(16)
    }
}
