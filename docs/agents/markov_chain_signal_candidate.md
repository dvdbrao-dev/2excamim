# Markov Chain Signal Candidate

## Hypothesis
Para mercados cripto Up/Down de corta duración (especialmente BTC 5m), puede existir estructura condicional débil:

`P(next_outcome | previous_outcome, spot_momentum_bucket, volatility_bucket, book_skew_bucket)`

Este agente se limita a candidate/shadow research.

## No-live guarantee
- No live trading.
- No order placement.
- No cancellation.
- No wallet/private keys.
- No endpoints autenticados CLOB.
- No auto-promote.
- Sin decisiones LLM.

## Inputs
Lee `market_snapshot.observed` desde JSONL y filtra por `asset/window`.
Reconstruye outcome por slot con spot:
- `open_spot`: primer snapshot del slot.
- `close_spot`: último snapshot del slot.
- `outcome = UP` si `close_spot > open_spot`, si no `DOWN`.
- `outcome_source = binance_reconstructed_v1`.

## State definition
`state = (previous_outcome, spot_momentum_bucket, volatility_bucket, book_skew_bucket)`

Buckets:
- `spot_momentum_bucket`: `strong_down|down|flat|up|strong_up`
- `volatility_bucket`: `low|medium|high`
- `book_skew_bucket`: `down_favored|neutral|up_favored`

## Transition probability
Cuenta transiciones `UP/DOWN` por estado y estima:

`p_up = (count_up + alpha) / (count_total + alpha + beta)`

Con `p_down = 1 - p_up`.

## Smoothing + backoff
Aplica smoothing Beta/Laplace y backoff jerárquico:
1. `full_state`
2. `previous_outcome + spot_momentum + volatility`
3. `previous_outcome + spot_momentum`
4. `previous_outcome`
5. `global_prior`

## Signal scoring
Para el slot activo/latest:
- calcula `edge_up = p_up - best_ask_up - cost_buffer`
- calcula `edge_down = p_down - best_ask_down - cost_buffer`
- selecciona lado con mayor net edge.

Emite `candidate_signal.scored` con campos estándar + `model/pricing/edge`.

## Rejection logic
Rechaza con `reject_reason` cuando:
- `orderbook_missing`
- `insufficient_global_samples`
- `insufficient_state_samples`
- `spread_too_wide`
- `depth_too_low`
- `too_close_to_expiry`
- `edge_below_threshold`
- `stale_feed`

Si no hay snapshots, emite `data_gap.detected` (`missing_market_snapshots`).

## Minimum evidence before promotion
El agente solo sugiere señales candidatas. Promoción requiere:
- suficiente muestra out-of-sample,
- scorecard con expectativa neta positiva robusta,
- estabilidad por régimen,
- drawdown y data quality aceptables,
- revisión humana explícita.

## Why this does not prove edge yet
Un modelo Markov simple puede capturar ruido/transitorios. Sin validación estricta walk-forward y costos realistas, no demuestra edge ejecutable.

## Run
```bash
bash scripts/run_markov_chain_signal_candidate.sh
```
