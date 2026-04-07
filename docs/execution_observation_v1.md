# Execution Observation / Fill Ingestion v1

## Objetivo

Introducir `fill.received` en el runtime operativo como observación local y auditable de ejecución, sin abrir todavía reconciliación avanzada, gateway real ni semántica de broker.

## Comando

Dry-run:

```bash
cargo run -- observe fill \
  --fill-id fill-1 \
  --order-id ord-1 \
  --side buy \
  --quantity 1 \
  --price 0.54 \
  --executed-at 2026-04-07T00:00:00Z \
  --store ./var/events.jsonl \
  --dry-run
```

Persistencia:

```bash
cargo run -- observe fill \
  --fill-id fill-1 \
  --order-id ord-1 \
  --side buy \
  --quantity 1 \
  --price 0.54 \
  --executed-at 2026-04-07T00:00:00Z \
  --store ./var/events.jsonl
```

JSON opcional:

```bash
cargo run -- observe fill \
  --fill-id fill-1 \
  --order-id ord-1 \
  --side buy \
  --quantity 1 \
  --price 0.54 \
  --executed-at 2026-04-07T00:00:00Z \
  --json
```

Flags opcionales:

- `--decision-id`
- `--instrument`
- `--venue`

## Validación mínima v1

La observación exige una orden local ya visible en el log:

- debe existir `order.registered`
- para persistir un `fill.received`, también debe existir `order.submitted`

Comprobaciones de relación:

- `order_id` debe resolver a una order local
- si se pasa `--decision-id`, debe coincidir con la decision local resuelta
- si se pasa `--instrument`, debe coincidir con el instrumento local
- si se pasa `--venue`, debe coincidir con la venue local
- `quantity` y `price` deben ser positivos
- `executed-at` debe ser RFC3339 válido

## Semántica operativa

- `order.submitted`: la orden cruzó el boundary de ejecución local/paper
- `fill.received`: el runtime observó ejecución asociada a esa orden

`fill.received` en este slice no implica:

- accepted/rejected
- reconciliación avanzada
- fill final frente a parcial
- integración con broker o exchange real

## Idempotencia y trazabilidad

- `fill.received` reutiliza la clave contractual `fill.received:v1:{venue}:{order_id}:{fill_id}`
- una repetición del mismo fill no duplica escritura
- `provenance.produced_by = runtime.fill_observation`
- `producer_run_id` usa `batch_trace_id`
- `trace_id` usa `fill_id`

## Superficie visible

- `inspect order <order-id>` ya refleja el efecto del slice mediante `Lifecycle`
- una order con `registered + submitted + fill.received` pasa a `ObservedWithFills`
- `inspect fill <fill-id>` permite ver readiness y execution boundary del fill observado

## Lo que no hace todavía

- scheduler
- gateway
- resolver
- routing multi-venue
- fills automáticos
- portfolio engine
