import HealthKit
import SwiftUI

// Health-Export: liest Apple Health (Zepp/Amazfit, Essens-App, iPhone …) und schickt neue Werte an die Notch am PC
// (`POST health`, siehe src-tauri/src/health.rs und docs/health-export.md). Nur lesen, nie schreiben.
// Neu und geaendert wird ueber Anker erkannt (je Wertart gemerkt); geloeschte Werte werden nicht nachgezogen.

private struct Q {
    let key: String
    let id: HKQuantityTypeIdentifier
    let unit: HKUnit
    let name: String
    var scale = 1.0   // Prozent liefert Health als 0…1

    init(_ key: String, _ id: HKQuantityTypeIdentifier, _ unit: HKUnit, _ name: String, scale: Double = 1) {
        self.key = key; self.id = id; self.unit = unit; self.name = name; self.scale = scale
    }
}

private let bpm = HKUnit.count().unitDivided(by: .minute())
private let gram = HKUnit.gram()
private let mg = HKUnit.gramUnit(with: .milli)
private let kg = HKUnit.gramUnit(with: .kilo)

private let quantities: [Q] = [
    // Bewegung
    Q("stepCount", .stepCount, .count(), "count"),
    Q("distanceWalkingRunning", .distanceWalkingRunning, .meter(), "m"),
    Q("distanceCycling", .distanceCycling, .meter(), "m"),
    Q("distanceSwimming", .distanceSwimming, .meter(), "m"),
    Q("flightsClimbed", .flightsClimbed, .count(), "count"),
    Q("activeEnergyBurned", .activeEnergyBurned, .kilocalorie(), "kcal"),
    Q("basalEnergyBurned", .basalEnergyBurned, .kilocalorie(), "kcal"),
    Q("exerciseTime", .appleExerciseTime, .minute(), "min"),
    Q("standTime", .appleStandTime, .minute(), "min"),
    // Herz, Atmung, Koerper
    Q("heartRate", .heartRate, bpm, "count/min"),
    Q("restingHeartRate", .restingHeartRate, bpm, "count/min"),
    Q("walkingHeartRateAverage", .walkingHeartRateAverage, bpm, "count/min"),
    Q("heartRateVariabilitySDNN", .heartRateVariabilitySDNN, .secondUnit(with: .milli), "ms"),
    Q("heartRateRecoveryOneMinute", .heartRateRecoveryOneMinute, bpm, "count/min"),
    Q("oxygenSaturation", .oxygenSaturation, .percent(), "%", scale: 100),
    Q("respiratoryRate", .respiratoryRate, bpm, "count/min"),
    Q("bodyTemperature", .bodyTemperature, .degreeCelsius(), "degC"),
    Q("wristTemperature", .appleSleepingWristTemperature, .degreeCelsius(), "degC"),
    Q("vo2Max", .vo2Max, HKUnit(from: "ml/(kg*min)"), "ml/kg/min"),
    Q("bloodPressureSystolic", .bloodPressureSystolic, .millimeterOfMercury(), "mmHg"),
    Q("bloodPressureDiastolic", .bloodPressureDiastolic, .millimeterOfMercury(), "mmHg"),
    Q("bloodGlucose", .bloodGlucose, HKUnit(from: "mg/dL"), "mg/dL"),
    Q("insulinDelivery", .insulinDelivery, .internationalUnit(), "IU"),
    Q("bodyMass", .bodyMass, kg, "kg"),
    Q("height", .height, .meter(), "m"),
    Q("bodyMassIndex", .bodyMassIndex, .count(), "kg/m2"),
    Q("bodyFatPercentage", .bodyFatPercentage, .percent(), "%", scale: 100),
    Q("leanBodyMass", .leanBodyMass, kg, "kg"),
    Q("waistCircumference", .waistCircumference, .meter(), "m"),
    // Gang und Umwelt
    Q("walkingSpeed", .walkingSpeed, HKUnit(from: "m/s"), "m/s"),
    Q("walkingStepLength", .walkingStepLength, .meter(), "m"),
    Q("walkingAsymmetry", .walkingAsymmetryPercentage, .percent(), "%", scale: 100),
    Q("walkingDoubleSupport", .walkingDoubleSupportPercentage, .percent(), "%", scale: 100),
    Q("walkingSteadiness", .appleWalkingSteadiness, .percent(), "%", scale: 100),
    Q("sixMinuteWalkDistance", .sixMinuteWalkTestDistance, .meter(), "m"),
    Q("environmentalAudioExposure", .environmentalAudioExposure, .decibelAWeightedSoundPressureLevel(), "dBASPL"),
    Q("headphoneAudioExposure", .headphoneAudioExposure, .decibelAWeightedSoundPressureLevel(), "dBASPL"),
    // Ernaehrung
    Q("dietaryEnergy", .dietaryEnergyConsumed, .kilocalorie(), "kcal"),
    Q("dietaryProtein", .dietaryProtein, gram, "g"),
    Q("dietaryCarbohydrates", .dietaryCarbohydrates, gram, "g"),
    Q("dietaryFat", .dietaryFatTotal, gram, "g"),
    Q("dietaryFatSaturated", .dietaryFatSaturated, gram, "g"),
    Q("dietaryFatMonounsaturated", .dietaryFatMonounsaturated, gram, "g"),
    Q("dietaryFatPolyunsaturated", .dietaryFatPolyunsaturated, gram, "g"),
    Q("dietarySugar", .dietarySugar, gram, "g"),
    Q("dietaryFiber", .dietaryFiber, gram, "g"),
    Q("dietaryCholesterol", .dietaryCholesterol, mg, "mg"),
    Q("dietarySodium", .dietarySodium, mg, "mg"),
    Q("dietaryPotassium", .dietaryPotassium, mg, "mg"),
    Q("dietaryCalcium", .dietaryCalcium, mg, "mg"),
    Q("dietaryIron", .dietaryIron, mg, "mg"),
    Q("dietaryVitaminC", .dietaryVitaminC, mg, "mg"),
    Q("dietaryVitaminD", .dietaryVitaminD, .gramUnit(with: .micro), "mcg"),
    Q("dietaryWater", .dietaryWater, .literUnit(with: .milli), "mL"),
    Q("dietaryCaffeine", .dietaryCaffeine, mg, "mg"),
    Q("alcoholicBeverages", .numberOfAlcoholicBeverages, .count(), "count"),
]

