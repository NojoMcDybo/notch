import SwiftUI

// League am iPhone: Spiel annehmen, bannen, picken. Zustand und Befehle laufen ueber den PC (lol_phone.rs),
// Namen und Bilder der Champions kommen von Riots Data Dragon. Aussehen angelehnt an den Client (Gold auf Dunkelblau).

private enum Hex {
    static let bg = Color(red: 0.004, green: 0.039, blue: 0.075)
    static let panel = Color(red: 0.035, green: 0.078, blue: 0.157)
    static let gold = Color(red: 0.784, green: 0.667, blue: 0.431)
    static let goldDark = Color(red: 0.471, green: 0.353, blue: 0.157)
    static let text = Color(red: 0.941, green: 0.902, blue: 0.824)
    static let blue = Color(red: 0.039, green: 0.784, blue: 0.725)
    static let red = Color(red: 0.86, green: 0.25, blue: 0.25)
}

// MARK: Daten

struct LolState: Decodable {
    var phase = "Offline"
    var search: Search?
    var ready: Ready?
    var select: Select?

    struct Search: Decodable { var time: Double; var estimate: Double }
    struct Ready: Decodable { var state: String; var response: String; var elapsed: Double }
    struct Slot: Decodable { var cell: Int; var champ: Int; var intent: Int; var pos: String }
    struct Action: Decodable { var id: Int; var kind: String; var champ: Int }
    struct Select: Decodable {
        var me: Int
        var my: [Slot]
        var their: [Slot]
        var myBans: [Int]
        var theirBans: [Int]
        var action: Action?
        var timer: String
        var left: Double
        var choices: [Int]?
    }
}

/// Champion-Namen und -Bilder (Data Dragon, deutsch). Die PC-Seite kennt nur die Nummern.
@MainActor final class Champs: ObservableObject {
    struct Champ: Identifiable { let id: Int; let name: String; let file: String }
    @Published private(set) var byID: [Int: Champ] = [:]
    private var version = ""

    func load() async {
        guard byID.isEmpty else { return }
        struct Item: Decodable { struct Img: Decodable { var full: String }; var key: String; var name: String; var image: Img }
        struct Doc: Decodable { var data: [String: Item] }
        do {
            let vs = try JSONDecoder().decode([String].self, from: try await URLSession.shared.data(from: URL(string: "https://ddragon.leagueoflegends.com/api/versions.json")!).0)
            guard let v = vs.first else { return }
            let doc = try JSONDecoder().decode(Doc.self, from: try await URLSession.shared.data(from: URL(string: "https://ddragon.leagueoflegends.com/cdn/\(v)/data/de_DE/champion.json")!).0)
            version = v
            byID = Dictionary(uniqueKeysWithValues: doc.data.values.compactMap { i in Int(i.key).map { ($0, Champ(id: $0, name: i.name, file: i.image.full)) } })
        } catch {}
    }

    func icon(_ id: Int) -> URL? {
        byID[id].flatMap { URL(string: "https://ddragon.leagueoflegends.com/cdn/\(version)/img/champion/\($0.file)") }
    }
}

@MainActor final class LolModel: ObservableObject {
    @Published var state = LolState()
    @Published var offline = false
    @Published var chosen: Int?
    @Published var busy = false
    private var actionID: Int?

    /// Fragt den PC ab, solange die Ansicht offen ist (laeuft eine Auswahl, oefter)
    func run(_ n: NotchModel) async {
        while !Task.isCancelled {
            do {
                let new = try JSONDecoder().decode(LolState.self, from: try await n.lol("lol"))
                buzz(old: state, new: new)
                if new.select?.action?.id != actionID { actionID = new.select?.action?.id; chosen = nil }
                state = new
                offline = false
            } catch {
                offline = true
            }
            let live = ["ReadyCheck", "ChampSelect"].contains(state.phase)
            try? await Task.sleep(nanoseconds: live ? 600_000_000 : 2_000_000_000)
        }
    }

