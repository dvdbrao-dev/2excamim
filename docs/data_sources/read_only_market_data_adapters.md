# Read-Only Market Data Adapters (External Candidates V1)

## No-live guarantee
- This path is read-only research only.
- No order placement, no wallet usage, no private keys, no authenticated CLOB trading.
- `mock` remains the default mode.
- `read_only` is opt-in via `--data-mode read_only`.

## Supported sources
- Binance public spot API (`BTCUSDT`, `ETHUSDT`, `SOLUSDT`) via unauthenticated GET.
- Polymarket Gamma metadata lookup by slug (best-effort, unauthenticated GET).
- Polymarket public orderbook adapter interface is implemented with safe disabled-by-default behavior.

## What is real now
- In `read_only` mode, spot prices come from real Binance public endpoint.
- Metadata lookup can be enabled via `--polymarket-metadata-enabled` and is best-effort.

## What is still disabled or partial
- Polymarket orderbook collection is opt-in (`--polymarket-orderbook-enabled`) and may fail depending on endpoint availability.
- Any metadata/orderbook miss emits `data_gap.detected`; failures never silently pass.
- This is not strategy execution and not promotion logic.

## How to run mock mode
```bash
python3 agents/research_collector_candidate.py --data-mode mock
```

## How to run read-only smoke mode
```bash
bash scripts/run_read_only_market_data_smoke.sh
```

## Risks
- Public endpoint availability can vary by region/environment.
- Latency and rate limits can degrade data completeness.
- Slug→market metadata resolution is best-effort and may return empty for candidate slugs.

## Why this is not edge proof yet
- This only improves data realism for candidate observations.
- No statistical validation conclusion is implied from adapter integration alone.
- Edge validation still requires reproducible backtests and out-of-sample evaluation.
