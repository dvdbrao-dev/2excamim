# Execution Boundary v1

`Execution boundary v1` is a query-only interpretation of the seam between `decision.formed` and
`fill.received`.

It does not introduce:

- a full local order lifecycle
- execution orchestration

In the current slice, `order.registered` exists as the minimum local order entity, but the boundary
still does not imply acceptance, amendment, cancellation, or completion semantics.
`order.submitted` is now the minimum local lifecycle evidence that the registered order was
actually attempted for execution.

## Report shape

The report exposes:

- `primary_ref_id`
- `primary_ref_type`
- `status`
- `reasons`
- `decision_refs`
- `observed_order_ids`
- `submitted_order_ids`
- `observed_fill_ids`
- `notes`

## Status semantics

- `Clear`: the decision-to-fill relationship is traceable and contractually coherent enough under
  the current model, including local order submission evidence.
- `Weak`: the boundary is only partially supported, such as a decision with no fills yet or a fill
  with only an external `order_id`, or a fill whose order was never locally submitted.
- `Blocked`: the decision side is blocked by upstream semantics, and observed fills therefore mark a
  problematic boundary.
- `Inconsistent`: conflicting references or contradictory downstream facts are present.

## Current limits

- `order_id` may still be only an external observed reference when no `order.registered` exists.
- `order.registered` can promote an external order reference into a local contractual entity.
- `order.submitted` is stronger than `order.registered` but still does not prove acceptance or
  execution by itself.
- Multiple fills are acceptable when they remain coherent.
- Multiple incompatible `order_id` references for the same decision are treated as ambiguous because
  the model lacks a first-class order contract.
- `governance_v1` reuses this boundary to decide whether a healthy decision remains eligible to
  advance or must be downgraded to `Weak`, `Blocked`, or `Inconsistent`.
