# MARKET_EVENT_CATALOG

## Scope

This catalog defines the first canonical prediction market events intended for the Rust runtime.
It aligns with `crates/market-domain` and with the repository's existing append-only event model.

These events describe domain facts. They do not imply live ingestion, streaming, scheduling, or new
storage infrastructure.

## Cross-Cutting Rules

### Canonical payload rule

- event payloads must use canonical domain types
- external provider schemas must not appear in canonical payloads
- provider-specific decoding belongs in adapter code before event formation

### Validation rule

- payloads should be validated before persistence
- invalid provider data should not be normalized silently into contradictory canonical facts
- failure to translate a provider payload is itself an observable event candidate

### Timestamp rule

- provider observation timestamps stay in the canonical payload value
- runtime receipt or emission timestamps stay on the event payload wrapper
- these timestamps are not interchangeable

### Source rule

- `MarketSource` captures normalized origin
- source-specific metadata, if retained later, belongs in provenance or adapter diagnostics

## Event Specifications

### `market.snapshot.received`

Purpose:
Record that a canonical market snapshot was received and accepted by the runtime boundary.

Payload:
`MarketSnapshotReceived`

Core fields:
`source_event_id`, `snapshot`, `received_at`

Expected producer:
Provider adapter or offline ingestion boundary

Expected consumers:
Snapshot projections, query surfaces, observability, downstream signal generation

Notes:

- `snapshot` must be a validated `MarketSnapshot`
- provider-native snapshot shapes must be translated before this event is formed

### `market.quote.updated`

Purpose:
Record a canonical quote change for a market.

Payload:
`MarketQuoteUpdated`

Core fields:
`source_event_id`, `quote`, `received_at`

Expected producer:
Provider adapter or offline ingestion boundary

Expected consumers:
Quote projections, signal generation, observability

Notes:

- `quote` must be a validated `MarketQuote`
- this event expresses quote state, not trade execution

### `market.activity.received`

Purpose:
Record canonical market activity such as trade, volume, open interest, or resolution facts.

Payload:
`MarketActivityReceived`

Core fields:
`source_event_id`, `activity`, `received_at`

Expected producer:
Provider adapter or offline ingestion boundary

Expected consumers:
Activity projections, signal generation, observability

Notes:

- `activity` must be a validated `MarketActivity`
- `Trade` activity requires both price and quantity

### `market.signal.generated`

Purpose:
Record a canonical derived signal for a prediction market.

Payload:
`MarketSignalGenerated`

Core fields:
`source_event_id`, `signal`, `emitted_at`

Expected producer:
Research translation layer or runtime-side derived logic

Expected consumers:
Queries, projections, observability, future decision logic

Notes:

- `signal` must be a validated `MarketSignal`
- this event is canonical and provider-agnostic even when its source data came from one provider

### `market.ingestion.failed`

Purpose:
Record that provider data or an offline market payload could not be translated into the canonical
domain.

Payload shape:
To be defined in the runtime contract layer; it should remain small and operational.

Minimum fields expected:

- `source`
- `source_event_id` when available
- `failure_stage`
- `error_code`
- `error_message`
- `occurred_at`

Expected producer:
Provider adapter or ingestion boundary

Expected consumers:
Observability, diagnostics, retry analysis

Notes:

- this event exists to make ingestion failures explicit without leaking raw provider schemas into
  the canonical market domain
- raw payload capture, if later required, should stay outside canonical domain structs

## Implementation Guidance

- Start with the four typed payloads already present in `market-domain`.
- Keep `market.ingestion.failed` outside the canonical value layer unless a stable operational
  payload is justified.
- Reuse the repository's existing event envelope, JSONL persistence, projections, and query style
  when the runtime integration is implemented.
- Do not bypass canonical mapping by storing raw provider payloads as if they were domain events.