private let byIdentifier = Dictionary(uniqueKeysWithValues: quantities.map { ($0.id.rawValue, $0) })

/// alles, was gelesen wird: (Schluessel im Export, Health-Typ)
private let sampleTypes: [(key: String, type: HKSampleType)] =
    quantities.map { (key: $0.key, type: HKQuantityType($0.id) as HKSampleType) } + [
        (key: "sleep", type: HKCategoryType(.sleepAnalysis) as HKSampleType),
        (key: "mindfulSession", type: HKCategoryType(.mindfulSession) as HKSampleType),
        (key: "workout", type: HKObjectType.workoutType() as HKSampleType),
    ]

// MARK: Umwandlung in JSON

private let iso: ISO8601DateFormatter = {
    let f = ISO8601DateFormatter()
    f.timeZone = .current
    f.formatOptions = [.withInternetDateTime]
    return f
}()

private let dayFmt: DateFormatter = {
    let f = DateFormatter()
    f.locale = Locale(identifier: "en_US_POSIX")
    f.calendar = Calendar(identifier: .gregorian)
    f.timeZone = .current
    f.dateFormat = "yyyy-MM-dd"
    return f
}()

private func clean(_ m: [String: Any]) -> [String: Any] {
    var o = [String: Any]()
    for (k, v) in m {
        switch v {
        case let s as String: o[k] = s
        case let d as Date: o[k] = iso.string(from: d)
        case let q as HKQuantity: o[k] = q.description
        case let n as NSNumber: o[k] = n
        default: break
        }
    }
    return o
}

/// `endDay`: Schlaf zaehlt zu dem Tag, an dem man aufwacht
private func base(_ s: HKSample, _ key: String, endDay: Bool = false) -> [String: Any] {
    var r: [String: Any] = [
        "id": s.uuid.uuidString, "t": key,
        "s": iso.string(from: s.startDate), "e": iso.string(from: s.endDate),
        "day": dayFmt.string(from: endDay ? s.endDate : s.startDate),
        "src": s.sourceRevision.source.name,
    ]
    if let d = s.device { r["dev"] = [d.name, d.model].compactMap { $0 }.joined(separator: " ") }
    if let m = s.metadata { let c = clean(m); if !c.isEmpty { r["m"] = c } }
    return r
}

private func workoutName(_ t: HKWorkoutActivityType) -> String {
    switch t {
    case .running: return "running"
    case .walking: return "walking"
    case .cycling: return "cycling"
    case .swimming: return "swimming"
    case .hiking: return "hiking"
    case .yoga: return "yoga"
    case .traditionalStrengthTraining: return "strength"
    case .functionalStrengthTraining: return "functionalStrength"
    case .highIntensityIntervalTraining: return "hiit"
    case .elliptical: return "elliptical"
    case .rowing: return "rowing"
    default: return "type\(t.rawValue)"
    }
}

