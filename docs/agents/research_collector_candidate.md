# Research Collector Candidate Agent

## Inspired By
Este agente está inspirado por la idea de harness de captura/snapshot de `polyrec`, pero reimplementado en estilo 2EXCAMIM y sin importar el repo externo.

## Purpose
`agents/research_collector_candidate.py` recolecta snapshots research para mercados cripto de predicción:
- spot price
- oracle/reference price (cuando exista)
- orderbook (cuando exista)
- metadata de slots desde `market_slot.discovered`
- eventos de salud de feed y gaps de datos

## What Is Collected
Emite:
- `market_snapshot.observed`
- `feed_health.checked`
- `data_gap.detected`

Con payloads orientados a research y feature engineering (`spread_bps`, `mid_price`, `depth`, `imbalance`, `spot_delta_bps`, `oracle_spot_delta_bps`).

## What Is Not Trusted Yet
- matching exacto de slugs contra mercado live
- calidad de oracle en tiempo real
- profundidad de orderbook fuera de adaptadores mock

## Why Snapshots Matter
Sin snapshots consistentes no se puede validar hipótesis de edge (por ejemplo oracle lag) ni construir scorecards reproducibles. Este collector es prerequisito de validación, no una estrategia ejecutable.

## Safety
- research/shadow only
- sin órdenes
- sin claves privadas
- modo offline/mock por defecto seguro

## Optional Run
```bash
bash scripts/run_research_collector_candidate.sh --mock --sample-count 2
```
