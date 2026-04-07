# Order Submission Boundary v1

## Objetivo

Cruzar de `order.registered` a `order.submitted` de forma contractual, auditable y local, sin implicar aceptación de broker ni ejecución real.

## Comando

```bash
cargo run -- submit orders --store ./var/events.jsonl --dry-run
```

Persistencia:

```bash
cargo run -- submit orders --store ./var/events.jsonl
```

JSON opcional:

```bash
cargo run -- submit orders --store ./var/events.jsonl --dry-run --json
```

## Submission policy v1

La `submission policy` es explícita y separada de `decision_promotion_policy`.

Una `order` se considera elegible para `order.submitted` cuando:

- existe localmente en `order_lifecycle`
- su estado es `Registered`
- no está ya en `Submitted` ni `ObservedWithFills`
- no está bloqueada o invalidada por la governance de su `decision` vinculada

Clasificación de salida:

- `Eligible`: sometible en `dry-run`
- `Submitted`: persistida como `order.submitted`
- `Skipped`: ya sometida, ya observada con fills o no lista bajo la policy actual
- `Blocked`: bloqueada por gobierno/lineage upstream
- `Inconsistent`: contradicciones visibles en el log

## Persistencia v1

Sin `--dry-run`, el flow persiste `order.submitted` solo para órdenes claramente elegibles.

Semántica usada:

- `order_id`: se preserva desde `order.registered`
- `venue`: se preserva desde `order.registered` y puede seguir siendo `paper`
- `instrument`: heredado de `order.registered`
- `parent_event_id`: `order.registered` más reciente del timeline local
- `correlation_id`: heredado del order local si existe

Provenance:

- `produced_by`: `runtime.order_submission`
- `actor`: `order_submission_boundary_v1`
- `producer_run_id`: batch trace del run

## Lo que no hace todavía

- accepted/rejected
- fills automáticos
- gateway
- resolver
- scheduler
- runtime live
- routing multi-venue