private func record(_ s: HKSample) -> [String: Any]? {
    if let q = s as? HKQuantitySample, let d = byIdentifier[q.quantityType.identifier] {
        var r = base(s, d.key)
        r["v"] = q.quantity.doubleValue(for: d.unit) * d.scale
        r["u"] = d.name
        return r
    }
    if let c = s as? HKCategorySample {
        if c.categoryType.identifier == HKCategoryTypeIdentifier.sleepAnalysis.rawValue {
            var r = base(s, "sleep", endDay: true)
            r["v"] = c.value
            r["vl"] = [0: "inBed", 1: "asleep", 2: "awake", 3: "core", 4: "deep", 5: "rem"][c.value] ?? "unknown"
            return r
        }
        var r = base(s, "mindfulSession")
        r["v"] = c.endDate.timeIntervalSince(c.startDate)
        r["u"] = "s"
        return r
    }
    if let w = s as? HKWorkout {
        var r = base(s, "workout")
        r["v"] = w.duration
        r["u"] = "s"
        r["wt"] = workoutName(w.workoutActivityType)
        if let kcal = w.statistics(for: HKQuantityType(.activeEnergyBurned))?.sumQuantity()?.doubleValue(for: .kilocalorie()) { r["kcal"] = kcal }
        if let dist = w.totalDistance?.doubleValue(for: .meter()) { r["dist"] = dist }
        return r
    }
    return nil
}

// MARK: Abgleich

struct HealthSource: Identifiable { let id: String; let sources: [String] }

@MainActor final class HealthSync: ObservableObject {
    static let shared = HealthSync()
    private let store = HKHealthStore()
    private let batch = 3000

    @Published var status = ""
    @Published var running = false
    @Published var sources: [HealthSource] = []
    @Published var last = UserDefaults.standard.object(forKey: "health.last") as? Date
    @Published var asked = UserDefaults.standard.bool(forKey: "health.asked")

    static var available: Bool { HKHealthStore.isHealthDataAvailable() }

    /// Ohne den Eintrag im Profil (kostenloses Entwicklerkonto?) stuerzt die Abfrage ab, darum vorher nachsehen
    static var entitled: Bool {
        guard let p = Bundle.main.path(forResource: "embedded", ofType: "mobileprovision"),
              let d = FileManager.default.contents(atPath: p) else { return true }   // ohne Profil (z. B. TestFlight) vorhanden
        return String(data: d, encoding: .isoLatin1)?.contains("com.apple.developer.healthkit") ?? false
    }

    func connect() async {
        guard Self.available, Self.entitled else { return }
        do {
            try await store.requestAuthorization(toShare: Set<HKSampleType>(), read: Set(sampleTypes.map { $0.type as HKObjectType }))
            UserDefaults.standard.set(true, forKey: "health.asked")
            asked = true
            await loadSources()
        } catch {
            status = "Health: \(error.localizedDescription)"
        }
    }

    /// Beim Oeffnen der App, hoechstens alle 5 Minuten
    func auto(_ n: NotchModel) async {
        guard UserDefaults.standard.object(forKey: "health.auto") as? Bool ?? true, asked,
              Date().timeIntervalSince(last ?? .distantPast) > 300 else { return }
        await sync(n)
    }

    func sync(_ n: NotchModel) async {
        guard !running, asked, Self.available, Self.entitled else { return }
        running = true
        defer { running = false }
        let defaults = UserDefaults.standard
        let since = defaults.object(forKey: "health.since") as? Date ?? Date().addingTimeInterval(-180 * 86400)
        defaults.set(since, forKey: "health.since")
        var added = 0
        for (key, type) in sampleTypes {
            status = "Sende \(key) …"
            do {
                added += try await syncType(type, since: since, n)
            } catch let e as NSError where e.domain == HKErrorDomain && e.code == HKError.errorDatabaseInaccessible.rawValue {
                status = "iPhone entsperren, dann nochmal"
                return
            } catch {
                status = "\(key): \(error.localizedDescription)"
                return
            }
        }
        last = Date()
        defaults.set(last, forKey: "health.last")
        status = added == 0 ? "Alles aktuell" : "\(added) neue Werte am PC"
    }

