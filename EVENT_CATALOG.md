# EVENT_CATALOG

## Scope

This catalog documents the current event contract implemented by `2EXCAMIM`.
It only covers the existing event set:

- `hypothesis.generated`
- `signal.generated`
- `signal.confirmed`
- `veto.raised`
- `decision.formed`
- `order.registered`
- `order.submitted`
- `fill.received`

For external-derived candidate research, this catalog also defines the pre-contract
event envelope and the initial event-type registry used by Python candidate agents.

External candidate agent references:
- `docs/agents/slot_discovery_candidate.md`
- `docs/agents/research_collector_candidate.md`
- `docs/agents/oracle_lag_signal_candidate.md`
- `docs/agents/shadow_execution_simulator.md`
- `docs/agents/external_candidate_scorecard.md`
- `docs/reports/external_candidate_reports.md`
- `docs/backtesting/external_candidate_backtest.md`
- `docs/research/cross_venue_market_matching.md`

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
- For externally-derived candidates, producers should also expose a stable `aggregate_key` in their
  pre-contract envelope so the Rust boundary can map aggregate lineage deterministically.

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
- External candidate producers must keep `idempotency_key` stable across retries for the same
  semantic fact before Rust translation.

### External candidate pre-contract envelope

Before translation into typed Rust events, candidate/shadow research components should emit a
pre-contract envelope with:
- `event_type`
- `event_id`
- `timestamp`
- `idempotency_key`
- `aggregate_key`
- `provenance`
- `payload`

This envelope does not replace typed contracts below; it standardizes ingestion boundaries for
externally-derived research outputs.

### External candidate initial event types (v1)

Allowed `event_type` values for the initial external-candidate program:
- `external_repo.audit_recorded`
- `market_slot.discovered`
- `market_snapshot.observed`
- `oracle_lag.observed`
- `candidate_signal.scored`
- `shadow_fill.simulated`
- `strategy_round.scored`
- `candidate_strategy.evaluated`
- `data_gap.detected`
- `feed_health.checked`
- `cross_venue.market_match_scored`

These are candidate/shadow/research-oriented signals and observations. They do not imply live
execution and must remain within paper/shadow governance boundaries.

`market_slot.discovered` candidate payload conventions:
- `asset` (`BTC|ETH|SOL` in v1 scope)
- `window` (`5m|15m` in v1 scope)
- `slot_start` (UTC ISO-8601)
- `slot_end` (UTC ISO-8601)
- `candidate_slug` (heuristic candidate string)
- `confidence` (heuristic confidence, conservative)
- `discovery_method` (e.g. `deterministic_slug_heuristic_v1`)
- `confirmed` (boolean; default false when no network confirmation)
- `source` (`slot_discovery_candidate`)

`market_snapshot.observed` candidate payload conventions:
- `asset`, `window`, `slot_start`, `slot_end`, `market_slug`
- `spot_price`
- `oracle_price` (nullable)
- `orderbook.best_bid`, `orderbook.best_ask`, `orderbook.mid_price`, `orderbook.spread_bps`
- `orderbook.depth_top_n`, `orderbook.imbalance_top_n`
- `features.spot_delta_bps`, `features.oracle_spot_delta_bps`
- `observation_latency_ms`
- `source_quality`

`candidate_signal.scored` candidate payload conventions:
- `asset`, `window`, `slot_start`, `slot_end`, `market_slug`
- `side` (`UP|DOWN`)
- `spot_delta_bps`, `oracle_delta_bps` (nullable), `book_mid_delta_bps`, `lag_gap_bps`
- `best_bid`, `best_ask`, `spread_bps`
- `confidence` (`0..1`)
- `raw_edge_bps`
- `rejected` (boolean), `reject_reason` (nullable string)
- `strategy_version`
- governance safety markers (`governance_state=candidate`, `promoted=false`, `executable=false`)

`shadow_fill.simulated` candidate payload conventions:
- `strategy_version`, `signal_event_id`, `asset`, `window`, `side`
- `limit_price`, `simulated_fill_price`
- `notional_usdc`, `size`
- `fee_usdc`, `slippage_usdc`
- `latency_ms`
- `fill_probability_estimate`, `fill_assumption`
- `rejected`, `reject_reason`

`strategy_round.scored` candidate payload conventions:
- `strategy_version`, `signal_event_id`, `fill_event_id`
- `outcome_known`, `resolved_side`
- `gross_pnl_usdc`, `net_pnl_usdc`
- `max_adverse_excursion`
- `notes`

`candidate_strategy.evaluated` payload conventions:
- `strategy_version`
- core metrics:
  - `signal_count`, `rejected_signal_count`, `rejection_rate`
  - `shadow_fill_count`, `shadow_fill_rate`
  - `resolved_round_count`
  - `gross_pnl_usdc`, `net_pnl_usdc`
  - `avg_net_edge_bps`, `median_net_edge_bps`
  - `max_drawdown_usdc`, `win_rate`, `expectancy_usdc`
  - `avg_fee_usdc`, `avg_slippage_usdc`, `avg_latency_ms`
  - `data_gap_count`, `stale_feed_count`
