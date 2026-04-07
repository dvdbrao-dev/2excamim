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

## Runtime Skeleton v1

Estado: implementado en versión mínima

Incluye:

- binario CLI en Rust para inspección del log JSONL
- entrypoint ejecutable claro para evaluación del sistema actual
- inspección de `signal`, `decision`, `order` y `fill`
- consulta de governance, policy y boundaries reutilizando `QueryService`
- salida estructurada en texto con JSON opcional

Pendiente siguiente:

- traducción del output Python de research a eventos del sistema Rust
- integración Python -> Rust sin introducir runtime live ni scheduler real todavía