    /// kurzes Anstupsen, wenn etwas auf dich wartet
    private func buzz(old: LolState, new: LolState) {
        if new.phase == "ReadyCheck", old.phase != "ReadyCheck" || old.ready?.state != new.ready?.state, new.ready?.response == "None" {
            UINotificationFeedbackGenerator().notificationOccurred(.warning)
        } else if new.select?.action != nil, old.select?.action?.id != new.select?.action?.id {
            UINotificationFeedbackGenerator().notificationOccurred(.success)
        }
    }

    func send(_ n: NotchModel, _ what: String, _ q: [String: String] = [:]) async {
        busy = true
        defer { busy = false }
        _ = try? await n.lol("lol/" + what, q, post: true)
    }

    func pick(_ n: NotchModel, _ id: Int) {
        chosen = id
        guard let a = state.select?.action else { return }
        Task { await send(n, "hover", ["id": String(a.id), "champ": String(id)]) }   // Mitspieler sehen die Vorauswahl
    }

    func lock(_ n: NotchModel) {
        guard let a = state.select?.action, let c = chosen ?? (a.champ > 0 ? a.champ : nil) else { return }
        Task { await send(n, "lock", ["id": String(a.id), "champ": String(c)]) }
    }
}

// MARK: Ansicht

struct LolView: View {
    @EnvironmentObject var n: NotchModel
    @StateObject private var model = LolModel()
    @StateObject private var champs = Champs()
    @State private var search = ""

    var body: some View {
        ZStack {
            Hex.bg.ignoresSafeArea()
            if model.offline {
                message("wifi.slash", "PC nicht erreichbar", "Notch am PC aktuell und im gleichen WLAN?")
            } else {
                content
            }
        }
        .navigationTitle("League")
        .navigationBarTitleDisplayMode(.inline)
        .toolbarBackground(Hex.bg, for: .navigationBar)
        .toolbarBackground(.visible, for: .navigationBar)
        .toolbarColorScheme(.dark, for: .navigationBar)
        .task { await champs.load() }
        .task { await model.run(n) }
        .onAppear { UIApplication.shared.isIdleTimerDisabled = true }
        .onDisappear { UIApplication.shared.isIdleTimerDisabled = false }
    }

    @ViewBuilder private var content: some View {
        let s = model.state
        switch s.phase {
        case "Offline": message("gamecontroller", "League-Client aus", "Starte League am PC und melde dich an.")
        case "Matchmaking": searching(s.search)
        case "ReadyCheck": readyCheck(s.ready)
        case "ChampSelect":
            if let sel = s.select { champSelect(sel) } else { message("person.3", "Championauswahl", "Lädt …") }
        case "InProgress", "GameStart", "Reconnect": message("flame", "Spiel läuft", "Viel Erfolg!")
        case "Lobby", "None": message("moon.zzz", "Kein Spiel in Sicht", "Starte am PC eine Suche, hier erscheint dann alles Weitere.")
        default: message("hourglass", s.phase, "")
        }
    }

    private func message(_ icon: String, _ title: String, _ sub: String) -> some View {
        VStack(spacing: 10) {
            Image(systemName: icon).font(.system(size: 44)).foregroundStyle(Hex.gold)
            Text(title).font(.system(.title3, design: .serif).weight(.bold)).foregroundStyle(Hex.text)
            if !sub.isEmpty { Text(sub).font(.callout).foregroundStyle(Hex.text.opacity(0.6)).multilineTextAlignment(.center) }
        }.padding(32)
    }

    private func clock(_ s: Double) -> String { String(format: "%d:%02d", Int(s) / 60, Int(s) % 60) }

    private func searching(_ s: LolState.Search?) -> some View {
        VStack(spacing: 14) {
            ProgressView().tint(Hex.gold).scaleEffect(1.4)
            Text("Suche läuft").font(.system(.title3, design: .serif).weight(.bold)).foregroundStyle(Hex.gold)
            if let s {
                Text(clock(s.time)).font(.system(size: 40, weight: .semibold, design: .serif)).foregroundStyle(Hex.text)
                if s.estimate > 0 { Text("Geschätzt \(clock(s.estimate))").font(.callout).foregroundStyle(Hex.text.opacity(0.6)) }
            }
        }
    }

