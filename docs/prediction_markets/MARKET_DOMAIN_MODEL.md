# MARKET_DOMAIN_MODEL

## Scope

This document defines the canonical prediction market domain model for integration into the Rust
runtime. It aligns with `crates/market-domain` and stays intentionally small.

It does not define:

- provider transport details
- network clients
- storage beyond the existing append-only event model
- execution semantics
- portfolio, position, or settlement logic

## Boundary Rule

External provider schemas must not leak into the canonical domain.

Adapters may decode provider payloads, but the runtime contract must use canonical values only.
Canonical consumers should not need to know whether a fact came from Polymarket, Kalshi, or another
provider beyond the normalized `MarketSource` value.

## Core Types

### `MarketSource`

Purpose:
Identifies the normalized origin of a market fact or signal.

Current values:

- `Polymarket`
- `Kalshi`
- `Manual`
- `Synthetic`

Notes:
Used for source attribution, not for provider-specific branching inside the domain model.

### `MarketStatus`

Purpose:
Represents the canonical lifecycle state of a market.

Current values:

- `Open`
- `Closed`
- `Resolved`
- `Suspended`
- `Cancelled`

Notes:
Provider-native status vocabularies must be translated into this enum before entering the runtime.

### `MarketSnapshot`

Purpose:
Represents point-in-time market state.

Key fields:
`market_id`, `source`, `title`, `status`, optional `best_bid`, optional `best_ask`, optional
`last_price`, optional `volume`, `observed_at`

Validation notes:

- `market_id` and `title` must not be blank
- price-like probability values must be in `[0,1]`
- `volume`, when present, must be positive
- `best_bid` must not be greater than `best_ask`

### `MarketQuote`

Purpose:
Represents a tradable quote update for a market.

Key fields:
`market_id`, `source`, `bid_price`, `ask_price`, optional `last_price`, `observed_at`

Validation notes:

- `market_id` must not be blank
- prices must be in `[0,1]`
- `bid_price` must not be greater than `ask_price`

### `MarketActivity`

Purpose:
Represents a normalized market activity sample.

Key fields:
`market_id`, `source`, `kind`, optional `price`, optional `quantity`, `observed_at`

Activity kinds:

- `Trade`
- `Volume`
- `OpenInterest`
- `Resolution`

Validation notes:

- `market_id` must not be blank
- `price`, when present, must be in `[0,1]`
- `quantity`, when present, must be positive
- trade activity requires both `price` and `quantity`

### `MarketSignal`

Purpose:
Represents a canonical derived signal about a market.

Key fields:
`signal_id`, `market_id`, `source`, `signal_name`, `direction`, `confidence`, optional
`rationale`, `generated_at`

Signal directions:

- `Yes`
- `No`
- `Neutral`

Validation notes:

- `signal_id`, `market_id`, and `signal_name` must not be blank
- `confidence` must be in `[0,1]`
- `rationale`, when present, must not be blank

## Event Payload Types

The crate also defines base payloads for the first canonical events:

- `MarketSnapshotReceived`
- `MarketQuoteUpdated`
- `MarketActivityReceived`
- `MarketSignalGenerated`

Each payload:

- carries a `source_event_id`
- embeds one validated canonical domain value
- records the runtime-side receipt or emission timestamp

## Identity Guidance

- `market_id` is the canonical identifier inside the source namespace
- `signal_id` is the canonical identifier for a derived signal
- canonical IDs may be derived from provider identifiers, but only after explicit normalization

If a future runtime envelope adds linkage or provenance, those records must reference canonical IDs,
not provider-native payload fragments.

## Design Constraints

- keep structs small and serializable
- keep validation local and explicit
- prefer additive evolution over deep inheritance or generic abstraction
- preserve the append-only event model already used by the runtime

## Out of Scope for v1

- market order books
- position accounting
- live feed subscription state
- provider auth or API concerns
- storage projections specific to prediction markets
