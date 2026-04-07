# DOMAIN_MODEL

## Scope

This document defines the minimum contractual domain model for `2EXCAMIM` in the current slice.
It is intentionally narrow. It does not introduce runtime orchestration, scheduler semantics,
gateways, order lifecycle machinery, risk engine behavior, or LLM boundaries.

The goal is to make the core evented model semantically defensible without inflating design.

## Core Concepts

### hypothesis

What it represents:
A research or model-originated market thesis that may later justify downstream signal generation.

Kind:
Entity, materialized through append-only events and reconstructed as a logical identity.

Purpose:
To capture a thesis before confirmation, veto, or execution concerns exist.

Key attributes:
`hypothesis_id`, `instrument`, `timeframe`, `thesis`, `direction_hint`, `confidence`.

Identity / ID authority:
`hypothesis_id` is the domain identifier. Authority belongs to the producer that generated the
hypothesis event.

Relationships:
May be referenced by zero or more signals.
May be carried in linkage and payload when downstream artifacts want to preserve ancestry.

Lifecycle:
Created by `hypothesis.generated`.
No terminal lifecycle event is defined in this slice.

Consistency notes:
`hypothesis_id` is the stable contractual reference. If duplicated in linkage and payload, values
must not disagree.

### signal

What it represents:
A directional market assertion derived from research, runtime logic, or other accepted upstream
logic.

Kind:
Entity, materialized through append-only events and reconstructed as a logical identity.

Purpose:
To express a trade-relevant indication before execution intent exists.

Key attributes:
`signal_id`, optional `hypothesis_id`, `instrument`, `timeframe`, `side`, `strength`, `rationale`.

Identity / ID authority:
`signal_id` is the domain identifier. Authority belongs to the producer of `signal.generated`.

Relationships:
May reference one hypothesis.
May later receive confirmations.
May later become the target of veto.
May later support formation of a decision.

Lifecycle:
Created by `signal.generated`.
May be further qualified by `signal.confirmed`.
May be blocked by `veto.raised`.

Consistency notes:
A signal can exist without confirmation.
Signal confirmation does not replace the original signal record; it adds state.
If `hypothesis_id` exists in both payload and linkage, both representations must agree.

### decision

What it represents:
An execution intent formed from upstream signal state.

Kind:
Entity, materialized through append-only events and reconstructed as a logical identity.

Purpose:
To represent intent that is strong enough to be acted upon later, without claiming that execution
already happened.

Key attributes:
`decision_id`, `instrument`, `action`, optional `side`, optional `size_hint`, `rationale`.

Identity / ID authority:
`decision_id` is the domain identifier. Authority belongs to the producer of
`decision.formed`.

Relationships:
Usually linked to a signal lineage.
May later be referenced by a fill.
May be blocked by veto through `target_id + scope`.

Lifecycle:
Created by `decision.formed`.
No `decision.executed`, `decision.cancelled`, or `order.*` lifecycle exists in this slice.

Consistency notes:
A decision is not an order.
A decision is not proof of execution.
If `decision_id` exists in both payload and linkage, both representations must agree.

### order

What it represents:
A local contractual record that this system recognizes an order reference as part of its own
operational trace.

Kind:
Entity, materialized through append-only events and reconstructed as a logical identity.

Purpose:
To close the semantic gap between formed intent and observed fills without introducing a full order
lifecycle.

Key attributes:
`order_id`, optional `decision_id`, `instrument`, `venue`.

Identity / ID authority:
`order_id` is the local contractual identifier once registered by `order.registered`.
In v1 the system may adopt an externally observed order reference as the local ID, but after
registration that ID becomes the canonical local handle.

Relationships:
May reference one decision.
May later be locally submitted for execution.
May later be referenced by fills.
Does not yet carry submission, amendment, cancellation, or completion lifecycle semantics.

Lifecycle:
Created by `order.registered`.
May later be advanced by `order.submitted`.
No additional order lifecycle events are defined in this slice.

Consistency notes:
An order is not a fill.
An order is not proof of execution by itself.
If `decision_id` exists in both payload and linkage, both representations must agree.
If `order_id` exists in both payload and linkage, both representations must agree.

### fill

What it represents:
An externally observed execution fact.

Kind:
Record.

Purpose:
To capture that a venue reported an executed fill with price, quantity, side, and venue-time.

Key attributes:
`fill_id`, optional `decision_id`, `order_id`, `instrument`, `side`, `quantity`, `price`,
`venue`, `executed_at`.

Identity / ID authority:
`fill_id` is venue-facing fill identity inside the current contract.
The full idempotent identity is effectively `venue + order_id + fill_id`.
Authority belongs to the execution-facing producer.

