# Governance v1

`Governance v1` is a query-only interpretation layer over the existing append-only event log.

It answers, for `signal` and `decision`:

- whether the entity is currently eligible to advance
- whether it is weak, blocked, or inconsistent
- which references support that conclusion
- which references block that conclusion
- why the conclusion was produced

## Report shapes

`SignalGovernanceReport` and `DecisionGovernanceReport` expose:

- `ref_id`
- `ref_type`
- `status`
- `reasons`
- `supporting_refs`
- `blocking_refs`
- `notes`

## Status semantics

- `Eligible`: the current log justifies advancement under the current contract.
- `Weak`: the entity exists, but the current contract does not justify strong advancement yet.
- `Blocked`: veto or blocked upstream semantics prevent advancement.
- `Inconsistent`: contradictory references or impossible semantics are visible in the log.

`Frozen` is intentionally not modeled inside governance itself in v1.
That prudential wait-state now lives in `promotion_policy_v1`, which composes governance with
execution-boundary and order-lifecycle semantics.

## Derivation rules

### Signal governance

- Derives directly from `signal_readiness`.
- Confirmed and not vetoed maps to `Eligible`.
- Generated but unconfirmed maps to `Weak`.
- Signal veto maps to `Blocked`.
- Contradictory signal history maps to `Inconsistent`.

### Decision governance

- Reuses `decision_readiness` for upstream health.
- Reuses `decision_lineage` for promotion support, veto traceability, and hypothesis ancestry.
- Reuses `decision_execution_boundary` for decision-to-order-to-fill sufficiency.
- A supported decision with healthy upstream remains `Eligible` even when no execution has been
  observed yet; that case is treated as pending boundary evidence, not as a blocker.
- Observed downstream weakness, such as fills without local order submission, downgrades the
  decision to `Weak`.

## Current boundaries

- No governance result is persisted as an event.
- No promotion write-time enforcement is introduced.
- No freeze persistence, deprecation model, or component scorecards are introduced.
