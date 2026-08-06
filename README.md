# BARG — Biblioteca de Análise de Registros de Gestos

*Gesture Record Analysis Library*

Biblioteca de análise do **gesto musical percussivo**: percepção de onset multicanal,
relógio determinístico, escalonamento antecipado, e — nas próximas versões — alinhamento e
comparação entre execuções.

Artefato de pesquisa do **GICC — Grupo de Pesquisa em Informação, Cultura e Computação**
(UFRN). Publicável, citável e implementável por terceiros.

[![License](https://img.shields.io/badge/license-Apache--2.0-blue)](LICENSE)

---

## O que é

O objeto de estudo é o **gesto** — a batida como ação que tem *quando*, *quanto* e *como
soa*, simultaneamente e por um mesmo motivo físico. A biblioteca torna esse gesto legível:
mede a execução de quem toca e permite colocá-la ao lado da execução de um intérprete
**nomeado**.

Duas propriedades governam o desenho e não são negociáveis:

- **Nenhum escore.** Nenhum caminho de código produz escalar de qualidade, acurácia ou
  proficiência. A saída é **delta** — a diferença, com sinal e magnitude, entre duas
  execuções nomeadas. Delta é uma relação entre duas coisas; escore é um juízo sobre uma
  delas.
- **Determinismo na medida.** O alinhamento entre execuções é algoritmo, não modelo
  aprendido. Auditabilidade, testabilidade e reprodutibilidade são requisitos, não
  preferências: quem usa precisa poder confiar no delta, e um terceiro precisa poder
  reproduzir a análise.

## Crates

| Crate | Papel |
|---|---|
| `barg-types` | Tipos fundamentais, `no_std` — sem alocação, sem dependência de plataforma |
| `barg-dsp` | `GrooveClock` sample-accurate e determinístico — a grade de referência — e análise de intensidade |
| `barg-perception` | Onset multicanal → `DrumHit { canal, peça, dinâmica, frame absoluto }`, com arbitragem entre canais para rejeitar vazamento |
| `barg-scheduler` | Escalonamento antecipado e lançamento quantizado — a camada assíncrona do padrão *Half-Sync/Half-Async* |

**Previstos:** `barg-analise` (alinhamento e comparação) e `barg-corpus` (leitor do
[FARG](https://github.com/gicc-ufrn/farg)).

**O que deliberadamente não está aqui:** síntese sonora. Vozes percussivas, baixo procedural
e sampler são do instrumento, não da análise — nada disso é necessário para reproduzir a
comparação entre execuções nem para implementar o FARG. Saíram na v0.2.0.

## Construir e testar

Requisito de projeto: **constrói e testa em Linux, sem Xcode, sem iOS e sem placa de áudio.**
É o que torna a análise reproduzível por terceiros, e o critério que define o que pertence a
esta biblioteca.

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Padrões arquiteturais

- **Half-Sync/Half-Async** (Schmidt & Cranor, 1996) — camada síncrona de tempo real e camada
  assíncrona de decisão, com fila entre elas
- **Producer-Consumer com fila SPSC lock-free** (Herlihy & Shavit, 2008)
- **RCU / publicação por troca de ponteiro atômico** (McKenney & Slingwine, 1998), com
  liberação diferida — nunca `dealloc` no callback de áudio
- **Regras de áudio em tempo real** (Bencina, 2011) — sem alocação, lock, I/O, syscall ou
  panic no callback

## Licença

**Apache-2.0** — ver [`LICENSE`](LICENSE). A concessão explícita de patente é deliberada:
há financiamento público envolvido.

## Como citar

Ver [`CITATION.cff`](CITATION.cff).