    // MARK: Spiel gefunden

    private func readyCheck(_ r: LolState.Ready?) -> some View {
        let left = max(0, 12 - Int((r?.elapsed ?? 0).rounded()))
        let answered = r.map { $0.response != "None" } ?? false
        return VStack(spacing: 24) {
            Text(answered ? (r?.response == "Accepted" ? "Angenommen" : "Abgelehnt") : "Spiel gefunden!")
                .font(.system(size: 30, weight: .bold, design: .serif)).foregroundStyle(Hex.gold).tracking(2)
            ZStack {
                Circle().stroke(Hex.goldDark.opacity(0.4), lineWidth: 6)
                Circle().trim(from: 0, to: CGFloat(left) / 12).stroke(Hex.gold, style: StrokeStyle(lineWidth: 6, lineCap: .round))
                    .rotationEffect(.degrees(-90)).animation(.linear(duration: 0.6), value: left)
                Text(answered ? "…" : "\(left)").font(.system(size: 54, weight: .semibold, design: .serif)).foregroundStyle(Hex.text)
            }.frame(width: 150, height: 150)
            if answered {
                Text(r?.response == "Accepted" ? "Warte auf die anderen Spieler" : "").foregroundStyle(Hex.text.opacity(0.6))
            } else {
                VStack(spacing: 12) {
                    Button("Annehmen") { Task { await model.send(n, "accept") } }.buttonStyle(HexButton())
                    Button("Ablehnen") { Task { await model.send(n, "decline") } }.buttonStyle(HexButton(tint: Hex.red))
                }.padding(.horizontal, 32)
            }
        }
    }

    // MARK: Championauswahl

    private func champSelect(_ s: LolState.Select) -> some View {
        VStack(spacing: 0) {
            header(s)
            ScrollView {
                VStack(spacing: 14) {
                    bans(s)
                    HStack(alignment: .top, spacing: 10) {
                        team(s.my, me: s.me, title: "Dein Team")
                        team(s.their, me: -1, title: "Gegner")
                    }
                }.padding(12)
            }
            if let a = s.action, let all = s.choices { picker(a, all) }
        }
    }

    private func header(_ s: LolState.Select) -> some View {
        let text: String
        switch (s.action?.kind ?? "", s.timer) {
        case ("ban", _): text = "Banne einen Champion"
        case ("pick", _): text = "Wähle deinen Champion"
        case (_, "PLANNING"): text = "Planung"
        case (_, "FINALIZATION"): text = "Letzte Anpassungen"
        case (_, "GAME_STARTING"): text = "Das Spiel startet"
        default: text = "Warte auf die anderen"
        }
        return VStack(spacing: 2) {
            Text(text).font(.system(.headline, design: .serif).weight(.bold)).tracking(1.5).textCase(.uppercase)
                .foregroundStyle(s.action != nil ? Hex.gold : Hex.text.opacity(0.7))
            Text("\(Int(s.left.rounded()))").font(.system(size: 30, weight: .semibold, design: .serif)).foregroundStyle(Hex.text)
        }
        .frame(maxWidth: .infinity).padding(.vertical, 8)
        .background(Hex.panel)
        .overlay(alignment: .bottom) { Rectangle().fill(Hex.goldDark).frame(height: 1) }
    }

    private func bans(_ s: LolState.Select) -> some View {
        HStack {
            banRow(s.myBans)
            Spacer()
            banRow(s.theirBans)
        }
    }

    private func banRow(_ ids: [Int]) -> some View {
        HStack(spacing: 4) {
            ForEach(Array(ids.enumerated()), id: \.offset) { _, id in
                portrait(id, size: 30).saturation(0).overlay(Rectangle().stroke(Hex.red.opacity(0.8), lineWidth: 1))
            }
        }
    }

