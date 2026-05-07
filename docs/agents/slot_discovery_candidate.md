# Slot Discovery Candidate Agent

## Purpose
`agents/market_slot_discovery_candidate.py` descubre slots temporales esperados para mercados cripto Up/Down de Polymarket (BTC/ETH/SOL en ventanas 5m y 15m) usando alineación UTC determinista.

El agente solo emite eventos `market_slot.discovered` (y, opcionalmente, eventos de salud/datos cuando falla confirmación).

## Limits
- Candidate/shadow/research only.
- No trading live.
- No colocación de órdenes.
- No uso de credenciales.
- No dependencia obligatoria de red.

## Why This Is Not Edge By Itself
Descubrir slots no implica ventaja de ejecución o predicción. Solo crea un mapa reproducible de candidatos de mercado para que otras capas (collector + scorer) evalúen hipótesis reales.

## How It Feeds Next Stages
- Oracle lag candidate scorer: consume los slots para medir desfase oracle/spot por ventana.
- Research snapshot collector: usa los slots como objetivo de captura para snapshots de orderbook/price.

## Event Shape
`market_slot.discovered` payload mínimo emitido:
- `asset`
- `window`
- `slot_start`
- `slot_end`
- `candidate_slug`
- `confidence`
- `discovery_method`
- `confirmed`
- `source`

## Example
```bash
python3 agents/market_slot_discovery_candidate.py \
  --assets BTC,ETH,SOL \
  --windows 5m,15m \
  --lookahead-slots 2 \
  --output-jsonl ./var/events/external_candidates.jsonl
```