- governance outputs:
  - `status` (effective)
  - `suggested_status` (advisory)
- `auto_promoted` (always `false` by default)
- `reason`
- `thresholds` snapshot


`cross_venue.market_match_scored` payload conventions:
- `polymarket_market_id`, `kalshi_market_id`
- `polymarket_title`, `kalshi_title`
- `asset`, `window`, `strike`
- `confidence`
- `reasons`
- `rejected`, `reject_reason`

Backtest harness event types:
- `backtest.run_started`
- `backtest.run_completed`

Backtest events are offline replay artifacts and must never trigger live execution paths.

### Derived readiness semantics

- Readiness and lifecycle are derived by query-time interpretation of the current event log.
- Readiness is not persisted as an event in this slice.
- `weak`, `blocked`, `insufficient`, and `inconsistent` results express what the current contract
  can and cannot justify from the available log.
- `signal` and `decision` are also interpreted through a query-derived governance layer that
  normalizes whether each entity is eligible, weak, blocked, or inconsistent for advancement.
- `signal`, `decision`, and `order` can also be interpreted through a query-derived promotion /
  freeze policy layer that adds a non-persisted `frozen` status for prudential wait states.
- `decision.formed` is also interpreted through a query-derived promotion boundary / lineage view:
  upstream signal support, traced hypothesis, applicable vetoes, and downstream fill observation.
- `order.registered` introduces the minimum local contract for `order_id`.
- `order.submitted` introduces the minimum local lifecycle evidence that a known order was actually
  attempted for execution.
- `fill.received` also participates in a query-derived execution boundary view that interprets only
  observed linkage to decisions and external `order_id` references.
- `order_id` can also be inspected through an order-centric query layer that summarizes local
  lifecycle state from `order.registered`, `order.submitted`, and `fill.received`.

### Order ID policy

- Before `order.registered`, `order_id` is only an external observed reference.
- After `order.registered`, that `order_id` becomes a local contractual entity handle in this
  system.
- After `order.submitted`, that same local order also carries minimum local submission intent.
- `venue + order_id` is the minimum practical uniqueness scope in v1.

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
No broader `order.*` lifecycle family is introduced here beyond `order.registered`.

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
Ideally should also have coherent upstream lineage.
When `order.registered` exists, that order reference gains local contractual meaning.

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
`order.registered` is now the minimum order contract in this slice.
Upstream sufficiency remains partially documented debt until a fuller order lifecycle exists.

### `order.registered`

Name:
`order.registered`

Version:
`v1`

Producer:
Decision-to-execution boundary producer or execution-facing bookkeeping producer.

Expected consumers:
Execution boundary queries, decision lineage readers, replay, audit readers.

Purpose:
Declare that the system now recognizes a local order entity and its operational linkage.

Payload schema summary:
`order_id`, optional `decision_id`, `instrument`, `venue`.

Required fields:
`order_id`, `instrument`, `venue`

Optional fields:
`decision_id`

Timestamp semantics:
`occurred_at` marks when the local system registered the order contractually.

Idempotency expectations:
Stable by `venue + order_id`.

Ordering expectations:
Should follow `decision.formed` when a decision linkage exists.
Should generally precede downstream fills when the system has prior knowledge of the order, but
write-time historical enforcement is not introduced in this slice.

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
Blank order ID, blank instrument, blank venue, invalid UUID, contradictory `order_id`,
contradictory `decision_id`, conflicting replay under the same idempotency key.

Future evolution notes:
`order.submitted` now exists as the next minimal lifecycle step.
No `order.cancelled`, `order.completed`, or amendment lifecycle is introduced in this slice.

### `order.submitted`

Name:
`order.submitted`

Version:
`v1`

Producer:
Execution-facing runtime or boundary producer.

Expected consumers:
Execution boundary queries, decision lineage readers, governance readers, replay, audit readers.

Purpose:
Record that a locally known order was actually submitted or intentionally sent for execution.

Payload schema summary:
`order_id`, optional `decision_id`, `instrument`, `venue`.

Required fields:
`order_id`, `instrument`, `venue`

Optional fields:
`decision_id`

Timestamp semantics:
`occurred_at` marks when the local system recorded the submission intent.
It does not prove venue acceptance, matching, completion, or settlement.

Idempotency expectations:
Stable by `venue + order_id`.

Ordering expectations:
Should generally follow `order.registered` for the same local order.
Should generally precede downstream fills when the local system observed submission before
execution, but write-time historical enforcement is not introduced in this slice.

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
Blank order ID, blank instrument, blank venue, invalid UUID, contradictory `order_id`,
contradictory `decision_id`, conflicting replay under the same idempotency key.

Future evolution notes:
This event captures only minimal submission intent.
It does not introduce acceptance, rejection, cancellation, amendment, or completion semantics.
