# EVENT_CATALOG

## Scope

This catalog documents the current event contract implemented by `2EXCAMIM`.
It only covers the existing event set:

- `hypothesis.generated`
- `signal.generated`
- `signal.confirmed`
- `veto.raised`
- `decision.formed`
- `fill.received`

## Cross-Cutting Rules v1

### Payload vs linkage

- Payload carries event-specific business facts.
- Linkage carries cross-event references used for correlation and ancestry.
- If the same reference appears in payload and linkage, values must match.
- Primary IDs should remain in payload because they define event-specific meaning.
- Linkage should carry references that other events and query surfaces need to traverse without
  decoding every payload shape.

### Minimum ID policy

- `event_id` must be a valid UUID.
- `schema_version` is currently fixed to `v1`.
- Domain IDs such as `hypothesis_id`, `signal_id`, `decision_id`, `veto_id`, `order_id`,
  `fill_id`, `confirmed_by`, and `venue` must not be blank.
- Any field contributing to an idempotency key must not contain `:`.
- `correlation_id` is optional but, when present, must not be blank.

### Minimum timestamp semantics

- `occurred_at` is the event-envelope timestamp recorded by the Rust contract layer.
- `occurred_at` represents when the event record was formed in this system, not necessarily when
  the underlying market fact originally happened.
- `executed_at` exists only on `fill.received` payload and represents venue execution time for the
  fill fact.
- `executed_at` and `occurred_at` are not interchangeable.

### Minimum retry and idempotency policy

- Producers must retry using the same idempotency key for the same semantic event.
- The store deduplicates by idempotency key.
- Replays with the same idempotency key but contradictory contract content are invalid.
- Replays with the same idempotency key and contract-equivalent content are tolerated as duplicates.
- Consumers must treat event processing as at-least-once and be safe under duplicate delivery.

### Derived readiness semantics

- Readiness and lifecycle are derived by query-time interpretation of the current event log.
- Readiness is not persisted as an event in this slice.
- `weak`, `blocked`, `insufficient`, and `inconsistent` results express what the current contract
  can and cannot justify from the available log.
- `decision.formed` is also interpreted through a query-derived promotion boundary / lineage view:
  upstream signal support, traced hypothesis, applicable vetoes, and downstream fill observation.
- `fill.received` also participates in a query-derived execution boundary view that interprets only
  observed linkage to decisions and external `order_id` references.

## Event Specifications

### `hypothesis.generated`

Name:
`hypothesis.generated`

Version:
`v1`

Producer:
Research or derived-analysis producer.

Expected consumers:
Signal generation, replay, query, observability, audit readers.

Purpose:
Record a thesis before downstream promotion exists.

Payload schema summary:
`hypothesis_id`, `instrument`, `timeframe`, `thesis`, optional `direction_hint`, optional
`confidence`.

Required fields:
`hypothesis_id`, `instrument`, `timeframe`, `thesis`

Optional fields:
`direction_hint`, `confidence`

Timestamp semantics:
`occurred_at` marks when the hypothesis event was formed in this system.

Idempotency expectations:
Stable by `hypothesis_id`.

Ordering expectations:
Should precede downstream events that claim this hypothesis as ancestry, when such ancestry is
present.

Linkage keys:
May include `hypothesis_id`, `parent_event_id`, `correlation_id`.

Provenance keys:
Full provenance record is expected.

Persistence requirements:
Must be persisted as valid JSON object payload with `event_id`, `schema_version`, linkage, and
provenance all validated.

Retry behavior:
Retry with the same idempotency key.

Failure modes:
Blank thesis, invalid confidence, invalid event UUID, schema drift, contradictory duplicated
`hypothesis_id`.

Future evolution notes:
Could later carry richer thesis metadata, but not in this slice.

### `signal.generated`

Name:
`signal.generated`

Version:
`v1`

Producer:
Signal-generation producer.

Expected consumers:
Confirmation logic, veto logic, decision formation, projections, queries, observability.

Purpose:
Record a trade-relevant signal before confirmation or decision formation.

Payload schema summary:
`signal_id`, optional `hypothesis_id`, `instrument`, `timeframe`, `side`, `strength`, optional
`rationale`.

Required fields:
`signal_id`, `instrument`, `timeframe`, `side`, `strength`

Optional fields:
`hypothesis_id`, `rationale`

Timestamp semantics:
`occurred_at` marks when the signal event was formed in this system.

Idempotency expectations:
Stable by `signal_id`.

Ordering expectations:
Should exist before `signal.confirmed`, signal-scoped `veto.raised`, or `decision.formed` events
that claim the same signal lineage.

Linkage keys:
May include `hypothesis_id`, `signal_id`, `parent_event_id`, `correlation_id`.

Provenance keys:
Full provenance record is expected.

Persistence requirements:
Stored payload must decode back into the typed signal contract and duplicated references must agree.

Retry behavior:
Retry with the same idempotency key.

Failure modes:
Strength outside `[0,1]`, blank identifiers, invalid UUID, contradictory `signal_id` or
`hypothesis_id` between payload and linkage.

Future evolution notes:
No promotion or readiness metadata should be added in this slice.

### `signal.confirmed`

Name:
`signal.confirmed`

Version:
`v1`

Producer:
Confirmation-capable producer.

