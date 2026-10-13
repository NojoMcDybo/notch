"""Kaggle-Notebook fuer das Jarvis-Feintuning (docs/jarvis.md, M2b) aus dem Notebook des Laya-Repos bauen.

Uebernimmt GPU-Pruefung, Installation, Trainingsskript (RLCD, 2xT4 DDP) und Start unveraendert und tauscht:
  - Checkpoint: laya-multilingual statt des englischen
  - Daten: train.jsonl aus dem Kaggle-Datensatz (make_train.py) statt LocalLLaMA/typed-decisions
  - Ende: Test gegen saetze.json (nie im Training), Modell als ZIP in /kaggle/working

    python tools/jarvis/make_notebook.py <laya-klon>/notebooks/laya_finetune_typed_decisions_2xT4_kaggle.ipynb <ausgabe.ipynb>
"""
import json
import sys


def code(src):
    return {"cell_type": "code", "metadata": {}, "execution_count": None, "outputs": [], "source": src}


def md(src):
    return {"cell_type": "markdown", "metadata": {}, "source": src}


PREP = r'''import os, json, glob, torch
from transformers import AutoTokenizer
from huggingface_hub import snapshot_download
from laya.agent import _fix_tokenizer_config
from laya.common import build_sequence, render_options, QTYPES

root = snapshot_download("convaiinnovations/laya", allow_patterns=["multilingual/*"])
model_dir = os.path.join(root, "multilingual")
_fix_tokenizer_config(model_dir)
tok = AutoTokenizer.from_pretrained(os.path.join(model_dir, "tokenizer"))
with open(os.path.join(model_dir, "rl_agent_config.json")) as f:
    cfg = json.load(f)

train_path = glob.glob("/kaggle/input/**/train.jsonl", recursive=True)
assert train_path, "train.jsonl nicht gefunden: Datensatz rechts unter 'Input' hinzufuegen"
rows = [json.loads(l) for l in open(train_path[0], encoding="utf-8")]
print(len(rows), "Saetze aus", train_path[0])

def build_training_item(state, q, gold_q):
    crit = q["criteria"]
    keys = list(crit.keys())
    target = [gold_q["probabilities"].get(k, 0.0) for k in keys]
    s = sum(target)
    target = [v / s for v in target]
    k = len(render_options({"t": q["type"], "crit": crit}))
    seq, markers = build_sequence(tok, state, {"t": q["type"], "ins": q["instructions"], "crit": crit}, cfg["max_len"], cfg["head_max_len"])
    if len(markers) != k:
        return None
    return {"ids": seq, "markers": markers, "qtype": QTYPES[q["type"]], "target": target, "label": target.index(max(target))}

items = []
for row in rows:
    state = json.loads(row["state"])
    questions, gold = json.loads(row["questions"]), json.loads(row["gold"])
    for qid, q in questions.items():
        it = build_training_item(state, q, gold[qid])
        if it:
            items.append(it)
print(len(items), "Trainingsfolgen")
torch.save(items, "/kaggle/working/train_items.pt")
QUESTIONS = json.loads(rows[0]["questions"])
'''

TEST = r'''import glob, json, time
import laya

test_path = glob.glob("/kaggle/input/**/saetze.json", recursive=True)
saetze = json.load(open(test_path[0], encoding="utf-8"))["saetze"]
SOLL = {"programm": "starten", "fenster": "wechseln", "seite": "web", "sonstiges": "nichts"}

def check(agent, name):
    ok, t = 0, []
    for s in saetze:
        want = "nichts" if s["zurueck"] and s["app"] == "keins" else SOLL[s["absicht"]]
        a = time.perf_counter(); r = agent.predict(s["t"], QUESTIONS)["answers"]["absicht"]; t.append(time.perf_counter() - a)
        ok += r["choice"] == want
        if name == "nachher":
            print(("  " if r["choice"] == want else "✗ ") + r["choice"].ljust(9), f'{r.get("answer_confidence", 0):.2f}', s["t"])
    print(f"{name}: {ok}/{len(saetze)} richtig, {1000 * sorted(t)[len(t) // 2]:.0f} ms je Satz (GPU)")

check(laya.Agent(model_dir, device="cuda"), "vorher")
check(laya.Agent(OUTPUT_DIR, device="cuda"), "nachher")
'''

ZIP = r'''import shutil, os
shutil.rmtree(os.path.join(OUTPUT_DIR, "checkpoint_latest"), ignore_errors=True)
shutil.make_archive("/kaggle/working/laya-jarvis-de", "zip", OUTPUT_DIR)
print("Fertig: /kaggle/working/laya-jarvis-de.zip", os.path.getsize("/kaggle/working/laya-jarvis-de.zip") // 1_000_000, "MB")
'''


def main():
    src, out = sys.argv[1], sys.argv[2]
    nb = json.load(open(src, encoding="utf-8"))
    cells = nb["cells"]
    train = "".join(cells[8]["source"])
    assert "%%writefile /kaggle/working/train_ddp.py" in train
    train = train.replace('cfg["model_name"] = "laya-typed-decisions"', 'cfg["model_name"] = "laya-jarvis-de"')
    nb["cells"] = [
        md("# Laya fuer Jarvis (Notch): Absicht in deutschen Sprachbefehlen\n\nFeintuning von `laya-multilingual` auf "
           "`train.jsonl` (tools/jarvis/make_train.py), Trainingsskript unveraendert aus dem Laya-Repo "
           "(RLCD, 2xT4 DDP). **Einstellungen rechts:** Accelerator *GPU T4 x2*, Internet *an*, Datensatz mit "
           "`train.jsonl` und `saetze.json` als Input."),
        cells[2],  # GPU-Pruefung
        cells[4],  # Installation
        md("## Daten"),
        code(PREP),
        md("## Trainingsskript (aus dem Laya-Repo)"),
        code(train),
        md("## Training"),
        code('OUTPUT_DIR = "/kaggle/working/laya-jarvis-de"\nMODEL_DIR = model_dir\n'
             'cmd = f"torchrun --standalone --nproc_per_node=2 /kaggle/working/train_ddp.py {MODEL_DIR} {OUTPUT_DIR}"\n'
             'print(cmd)\n!{cmd}\n'),
        md("## Test gegen saetze.json (nie im Training)"),
        code(TEST),
        md("## Modell zum Herunterladen"),
        code(ZIP),
    ]
    json.dump(nb, open(out, "w", encoding="utf-8"), ensure_ascii=False, indent=1)
    print("Notebook:", out)


if __name__ == "__main__":
    main()
