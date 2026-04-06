# Execution Boundary v1

`Execution boundary v1` is a query-only interpretation of the seam between `decision.formed` and
`fill.received`.

It does not introduce:

- a local `order` entity
- `order.*` events
- execution orchestration

## Report shape

The report exposes:

- `primary_ref_id`
- `primary_ref_type`
- `status`
- `reasons`
- `decision_refs`
- `observed_order_ids`
- `observed_fill_ids`
- `notes`

## Status semantics

- `Clear`: the decision-to-fill relationship is traceable and contractually coherent enough under
  the current model.
- `Weak`: the boundary is only partially supported, such as a decision with no fills yet or a fill
  with only an external `order_id`.
- `Blocked`: the decision side is blocked by upstream semantics, and observed fills therefore mark a
  problematic boundary.
- `Inconsistent`: conflicting references or contradictory downstream facts are present.

## Current limits

- `order_id` is treated as an external observed reference only.
- Multiple fills are acceptable when they remain coherent.
- Multiple incompatible `order_id` references for the same decision are treated as ambiguous because
  the model lacks a first-class order contract.