    private func team(_ slots: [LolState.Slot], me: Int, title: String) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(title).font(.caption.weight(.bold)).tracking(1).textCase(.uppercase).foregroundStyle(Hex.gold)
            ForEach(slots, id: \.cell) { p in
                let id = p.champ > 0 ? p.champ : p.intent
                HStack(spacing: 8) {
                    portrait(id, size: 40).opacity(p.champ > 0 ? 1 : 0.55)
                    VStack(alignment: .leading, spacing: 1) {
                        Text(id > 0 ? (champs.byID[id]?.name ?? "…") : "Wählt …").font(.footnote.weight(.semibold)).foregroundStyle(Hex.text).lineLimit(1)
                        Text(pos(p.pos)).font(.caption2).foregroundStyle(Hex.text.opacity(0.5))
                    }
                    Spacer(minLength: 0)
                }
                .padding(6)
                .background(Hex.panel)
                .overlay(Rectangle().stroke(p.cell == me ? Hex.gold : Hex.goldDark.opacity(0.5), lineWidth: p.cell == me ? 2 : 1))
            }
        }.frame(maxWidth: .infinity)
    }

    private func pos(_ p: String) -> String {
        ["top": "Oben", "jungle": "Dschungel", "middle": "Mitte", "bottom": "Unten", "utility": "Support"][p] ?? ""
    }

    private func portrait(_ id: Int, size: CGFloat) -> some View {
        Group {
            if let u = champs.icon(id) {
                AsyncImage(url: u) { $0.resizable().scaledToFill() } placeholder: { Hex.panel }
            } else {
                ZStack { Hex.panel; Image(systemName: "questionmark").foregroundStyle(Hex.goldDark) }
            }
        }
        .frame(width: size, height: size).clipped()
        .overlay(Rectangle().stroke(Hex.goldDark, lineWidth: 1))
    }

    // MARK: Auswahl am unteren Rand

    private func picker(_ a: LolState.Action, _ all: [Int]) -> some View {
        let ban = a.kind == "ban"
        let list = all.compactMap { champs.byID[$0] }
            .filter { search.isEmpty || $0.name.localizedCaseInsensitiveContains(search) }
            .sorted { $0.name.localizedCompare($1.name) == .orderedAscending }
        let current = model.chosen ?? (a.champ > 0 ? a.champ : nil)
        return VStack(spacing: 8) {
            TextField("Champion suchen", text: $search)
                .padding(8).background(Hex.panel).foregroundStyle(Hex.text)
                .overlay(Rectangle().stroke(Hex.goldDark, lineWidth: 1))
            ScrollView {
                LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: 6), count: 5), spacing: 6) {
                    ForEach(list) { c in
                        Button { model.pick(n, c.id) } label: {
                            VStack(spacing: 2) {
                                portrait(c.id, size: 56).overlay(Rectangle().stroke(current == c.id ? (ban ? Hex.red : Hex.gold) : .clear, lineWidth: 3))
                                Text(c.name).font(.system(size: 9)).foregroundStyle(Hex.text).lineLimit(1)
                            }
                        }.buttonStyle(.plain)
                    }
                }
            }.frame(maxHeight: 230)
            Button(ban ? "Bannen" : "Sperren") { model.lock(n) }
                .buttonStyle(HexButton(tint: ban ? Hex.red : Hex.gold))
                .disabled(current == nil || model.busy)
        }
        .padding(12)
        .background(Hex.bg)
        .overlay(alignment: .top) { Rectangle().fill(Hex.goldDark).frame(height: 1) }
    }
}

struct HexButton: ButtonStyle {
    var tint = Hex.gold
    @Environment(\.isEnabled) private var enabled

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.system(.headline, design: .serif).weight(.bold)).tracking(1.5).textCase(.uppercase)
            .foregroundStyle(Hex.text)
            .frame(maxWidth: .infinity).padding(.vertical, 14)
            .background(Hex.panel)
            .overlay(Rectangle().stroke(LinearGradient(colors: [tint, Hex.goldDark], startPoint: .top, endPoint: .bottom), lineWidth: 2))
            .opacity(enabled ? (configuration.isPressed ? 0.7 : 1) : 0.4)
    }
}
