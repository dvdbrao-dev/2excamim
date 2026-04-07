# Promotion Policy v1

`Promotion Policy v1` is a query-only policy layer.

It composes:

- `readiness_v1`
- `decision_lineage_v1`
- `execution_boundary_v1`
- `governance_v1`
- `order_lifecycle_v1`

to answer whether a `signal`, `decision`, or `order` can advance to the next modeled step.

## Report shape

Each report exposes:

- `ref_id`
- `ref_type`
- `status`
- `next_step`
- `reasons`
- `supporting_refs`
- `blocking_refs`
- `notes`

## Status semantics

- `Eligible`: the entity can advance under the current model.
- `Weak`: the entity exists, but evidence is too partial to call it promotable.
- `Blocked`: veto or equivalent hard semantic blockers apply.
- `Frozen`: the entity is in a prudential wait-state; it is not weak or contradictory, but the
  current log says the system should wait rather than promote again.
- `Inconsistent`: the log exposes contradictions or impossible combinations.

## Current frozen rules

- `signal`: no dedicated frozen rule in v1.
- `decision`: frozen when healthy upstream exists but local order/execution activity has already
  started and the decision should now wait on order submission or execution follow-up.
- `order`: frozen when the order is locally registered but not yet submitted.

## Current limits

- No freeze fact is persisted.
- No deprecation model or scorecards exist.
- No write-time enforcement is introduced.
