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

## Python -> Rust Handoff v1

Estado: implementado en version minima

Incluye:

- comando CLI offline para ingerir `latest_signals.parquet`
- frontera explicita entre output research y eventos Rust
- traduccion contractual a `signal.generated`
- validacion minima por fila y reporte de aceptados, rechazados y deduplicados
- `dry-run`, batch trace y report estructurado de ingestión

Pendiente siguiente:

- ampliar el handoff sin salir de modo offline
- decidir si el decoder Parquet pasa a Rust nativo
- evaluar cuando el output research justifica `hypothesis.generated`

## Decision Materialization Flow v1

Estado: implementado en version minima

Incluye:

- comando runtime `materialize decisions`
- evaluacion explicable sobre readiness, governance y promotion policy
- `dry-run` y salida estructurada
- persistencia prudente de `decision.formed` para casos claramente elegibles

## Order Materialization Flow v1

Estado: implementado en version minima

Incluye:

- comando runtime `materialize orders`
- evaluacion explicable sobre `decision_promotion_policy`
- `dry-run` y salida estructurada
- persistencia prudente de `order.registered` para casos claramente elegibles
- identidad determinista `order-{decision_id}`

Pendiente siguiente:

- decidir si el siguiente paso merece `order.submitted` manual o automática
- mantener este slice sin gateway ni ejecución live

## Order Submission Boundary v1

Estado: implementado en version minima

Incluye:

- comando runtime `submit orders`
- `submission policy` explícita sobre órdenes locales
- `dry-run` y salida estructurada
- persistencia prudente de `order.submitted`
- preservación de `venue` local actual, incluida la convención `paper`

Pendiente siguiente:

- accepted/rejected
- integración con gateway real
- follow-up de ejecución sin introducir runtime live todavía

## Execution Observation / Fill Ingestion v1

Estado: implementado en version minima

Incluye:

- comando runtime `observe fill`
- validacion minima sobre la relacion con la `order` local
- `dry-run` y salida estructurada
- persistencia prudente de `fill.received`
- deteccion de duplicados por idempotency key contractual
- reflejo de ejecucion observada en `order_lifecycle`

Pendiente siguiente:

- reconciliacion avanzada
- accepted/rejected
- integracion con gateway real
- follow-up de ejecucion sin introducir runtime live todavia

## Execution Reconciliation / Order Execution State v1

Estado: implementado en version minima

Incluye:

- derivacion query-only de execution state por `order_id`
- resumen minimo con `ordered_quantity`, `filled_quantity`, `remaining_quantity`, `average_fill_price` y `fill_count`
- distincion entre `submitted_without_fills`, `partially_filled`, `fully_filled`, `overfilled`, `target_quantity_unknown` e `inconsistent`
- integracion visible en `inspect order`

Pendiente siguiente:

- hacer contractual la cantidad objetivo de la `order`
- reconciliacion mas rica sin introducir todavia broker semantics

## Batch Runner v1

Estado: implementado en version minima

Incluye:

- comando runtime `run batch`
- encadenado manual y reproducible de:
  - `ingest research-signals`
  - `materialize decisions`
  - `summary`
- `dry-run`
- report consolidado por fases
- errores que identifican claramente la fase fallida

Pendiente siguiente:

- decidir si el batch runner merece perfiles o filtros adicionales
- mantenerlo batch/manual sin convertirlo todavia en scheduler o daemon
