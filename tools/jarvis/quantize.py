"""Modellpaket fuer Jarvis bauen: feingetuntes Laya (ONNX-Export, fp32) -> INT4 fuer die Notch.

Encoder: MatMul als INT4 (MatMulNBits, Block 32, symmetrisch), die Einbettungstabelle (256000 x 768) aber als int8
je Zeile aus einfachen Bausteinen: Gather(int8) -> Cast -> Mul(Gather(Skala)). Grund: GatherBlockQuantized (die
INT4-Tabelle) rechnet in onnxruntime-web 1.30 auf WebGPU/JSEP falsch (docs/jarvis.md 2d), MatMulNBits stimmt.
Head: wie bisher MatMul + Gather INT4 (laeuft in der Notch auf WASM, dort stimmt beides).

  python -I tools/jarvis/quantize.py <export-ordner fp32> <ziel-ordner>
Danach Paket mit den Release-Namen aus src-tauri/src/laya.rs anlegen und Groessen/SHA-256 dort eintragen.
"""
import os
import shutil
import sys

import numpy as np
import onnx
from onnx import TensorProto, helper, numpy_helper
from onnxruntime.quantization.matmul_nbits_quantizer import DefaultWeightOnlyQuantConfig, MatMulNBitsQuantizer


def int4(model, ops):
    axes = tuple((o, 0 if o == "MatMul" else 1) for o in ops)
    cfg = DefaultWeightOnlyQuantConfig(block_size=32, is_symmetric=True, accuracy_level=4, op_types_to_quantize=ops, quant_axes=axes)
    q = MatMulNBitsQuantizer(model, algo_config=cfg)
    q.process()
    return q.model.model


def embedding_int8(model):
    g = model.graph
    inits = {i.name: i for i in g.initializer}
    done = 0
    for n in list(g.node):
        if n.op_type != "Gather" or n.input[0] not in inits:
            continue
        w = numpy_helper.to_array(inits[n.input[0]])
        if w.ndim != 2 or w.shape[0] < 10000 or w.dtype != np.float32:
            continue
        scale = np.abs(w).max(axis=1, keepdims=True) / 127.0
        scale[scale == 0] = 1.0
        q = np.clip(np.round(w / scale), -127, 127).astype(np.int8)
        err = np.abs(q.astype(np.float32) * scale - w).max()
        assert err < 0.01, f"Einbettung: Rundungsfehler {err}"
        print(f"{n.input[0]} {w.shape}: int8 je Zeile, max. Fehler {err:.5f}")
        b = n.input[0].replace(".", "_")
        g.initializer.remove(inits[n.input[0]])
        g.initializer.extend([numpy_helper.from_array(q, b + "_i8"), numpy_helper.from_array(scale.astype(np.float32), b + "_scale")])
        new = [
            helper.make_node("Gather", [b + "_i8", n.input[1]], [b + "_gq"], axis=0, name=b + "_gather_i8"),
            helper.make_node("Cast", [b + "_gq"], [b + "_gf"], to=TensorProto.FLOAT, name=b + "_cast"),
            helper.make_node("Gather", [b + "_scale", n.input[1]], [b + "_gs"], axis=0, name=b + "_gather_scale"),
            helper.make_node("Mul", [b + "_gf", b + "_gs"], [n.output[0]], name=b + "_mul"),
        ]
        pos = list(g.node).index(n)
        g.node.remove(n)
        for k, nn in enumerate(new):
            g.node.insert(pos + k, nn)
        done += 1
    assert done == 1, f"{done} Einbettungstabellen gefunden, erwartet 1"
    return model


def main(src, dst):
    os.makedirs(dst, exist_ok=True)
    for f in ("tokenizer.json", "rl_agent_config.json"):
        shutil.copy(os.path.join(src, f), dst)
    enc = int4(onnx.load(os.path.join(src, "encoder.onnx")), ("MatMul",))
    enc = embedding_int8(enc)
    del enc.graph.value_info[:]
    onnx.checker.check_model(enc)
    onnx.save_model(enc, os.path.join(dst, "encoder.onnx"), save_as_external_data=False)
    head = int4(onnx.load(os.path.join(src, "head.onnx")), ("MatMul", "Gather"))
    onnx.save_model(head, os.path.join(dst, "head.onnx"), save_as_external_data=False)
    for f in sorted(os.listdir(dst)):
        print(f, os.path.getsize(os.path.join(dst, f)) // 1_000_000, "MB")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
