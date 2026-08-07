#!/usr/bin/env python3
"""FF-C9 — nenhum crate do BARG depende de crate do produto.

A fronteira da ADR-0001 é 'pesquisa × produto', não 'Rust × Swift'. Ela só é
fronteira se for verificável: sem isto, a violação aparece em dois lugares e
ninguém sabe qual é o certo.
"""
import json, subprocess, sys

PROIBIDOS = {"drumhud-engine", "drumhud-synth", "drumhud-ffi", "casa13-engine", "casa13-ffi"}

meta = json.loads(subprocess.run(
    ["cargo", "metadata", "--format-version", "1", "--no-deps"],
    capture_output=True, text=True, check=True).stdout)

faltas = []
locais = {p["name"] for p in meta["packages"]}
for p in meta["packages"]:
    for d in p["dependencies"]:
        if d["name"] in PROIBIDOS:
            faltas.append(f"{p['name']} depende de {d['name']}")
    if not p["name"].startswith("barg"):
        faltas.append(f"crate '{p['name']}' fora do prefixo barg-* neste workspace")

if faltas:
    print("FF-C9 violada:", file=sys.stderr)
    for f in faltas:
        print(f"  ✗ {f}", file=sys.stderr)
    sys.exit(1)
print(f"  ✓ FF-C9: {len(locais)} crates, nenhum depende do produto")
