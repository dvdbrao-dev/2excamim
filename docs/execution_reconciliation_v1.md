# Execution Reconciliation / Order Execution State v1

## Objetivo

Derivar un estado de ejecucion claro por `order_id` a partir de evidencia local ya existente:

- `order.submitted`
- `fill.received`

Este slice no introduce eventos nuevos. Es reconciliacion derivada y auditable sobre el log actual.

## Superficie

La reconciliacion queda visible en:

```bash
cargo run -- inspect order ord-1 --store ./var/events.jsonl
```

Y en JSON:

```bash
cargo run -- inspect order ord-1 --store ./var/events.jsonl --json
```

La seccion nueva es `Execution Summary`.

## Campos derivados

- `ordered_quantity`
- `filled_quantity`
- `remaining_quantity`
- `average_fill_price`
- `fill_count`
- `execution_status`

## Fuente de verdad v1

- `filled_quantity`, `fill_count` y `average_fill_price` se derivan de los `fill.received` de esa `order`
- `ordered_quantity` se deriva del `decision.size_hint` vinculado a la `order`, si existe

Esto mantiene el slice pequeno porque `order.registered` todavia no tiene una cantidad contractual propia.

## Estados expuestos

- `submitted_without_fills`
- `partially_filled`
- `fully_filled`
- `overfilled`
- `target_quantity_unknown`
- `inconsistent`

## Reglas minimas v1

- si la order esta `Submitted` y no tiene fills, el estado es `submitted_without_fills`
- si hay fills y `decision.size_hint` existe:
  - `filled < ordered` => `partially_filled`
  - `filled == ordered` => `fully_filled`
  - `filled > ordered` => `overfilled`
- si hay fills pero no hay `decision.size_hint`, el estado es `target_quantity_unknown`
- si el `order_lifecycle` local es `Weak` o `Inconsistent`, la reconciliacion se expone como `inconsistent`

## Lo que no hace

- accepted/rejected
- reconciliacion de broker
- scheduler
- gateway real
- posiciones
- portfolio engine
- PnL
- routing multi-venue

## Limitacion principal

La cantidad objetivo por order depende hoy de `decision.size_hint`. Si una decision no la trae, el runtime puede observar fills pero no cerrar si la order esta parcial o totalmente ejecutada.
