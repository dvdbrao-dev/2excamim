# Decision Lineage v1

`Decision lineage v1` is a query-only report for `decision.formed`.

It answers:

- what upstream is traceable
- whether the decision is semantically supported, weak, blocked, or inconsistent
- what downstream fills have been observed
- why that classification was produced

## Report shape

The report exposes:

- `status`
- `reasons`
- `upstream_refs`
- `downstream_refs`
- `notes`

## Status semantics

- `Supported`: the decision is formed and its traced upstream signal is confirmed and not vetoed.
- `Weak`: the decision exists, but the current contract cannot justify strong promotion support.
- `Blocked`: a direct decision veto exists or the traced upstream signal is blocked.
- `Inconsistent`: the current log exposes conflicting references or contradictory downstream facts.

## Current boundaries

- The report is derived from the existing log only.
- No new events or persistence are introduced.
- No order lifecycle is implied by downstream fill observation.
- Missing or partial upstream is reported honestly as weak, not guessed away.
- `order.registered` may exist in parallel as a minimal local order entity, but lineage still avoids
  pretending that a full order lifecycle exists.
- Execution-boundary interpretation is handled separately in `execution_boundary_v1`; lineage keeps
  the upstream/downstream graph, while execution boundary focuses on the semantic seam between
  `decision.formed` and `fill.received`.
- `governance_v1` composes this report with readiness and execution boundary rather than replacing
  it.
