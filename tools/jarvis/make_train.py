"""Trainingssatz fuer den Jarvis-Frueherkenner (docs/jarvis.md, M2b): eine Frage `absicht` an Laya.

Erzeugt train.jsonl im Format des Laya-Feintuning-Notebooks (state / questions / gold mit Wahrscheinlichkeiten).
Saetze aus Vorlagen mit echten App-Namen, Zwecken, Suchbegriffen; dazu bloße Erwaehnungen, Verneinungen,
Fragen ohne PC-Bezug und abgeschnittene Anfaenge. Die Testsaetze aus saetze.json kommen nie hinein.

    python tools/jarvis/make_train.py [ausgabe.jsonl]
"""
import json
import random
import sys
from pathlib import Path

HERE = Path(__file__).parent

# Die Frage, wie sie spaeter auch die Notch stellt (main.ts / eval.mjs muessen dieselbe nehmen)
FRAGE = {
    "absicht": {
        "type": "choice",
        "instructions": "Was will der Sprecher gerade vom PC?",
        "criteria": {
            "starten": "ein Programm jetzt öffnen oder starten",
            "wechseln": "zu einem schon offenen Fenster wechseln oder es nach vorn holen",
            "web": "eine Webseite öffnen oder im Internet suchen",
            "nichts": "nichts davon: eine Frage, eine Bemerkung, Musik, Timer, oder etwas zurücknehmen",
        },
    }
}

APPS = [
    "Chrome", "Google Chrome", "Firefox", "Edge", "Spotify", "Discord", "Steam", "League", "League of Legends", "Explorer",
    "den Explorer", "Editor", "den Editor", "Haze", "Notch", "Folio", "Arena", "Vantage", "Helio", "Obsidian", "Notion",
    "Telegram", "Signal", "Outlook", "Word", "Excel", "PowerPoint", "Teams", "Thunderbird", "Blender", "DaVinci Resolve",
    "Epic Games", "den Epic Games Launcher", "Minecraft", "Valorant", "Anki", "Paint", "den Rechner", "Visual Studio Code",
    "VS Code", "das Terminal", "die Einstellungen", "Sonos", "OneNote", "Riot Client", "Cyberpunk", "Terraria", "Skype",
    "WhatsApp", "Logitech G Hub", "TeamSpeak", "Perplexity", "ChatGPT", "DeepL", "FileZilla", "OrcaSlicer", "Fusion",
    "die Kamera-App", "YouTube Music", "Clipchamp", "Overwolf", "GitHub Desktop", "Battle.net",
]
ZWECK = [
    "nach Flügen nach Lissabon zu suchen", "was zu schreiben", "mit Max zu reden", "eine Runde zu spielen",
    "meine Mails zu checken", "die Präsentation fertig zu machen", "Musik zu hören", "das Projekt zu öffnen",
    "kurz was nachzuschauen", "das Video zu schneiden", "meine Notizen durchzugehen", "zu lernen",
]
UND = [
    "such nach dem neuen Album von Linkin Park", "spiel meine Playlist", "schreib Max", "zeig mir die Downloads",
    "öffne die letzte Datei", "mach eine neue Notiz", "starte ein Match", "geh in den Sprachkanal",
]
SUCHE = [
    "dem Wetter in Berlin", "Rezepten für Lasagne", "Flügen nach Lissabon", "der Bundesliga-Tabelle", "Gladbach gegen Köln",
    "einem guten Laptop unter 1000 Euro", "Öffnungszeiten vom Baumarkt", "dem Rust Borrow Checker", "Tauri Fenster transparent",
    "Zugverbindungen nach Hamburg", "dem Patch 26.20", "Kinoprogramm heute", "Mittel gegen Kopfschmerzen",
]
SUCHE_NACKT = [s.replace("dem ", "").replace("der ", "").replace("einem ", "").replace("Rezepten", "Rezepte").replace("Flügen", "Flüge")
               .replace("Öffnungszeiten vom", "Öffnungszeiten") for s in SUCHE]
