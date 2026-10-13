# Modelle für „Hey Jarvis“

Aus [openWakeWord](https://github.com/dscripka/openWakeWord), Release v0.5.1, unverändert:

| Datei | SHA-256 |
|---|---|
| `melspectrogram.onnx` | `ba2b0e0f8b7b875369a2c89cb13360ff53bac436f2895cced9f479fa65eb176f` |
| `embedding_model.onnx` | `70d164290c1d095d1d4ee149bc5e00543250a7316b59f31d056cff7bd3075c1f` |
| `hey_jarvis_v0.1.onnx` | `94a13cfe60075b132f6a472e7e462e8123ee70861bc3fb58434a73712ee0d2cb` |

**Lizenz:** Der Code von openWakeWord steht unter Apache-2.0; die vortrainierten Modelle stehen laut Projekt unter
[CC BY-NC-SA 4.0](https://creativecommons.org/licenses/by-nc-sa/4.0/) (Namensnennung: David Scripka, openWakeWord),
also **nur nicht-kommerziell**. Das Einbettungsmodell stammt laut Projekt von Googles Speech-Embedding-Modell.
Soll die Notch verkauft werden: eigenes Wort mit dem Trainings-Notebook von openWakeWord trainieren und diese
Dateien ersetzen.

Nachbau der Pipeline: `src/wake/detector.ts`, Test: `tools/wake-test.mjs`.