Expected consumers:
Decision formation, audit readers, projections, observability.

Purpose:
Record that a specific producer confirmed a signal.

Payload schema summary:
`signal_id`, `confirmed_by`, optional `confirmation_reason`, optional `confirmation_score`.

Required fields:
`signal_id`, `confirmed_by`

Optional fields:
`confirmation_reason`, `confirmation_score`

Timestamp semantics:
`occurred_at` marks when the confirmation record was formed.

Idempotency expectations:
Stable by `signal_id + confirmed_by`.

Ordering expectations:
Semantically depends on an already existing signal, although the current contract layer does not
yet enforce full historical existence checks.

Linkage keys:
Should carry `signal_id`; may also carry `hypothesis_id`, `correlation_id`, `parent_event_id`.

Provenance keys:
Full provenance record is expected.

Persistence requirements:
Typed payload validation must succeed; duplicated `signal_id` must not disagree.

Retry behavior:
Retry with the same idempotency key.

Failure modes:
Invalid score, blank confirmer, invalid UUID, contradictory `signal_id`.

Future evolution notes:
No confirmation-state machine is introduced here.

### `veto.raised`

Name:
`veto.raised`

Version:
`v1`

Producer:
Risk, policy, or human-override producer.

Expected consumers:
Decision gating, replay, audit readers, observability.

Purpose:
Record that a scope-specific veto blocks downstream progression.

Payload schema summary:
`veto_id`, `scope`, `target_id`, `reason_code`, optional `reason_text`, `raised_by`.

Required fields:
`veto_id`, `scope`, `target_id`, `reason_code`, `raised_by`

Optional fields:
`reason_text`

Timestamp semantics:
`occurred_at` marks when the veto record was formed.

Idempotency expectations:
Stable by `veto_id`.

Ordering expectations:
Should not be interpreted as effective before the targeted entity exists, but current storage does
not enforce historical lookup.

Linkage keys:
For scoped vetoes, linkage should use the matching target field:
`signal_id` for `Signal`, `decision_id` for `Decision`, `order_id` for `Order`.
`Global` may omit target linkage.

Provenance keys:
Full provenance record is expected.

Persistence requirements:
Typed payload validation must succeed; scoped target linkage must not disagree with `target_id`.

Retry behavior:
Retry with the same idempotency key.

Failure modes:
Blank reason code, blank target, invalid UUID, contradictory target linkage for scoped vetoes.

Future evolution notes:
No veto-lift or veto-resolution event is introduced in this slice.

### `decision.formed`

Name:
`decision.formed`

Version:
`v1`

Producer:
Decision-forming producer.

Expected consumers:
Execution-facing readers, projections, queries, audit readers.

Purpose:
Record a formed trading intent derived from upstream state.

Payload schema summary:
`decision_id`, `instrument`, `action`, optional `side`, optional `size_hint`, optional
`rationale`.

Required fields:
`decision_id`, `instrument`, `action`

Optional fields:
`side`, `size_hint`, `rationale`

Timestamp semantics:
`occurred_at` marks when intent was formed, not when anything executed.

Idempotency expectations:
Stable by `decision_id`.

Ordering expectations:
Should follow any signal lineage it claims to derive from.

Linkage keys:
Should carry `decision_id`; may also carry `signal_id`, `hypothesis_id`, `correlation_id`,
`parent_event_id`.

Provenance keys:
Full provenance record is expected.

Persistence requirements:
Typed payload validation must succeed; duplicated `decision_id` must not disagree.

Retry behavior:
Retry with the same idempotency key.

Failure modes:
Blank decision ID, invalid size hint, invalid UUID, contradictory `decision_id`.

Future evolution notes:
No `order.*` event family is introduced here.

### `fill.received`

Name:
`fill.received`

Version:
`v1`

Producer:
Execution-facing producer or venue-ingestion producer.

Expected consumers:
Queries, projections, replay, audit readers, observability.

Purpose:
Record that a venue reported a fill.

Payload schema summary:
`fill_id`, optional `decision_id`, `order_id`, `instrument`, `side`, `quantity`, `price`,
`venue`, `executed_at`.

Required fields:
`fill_id`, `order_id`, `instrument`, `side`, `quantity`, `price`, `venue`, `executed_at`

Optional fields:
`decision_id`

Timestamp semantics:
`executed_at` is venue execution time.
`occurred_at` is the local event formation time when this fill record entered the contract layer.

Idempotency expectations:
Stable by `venue + order_id + fill_id`.

Ordering expectations:
Should follow the execution fact it represents.
Ideally should also have coherent upstream lineage, but the current slice does not yet model `order`
as a first-class entity.

Linkage keys:
Should carry `order_id`.
May carry `decision_id`, `signal_id`, `hypothesis_id`, `correlation_id`, `parent_event_id`.

Provenance keys:
Full provenance record is expected.

Persistence requirements:
Typed payload validation must succeed.
Duplicated `order_id` and optional `decision_id` must not disagree between payload and linkage.

Retry behavior:
Retry with the same idempotency key.

Failure modes:
Non-positive quantity, non-positive price, invalid UUID, contradictory `order_id`, contradictory
`decision_id`, semantically weak upstream lineage.

Future evolution notes:
`order` is intentionally not introduced in this slice.
Upstream sufficiency remains partially documented debt until `order` becomes first-class.