SEITEN = ["github.com", "YouTube", "Reddit", "Amazon", "kicker.de", "Wikipedia", "die Tagesschau", "Netflix", "Twitch",
          "spiegel.de", "die Seite von der Bahn", "mein Postfach im Browser", "Google Maps", "Stack Overflow", "Hugging Face"]
SONST = [
    "Wie spät ist es", "Mach die Musik lauter", "Leiser bitte", "Nächstes Lied", "Pausier die Musik", "Spiel weiter",
    "Stell einen Timer auf {n} Minuten", "Starte einen Timer für {n} Minuten", "Starte einen Pomodoro", "Starte den Timer",
    "Stopp den Timer", "Wie steht es bei Gladbach", "Wer hat gestern gewonnen", "Erzähl mir einen Witz", "Was hab ich heute noch vor",
    "Wie ist mein Blutzucker", "Wie wird das Wetter morgen", "Danke dir", "Okay, passt", "Was kannst du alles",
    "Wie lange läuft der Timer noch", "Wie heißt das Lied", "Spiel was von Linkin Park", "Mach mal Musik an", "Lauter",
    "Wie viel Uhr haben wir", "Erinner mich in zehn Minuten", "Was ist die Hauptstadt von Portugal", "Rechne 17 mal 23",
    "Wie geht's dir", "Hörst du mich", "Ich bin gleich wieder da", "Wie viele CS brauche ich noch", "Wann spawnt der Drache",
    "Was steht in meiner Ablage", "Wandle die Datei in PDF um", "Wie hoch ist mein Puls",
]
ERWAEHNT = [
    "{app} ist echt teuer geworden", "Ich hab {app} gestern deinstalliert", "Warum ist {app} so langsam",
    "Hast du {app} schon mal benutzt", "{app} hat schon wieder ein Update", "Ich mag {app} nicht",
    "Wie viel kostet {app} eigentlich", "{app} stürzt ständig ab", "Max hat mir {app} empfohlen",
    "Ist {app} besser als die Alternative", "Mein Bruder spielt die ganze Zeit {app}", "Gestern lief {app} ewig",
    "Kennst du {app}", "Ich überlege, {app} zu kündigen", "{app} nervt heute",
]
NEIN = [
    "Öffne nicht {app}", "Mach {app} nicht auf", "Bitte nicht {app} starten", "Starte {app}, ach nee, lass mal",
    "Öffne {app}, nein, doch nicht", "Kein {app} bitte", "Lass {app} zu", "Nein, {app} nicht", "Schließ {app}",
    "Mach {app} zu", "Beende {app}", "{app} brauch ich doch nicht",
]
STARTEN = [
    "Öffne {app}", "Öffne mal {app}", "Öffne mir {app}", "Mach {app} auf", "Mach mal {app} auf", "Mach {app} an",
    "Mach mal {app} an", "Starte {app}", "Starte mal {app}", "Kannst du {app} öffnen", "Kannst du bitte {app} starten",
    "Könntest du {app} aufmachen", "{app} bitte", "{app} öffnen", "{app} starten", "Ich brauch {app}", "Ich brauche mal {app}",
    "Hey, öffne {app}", "Bitte öffne {app}", "Wirf mal {app} an", "Lass uns {app} aufmachen", "Öffne {app}, um {zweck}",
    "Mach {app} auf, ich will {zweck2}", "Starte {app} und {und}", "Öffne {app} und {und}", "Ich will {app} spielen, mach das mal auf",
    "Kannst du mir {app} aufmachen", "Hol mir {app} auf den Bildschirm", "{app}, bitte starten", "Ich muss in {app}, mach das mal auf",
]
WECHSELN = [
    "Wechsel zu {app}", "Geh zu {app}", "Hol {app} nach vorn", "Hol {app} nach vorne", "Zeig mir {app}",
    "Zeig mir das {app}-Fenster", "Bring {app} in den Vordergrund", "Zurück zu {app}", "Geh zurück zu {app}",
    "Spring zu {app}", "Wechsel mal rüber zu {app}", "Ich will wieder in {app}", "Bring mich zu {app}",
]
WEB = [
    "Such nach {s}", "Such mal nach {s}", "Google nach {s}", "Google mal {sn}", "Such im Internet nach {s}",
    "Schau mal im Netz nach {s}", "Öffne {seite}", "Geh auf {seite}", "Mach {seite} auf", "Zeig mir {sn} im Browser",
    "Such mir {sn}", "Öffne die Seite {seite}", "Kannst du nach {s} googeln", "Ruf {seite} auf",
]
VERB_ANFANG = ["Öffne", "Mach mal", "Starte", "Kannst du", "Kannst du bitte", "Hol mal", "Wechsel zu", "Such nach", "Mach", "Bitte"]


