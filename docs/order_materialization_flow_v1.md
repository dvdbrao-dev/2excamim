# Order Materialization Flow v1

## Objetivo

Evaluar decisiones del log actual y materializar `order.registered` solo para casos claramente elegibles, sin ejecutar mercado ni someter órdenes automáticamente.

## Comando

```bash
cargo run -- materialize orders --store ./var/events.jsonl --dry-run
```

Persistencia prudente:

```bash
cargo run -- materialize orders --store ./var/events.jsonl
```

JSON opcional:

```bash
cargo run -- materialize orders --store ./var/events.jsonl --dry-run --json
```

## Regla de materialización en v1

Una `decision` se considera materializable como `order.registered` solo cuando:

- existe en projections
- `decision_promotion_policy` devuelve `Eligible`
- `next_step` es `RegisterOrder`
- no existen `local_order_ids` downstream en `decision_lineage`

Clasificación de salida:

- `Eligible`: materializable en `dry-run`
- `Materialized`: persistida como `order.registered`
- `Skipped`: débil, congelada o con orden local ya registrada
- `Blocked`: vetada o bloqueada por semántica upstream
- `Inconsistent`: contradicciones visibles en el log

## Persistencia v1

Sin `--dry-run`, el flow persiste `order.registered` solo para casos claramente elegibles.

Semántica usada:

- `order_id` determinista: `order-{decision_id}`
- `venue`: `paper`
- `instrument`: heredado de `decision.formed`
- `parent_event_id`: `decision.formed` más reciente del timeline de la decisión
- `correlation_id`: heredado de la decisión si existe

Provenance:

- `produced_by`: `runtime.order_materialization`
- `actor`: `order_materialization_flow_v1`
- `producer_run_id`: batch trace del run

## Lo que no hace todavía

- `order.submitted` automática
- accepted/rejected
- gateway
- resolver
- runtime live
- scheduler
- orquestación permanente
