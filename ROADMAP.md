# Roadmap

## Prediction Markets Research MVP

Estado: implementado en versión mínima

Incluye:

- ingestión básica Kalshi
- ingestión básica Polymarket
- features MVP:
  - spread_tight
  - volume_spike_24h
  - price_deviation_vwap_1h
- señales MVP:
  - tight_spread_momentum
  - vwap_reversion
- export a Parquet

Pendiente:

- histórico persistido para baselines robustos
- validación estadística
- integración con runtime Rust
- traducción de output Python a eventos del sistema