Relationships:
May reference one decision.
Must reference one `order_id`, which may now be a local contractual entity when
`order.registered` exists.

Lifecycle:
Created by `fill.received`.
No settlement, reconciliation, or position lifecycle is defined here.

Consistency notes:
A fill is execution evidence, not intent.
A fill without a coherent upstream chain is contractually weak even if it is syntactically valid.
Even with `order` introduced in v1, fill sufficiency remains intentionally partial because order
lifecycle semantics are still absent.

### linkage record

What it represents:
Cross-event references used for correlation, ancestry, and timeline reconstruction.

Kind:
Record.

Purpose:
To provide stable cross-cutting references without forcing every consumer to inspect payload
shape-specific fields.

Key attributes:
`hypothesis_id`, `signal_id`, `decision_id`, `order_id`, `position_id`, `parent_event_id`,
`correlation_id`.

Identity / ID authority:
There is no standalone `linkage_id`. Each field borrows identity authority from the referenced
entity or record.

Relationships:
May connect an event to hypothesis, signal, decision, order, position, and lineage context.

Lifecycle:
Created and persisted inside every event envelope and stored event.

Consistency notes:
Linkage is supplementary, not a source of truth that can contradict payload.
If a primary reference appears in payload and linkage, linkage must match payload.

### provenance record

What it represents:
Operational origin metadata for an event.

Kind:
Record.

Purpose:
To record source category, upstream source reference, producer run, actor, trace, and notes.

Key attributes:
`source_kind`, `source_ref`, `producer_run_id`, `actor`, `trace_id`, `notes`.

Identity / ID authority:
No standalone identity. Provenance metadata is scoped to the containing event.

Relationships:
Attached to every event.

Lifecycle:
Created with the event and immutable after persistence.

Consistency notes:
Provenance explains origin; it does not replace domain linkage.
Traceability may be partial, but blank provided fields are not contractually acceptable.

## Lifecycle Rules v1

- A `signal` can exist without confirmation.
- A confirmed `signal` can become eligible for `decision` formation if no veto blocks promotion.
- A vetoed `signal` blocks promotion to `decision` while that veto remains semantically in force.
- Governance is query-derived in v1: it classifies `signal` and `decision` as eligible, weak,
  blocked, or inconsistent from the current log.
- A `decision` represents formed intent, not execution.
- A `decision` lineage is query-derived from current upstream references and observed downstream
  evidence; it is not a separately persisted entity in v1.
- The boundary between `decision` and `fill` is also query-derived in v1: it reports observed
  external `order_id` references and execution evidence without promoting `order` into a local
  entity.
- `order.registered` introduces the minimum local order entity needed to trace `decision -> order ->
  fill`.
- `order.submitted` adds the minimum local lifecycle evidence that the registered order was
  actually attempted for execution.
- `order` in v1 is a traceability entity, not a complete execution lifecycle.
- A `fill` must not be treated as contractually healthy when its upstream lineage is absent,
  contradictory, or materially underspecified.
- `fill.received` may carry an optional `decision_id` in the current model; this is an admitted
  limitation, not proof that missing decision linkage is semantically complete.
- `fill.order_id` may still be only an external reference when no local `order.registered` exists.
- Multiple external or local order references for the same decision remain semantically weak or
  inconsistent unless the current model can justify them.
- `linkage` may help consumers reconstruct ancestry, but it must not disagree with payload
  references.
- `provenance` explains source and trace, but does not authorize semantic promotion by itself.

## Notes On Current Deliberate Boundaries

- Readiness and lifecycle interpretation are query-derived from the event log in this phase; they
  are not persisted as separate facts.
- A readiness result is an interpretation layer over existing events, not a new event family.
- A governance result is also an interpretation layer over existing events, not a new event family.
- An order-centric lifecycle result is also query-derived in v1: it reports whether an `order_id`
  is merely registered, submitted, observed with fills, weak, or inconsistent.
- A promotion / freeze policy result is also query-derived in v1: it reports whether a `signal`,
  `decision`, or `order` is eligible, weak, blocked, frozen, or inconsistent for the next modeled
  step.
- `order` is now a first-class local entity, but only at the minimal registration layer.
- No runtime scheduler semantics are part of this document.
- No gateway behavior is specified here.
- No risk engine policy is specified here beyond the existence of veto as an event.
- No LLM-origin contract is defined here.
- No new `order.cancelled`, `order.rejected`, `order.completed`, `promotion.*`, `freeze.*`,
  `governance.*`, or `readiness.*` events are introduced here.
