#!/usr/bin/env python3
"""ADR-0001 — a biblioteca analisa registros de gesto; não gera som.

A v0.2.0 removeu a síntese que tinha vindo junto na extração. Esta verificação
existe para que ela não volte por descuido: síntese é vocabulário de produto.
"""
import pathlib, re, sys

# Sinais de geração de som, não de análise.
SINAIS = [
    (r"\bfn\s+render\w*\s*\(", "função render* (geração de buffer de áudio)"),
    (r"\bstruct\s+\w*(Oscillator|Osc|Synth|Voice\w*Gen|Envelope)\b", "tipo de síntese"),
    (r"\b(sample_and_hold|wavetable|adsr|noise_gen)\b", "primitiva de síntese"),
]
IGNORAR = re.compile(r"(^|/)(target|tests|benches)/")

faltas = []
for arq in pathlib.Path("crates").rglob("*.rs"):
    if IGNORAR.search(str(arq)):
        continue
    texto = arq.read_text("utf-8", "replace")
    # tira comentários de linha para não acusar prosa que explica a regra
    codigo = re.sub(r"//.*", "", texto)
    for padrao, oque in SINAIS:
        if re.search(padrao, codigo):
            faltas.append(f"{arq}: {oque}")

if faltas:
    print("ADR-0001 violada — síntese sonora no BARG:", file=sys.stderr)
    for f in faltas:
        print(f"  ✗ {f}", file=sys.stderr)
    sys.exit(1)
print("  ✓ ADR-0001: nenhuma síntese sonora na biblioteca")
