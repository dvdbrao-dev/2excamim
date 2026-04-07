# Order Lifecycle v1

`Order Lifecycle v1` introduces the smallest local lifecycle distinction that is still useful:

- `order.registered`
- `order.submitted`
- `fill.received`

## Purpose

The slice separates:

- local order identity
- local intent-to-execute
- externally observed execution

without introducing a gateway, orchestration runtime, or full order state machine.

## Semantics

- `order.registered`: the system recognizes a local order entity and can trace it.
- `order.submitted`: the system recorded that the local order was actually submitted or attempted
  for execution.
- `fill.received`: an external venue reported execution.

`order.submitted` is stronger than `order.registered`, but it still does not imply:

- venue acceptance
- partial or final completion semantics
- cancellation or amendment semantics

## Query impact

- `decision_lineage` can now distinguish registered local order support from submitted local order
  support.
- `execution_boundary` now treats `registered + fill` as weaker than `submitted + fill`.
- `governance_v1` benefits indirectly through the refined execution boundary.
- `order_query_v1` exposes a direct report by `order_id` for local lifecycle inspection.
- `promotion_policy_v1` can interpret `registered` as a frozen wait-state and `submitted` as
  eligible for execution follow-up, without persisting freeze facts.

## Current limits

- No `order.cancelled`, `order.rejected`, `order.amended`, or `order.completed` exists.
- No write-time historical enforcement is introduced.
- Submission without prior registration can be detected as inconsistent at query time, but it is
  not blocked at append time in this slice.

## Runtime materialization note

`materialize orders` only creates `order.registered`. It intentionally does not emit
`order.submitted` in v1.

## Runtime submission note

`submit orders` now materializes `order.submitted` from eligible local orders, but still does not
imply broker acceptance, execution, amendment or cancellation semantics.

## Runtime execution observation note

`observe fill` now materializes `fill.received` from observed local execution evidence, but this
still does not imply accepted/rejected semantics, reconciliation completeness or gateway
integration.
