# Batch Runner v1

## Objetivo

Encadenar en un solo comando los pasos batch ya existentes del runtime, sin introducir daemon, scheduler ni runtime live.

## Comando

```bash
cargo run -- run batch --research-signals research_prediction_markets/output/signals/latest_signals.parquet --store ./var/events.jsonl
```

Dry run:

```bash
cargo run -- run batch --research-signals research_prediction_markets/output/signals/latest_signals.parquet --store ./var/events.jsonl --dry-run
```

JSON opcional:

```bash
cargo run -- run batch --research-signals research_prediction_markets/output/signals/latest_signals.parquet --store ./var/events.jsonl --json
```

## Fases v1

El batch runner v1 ejecuta siempre estas fases, en este orden:

1. `ingest research-signals`
2. `materialize decisions`
3. `summary`

No reimplementa la semantica de negocio. Reutiliza las mismas capas y funciones del runtime.

Nota:

- en v1 el batch runner todavía se detiene antes de `materialize orders`

## Report consolidado

Salida minima:

- `batch_trace_id`
- `store_path`
- `research_signals_path`
- `dry_run`
- `success`
- resumen de ingestión
- resumen de materialización
- summary final del sistema

## Errores

Si una fase falla:

- el batch termina inmediatamente
- el error indica la fase exacta (`ingest`, `materialize` o `summary`)
- el comando devuelve exit code operativo no cero

## Lo que no hace todavia

- daemon
- watch mode
- scheduler persistente
- cron interno
- gateway
- resolver
- runtime live
- materialización automática de orders
