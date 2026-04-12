# ADR-001 Market Integration

## Status

Accepted

## Context

`2EXCAMIM` already separates:

- Python research and offline data work
- Rust runtime, event contracts, projections, queries, and JSONL persistence

Prediction markets are already present in the repository as an offline Python research area and as
the new Rust crate `crates/market-domain`. What does not exist yet is a canonical integration
contract that keeps provider-specific schemas out of the runtime domain.

We need a small, implementation-oriented foundation for future OpenFang-facing market ingestion and
signal generation work without claiming live adapters, network polling, schedulers, or storage
beyond what the repo already supports.

## Decision

We standardize prediction market integration around a canonical Rust domain crate:

- crate: `market-domain`
- responsibility: canonical market types and base event payloads
- role: boundary between external providers and the runtime event model

Provider adapters must translate external payloads into canonical domain values before any runtime
event is formed.

External provider schemas must not leak into the canonical domain. This includes:

- provider field names
- provider enum names
- provider-specific nested payload shapes
- provider-specific status codes used as runtime state

Canonical market facts are represented through:

- `MarketSource`
- `MarketStatus`
- `MarketSnapshot`
- `MarketQuote`
- `MarketActivity`
- `MarketSignal`

Base event payloads are represented through:

- `MarketSnapshotReceived`
- `MarketQuoteUpdated`
- `MarketActivityReceived`
- `MarketSignalGenerated`

The first event names for the integration boundary are:

- `market.snapshot.received`
- `market.quote.updated`
- `market.activity.received`
- `market.signal.generated`
- `market.ingestion.failed`

## Intended Flow

The intended v1 flow is narrow:

1. A provider-facing adapter decodes external data outside the canonical domain.
2. The adapter maps provider data into `market-domain` types.
3. The runtime validates canonical values.
4. The runtime emits canonical events into the existing append-only event model.
5. Queries, projections, and observability consume canonical events only.

This ADR does not introduce:

- live network ingestion
- new storage systems
- async processing
- a scheduler
- execution or order routing for prediction markets

## Consequences

Positive:

- provider churn stays isolated in adapter code
- runtime logic can depend on stable canonical types
- event contracts remain small and auditable
- tests can target canonical semantics instead of raw provider payloads

Tradeoffs:

- every provider needs an explicit translation layer
- some provider-specific nuance may need controlled normalization before persistence

## Implementation Notes

- Keep `market-domain` dependency-light and serde-friendly.
- Validation should stay lightweight and local to the canonical types.
- Provider-specific raw payload capture, if needed later, belongs in provenance or adapter logs, not
  in canonical structs.
- `market.ingestion.failed` is part of the event contract surface even if failure handling remains
  minimal in the first slice.

## Current Boundary

In the current repository state, this ADR should be read as a contract-first foundation.

It aligns with the existing architecture because:

- Rust already owns event contracts and append-only persistence
- Python already owns offline research for prediction markets
- the repo does not yet implement live OpenFang runtime ingestion for market providers

That limitation is intentional and remains unchanged by this ADR.