def zweck2(z):
    return z.replace(" zu ", " ").replace("zu ", "") if z.endswith("zu") is False else z


def clean(s):
    s = s[0].upper() + s[1:]
    return " ".join(s.split())


def soft(label, keys, p=0.9):
    rest = (1 - p) / (len(keys) - 1)
    return {k: (p if k == label else rest) for k in keys}


def main():
    rnd = random.Random(20261013)
    out = Path(sys.argv[1]) if len(sys.argv) > 1 else HERE / "train.jsonl"
    test = {s["t"].lower() for s in json.loads((HERE / "saetze.json").read_text(encoding="utf-8"))["saetze"]}
    keys = list(FRAGE["absicht"]["criteria"])
    rows = {}

    def add(text, label, p=0.9, cut=False):
        t = clean(text)
        if t.lower() in test or t.lower().rstrip("?.!") in test:
            return
        # Satzzeichen variieren wie eine Spracherkennung (mal mit, mal ohne); abgeschnittene Saetze nie
        if not cut and rnd.random() < 0.3:
            t += rnd.choice([".", "?", "!"])
        rows[t] = soft(label, keys, p)

    app = lambda: rnd.choice(APPS)
    # ohne Artikel ("Kein Rechner", nicht "Kein den Rechner")
    def bare():
        a = app()
        return a.split(" ", 1)[1] if a.split(" ", 1)[0] in ("den", "die", "das") else a
    for _ in range(1400):
        tpl = rnd.choice(STARTEN)
        z = rnd.choice(ZWECK)
        add(tpl.format(app=app(), zweck=z, zweck2=z.replace(" zu ", " ").replace("zu ", ""), und=rnd.choice(UND)), "starten")
    for _ in range(500):
        add(rnd.choice(WECHSELN).format(app=app()), "wechseln")
    for _ in range(500):
        i = rnd.randrange(len(SUCHE))
        add(rnd.choice(WEB).format(s=SUCHE[i], sn=SUCHE_NACKT[i], seite=rnd.choice(SEITEN)), "web")
    for _ in range(500):
        add(rnd.choice(ERWAEHNT).format(app=bare()), "nichts")
    for _ in range(450):
        add(rnd.choice(NEIN).format(app=bare()), "nichts")
    for s in SONST:
        for n in (3, 5, 10, 15, 25):
            add(s.format(n=n), "nichts")
    # abgeschnittene Anfaenge: das Verb traegt die Absicht schon, der Rest fehlt noch
    for _ in range(350):
        a = app()
        # nur Vorlagen, die mit dem Verb beginnen: "Clipcham" allein ist noch keine Absicht
        full = rnd.choice([t for t in STARTEN[:17] if not t.startswith("{app}")]).format(app=a)
        cut = full[: max(4, int(len(full) * rnd.uniform(0.55, 0.9)))]
        add(cut, "starten", 0.75, cut=True)
    for v in VERB_ANFANG:
        add(v, "nichts", 0.6)  # noch nichts gesagt -> nichts tun

    items = list(rows.items())
    rnd.shuffle(items)
    with out.open("w", encoding="utf-8") as f:
        for i, (text, probs) in enumerate(items):
            f.write(json.dumps({"id": f"j{i}", "state": json.dumps(text, ensure_ascii=False),
                                "questions": json.dumps(FRAGE, ensure_ascii=False),
                                "gold": json.dumps({"absicht": {"probabilities": probs}})}, ensure_ascii=False) + "\n")
    counts = {}
    for _, p in items:
        k = max(p, key=p.get)
        counts[k] = counts.get(k, 0) + 1
    print(f"{len(items)} Saetze -> {out}  {counts}")


if __name__ == "__main__":
    main()