    private func syncType(_ t: HKSampleType, since: Date, _ n: NotchModel) async throws -> Int {
        let key = "health.anchor." + t.identifier
        var anchor = UserDefaults.standard.data(forKey: key).flatMap { try? NSKeyedUnarchiver.unarchivedObject(ofClass: HKQueryAnchor.self, from: $0) }
        var added = 0
        while true {
            let (samples, next) = try await fetch(t, anchor, since)
            let recs = samples.compactMap(record)
            if !recs.isEmpty {
                let resp = try await n.api("health", body: try JSONSerialization.data(withJSONObject: ["records": recs]))
                added += (try? JSONSerialization.jsonObject(with: resp) as? [String: Any])?["added"] as? Int ?? 0
            }
            // Anker erst nach erfolgreichem Senden merken, sonst gingen Werte verloren
            if let next, let d = try? NSKeyedArchiver.archivedData(withRootObject: next, requiringSecureCoding: true) {
                UserDefaults.standard.set(d, forKey: key)
            }
            anchor = next
            if samples.count < batch { return added }
        }
    }

    private func fetch(_ t: HKSampleType, _ anchor: HKQueryAnchor?, _ since: Date) async throws -> ([HKSample], HKQueryAnchor?) {
        try await withCheckedThrowingContinuation { c in
            let q = HKAnchoredObjectQuery(type: t, predicate: HKQuery.predicateForSamples(withStart: since, end: nil),
                                          anchor: anchor, limit: batch) { _, samples, _, newAnchor, error in
                if let error { c.resume(throwing: error) } else { c.resume(returning: (samples ?? [], newAnchor)) }
            }
            store.execute(q)
        }
    }

    /// Alles neu senden (der PC ignoriert, was er schon hat)
    func reset() {
        let d = UserDefaults.standard
        for (_, t) in sampleTypes { d.removeObject(forKey: "health.anchor." + t.identifier) }
        d.removeObject(forKey: "health.last")
        last = nil
        status = "Beim nächsten Abgleich wird alles neu gesendet"
    }

    /// Welche Quellen (Zepp, iPhone, Essens-App …) liefern was: zeigt, was ueberhaupt in Health ankommt
    func loadSources() async {
        guard Self.available, Self.entitled, asked else { return }
        var rows: [HealthSource] = []
        for (key, type) in sampleTypes {
            let names: [String] = await withCheckedContinuation { c in
                let q = HKSourceQuery(sampleType: type, samplePredicate: nil) { _, found, _ in
                    c.resume(returning: (found ?? []).map { $0.name }.sorted())
                }
                store.execute(q)
            }
            if !names.isEmpty { rows.append(HealthSource(id: key, sources: names)) }
        }
        sources = rows
    }
}

// MARK: Ansicht

struct HealthView: View {
    @EnvironmentObject var n: NotchModel
    @ObservedObject private var h = HealthSync.shared
    @AppStorage("health.auto") private var auto = true

    var body: some View {
        List {
            Section {
                if !HealthSync.available {
                    Text("Health ist auf diesem Gerät nicht verfügbar.")
                } else if !HealthSync.entitled {
                    Text("Diese Installation darf nicht auf Health zugreifen: Das Profil enthält die Health-Berechtigung nicht (kostenloses Entwicklerkonto?).")
                        .foregroundStyle(.orange)
                } else if !h.asked {
                    Button { Task { await h.connect() } } label: { Label("Mit Health verbinden", systemImage: "heart.text.square") }
                } else {
                    Button { Task { await h.sync(n) } } label: { Label("Jetzt abgleichen", systemImage: "arrow.triangle.2.circlepath") }
                        .disabled(h.running)
                    Toggle("Beim Öffnen automatisch", isOn: $auto)
                }
            } header: {
                Text("Apple Health → PC")
            } footer: {
                VStack(alignment: .leading, spacing: 4) {
                    if h.running { ProgressView() }
                    if !h.status.isEmpty { Text(h.status) }
                    if let l = h.last { Text("Zuletzt: " + l.formatted(date: .abbreviated, time: .shortened)) }
                    Text("Die letzten 180 Tage beim ersten Mal, danach nur Neues. Am PC unter Dokumente › Notch › Health. Geschrieben wird nie etwas in Health.")
                }
            }
            if !h.sources.isEmpty {
                Section {
                    ForEach(h.sources) { row in
                        VStack(alignment: .leading, spacing: 2) {
                            Text(row.id).font(.callout)
                            Text(row.sources.joined(separator: ", ")).font(.caption).foregroundStyle(.secondary)
                        }
                    }
                } header: {
                    Text("Was in Health ankommt")
                } footer: {
                    Text("Je Wertart die Quellen. Fehlt hier etwas, das die Zepp-App zeigt, legt Zepp es nicht in Health ab.")
                }
            }
            if h.asked {
                Section {
                    Button("Alles neu senden", role: .destructive) { h.reset() }
                }
            }
        }
        .navigationTitle("Health-Export")
        .task { await h.loadSources() }
    }
}
