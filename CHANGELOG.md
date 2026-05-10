# Changelog

## 2026-05-10

### feat: add Markov chain signal candidate
- Added `agents/markov_chain_signal_candidate.py` (candidate/shadow-only):
  - reads `market_snapshot.observed` by `asset/window`,
  - reconstructs slot outcomes from spot open/close (`binance_reconstructed_v1`),
  - builds state `(previous_outcome, momentum, volatility, book_skew)`,
  - estimates transition probabilities with Beta/Laplace smoothing,
  - applies hierarchical backoff (`full_state` -> `global_prior`),
  - scores UP/DOWN net edge vs orderbook ask + configurable cost buffer,
  - emits `candidate_signal.scored` with standard fields plus nested `model`, `pricing`, and `edge`,
  - rejects conservatively on missing orderbook, low evidence, spread/depth/expiry/stale/edge gates,
  - emits `data_gap.detected` when no snapshots are available.
- Added optional safe runner `scripts/run_markov_chain_signal_candidate.sh`.
- Added tests `tests/test_markov_chain_signal_candidate.py` covering smoothing, backoff, UP/DOWN scoring, rejection paths, envelope validity, and dry-run behavior.
- Added docs `docs/agents/markov_chain_signal_candidate.md`.
- Updated `README.md`, `ROADMAP.md`, and `EVENT_CATALOG.md` for Markov candidate scope and constraints.
### fix: make Polymarket read-only HTTP client Cloudflare-compatible
- Updated `agents/adapters/http_client.py` with host-aware default public headers for:
  - `gamma-api.polymarket.com`
  - `clob.polymarket.com`
- Read-only requests to those hosts now include:
  - `User-Agent: Mozilla/5.0`
  - `Accept: application/json`
- Kept headers configurable/overrideable per request and did not add auth headers.
- Kept non-Polymarket behavior unchanged (Binance requests do not get Polymarket `User-Agent` injection).
- Added adapter tests to validate host header injection, override behavior, and no unintended Binance header changes.

### fix: use canonical Polymarket updown slot slugs
- Switched `market_slot_discovery_candidate` default slug generation to canonical family:
  - `<asset>-updown-<window>-<unix_slot_start>` for BTC/ETH/SOL and 5m/15m.
- Added `slug_family`, `slug_timestamp`, `legacy_candidate_slug`, and canonical `discovery_method` in `market_slot.discovered` payload.
- Preserved legacy heuristic slug behind explicit `--slug-mode legacy` (no longer default in read-only path).
- Upgraded metadata adapter diagnostics:
  - exact slug 404 is preserved (`http_404` / `slug_not_found`) and no longer masked by fallback errors,
  - fallback errors are emitted separately via `adapter_errors` (`exact_slug`, `fallback_search`),
  - canonical slug parsing and nearby timestamp fallback reasoning (`nearby_canonical_slug_minus_one`, `nearby_canonical_slug_plus_one`).
- Updated smoke script output with canonical slugs, counters, adapter errors, and explicit `STRICT` pass line.
- Added/updated tests for canonical slug generation and 404-vs-403 metadata diagnosis.

### feat: add Polymarket read-only orderbook collection
- Upgraded `PolymarketMetadataAdapter` to resolve richer market metadata from Gamma:
  - market/condition IDs, question, active/closed/resolved flags,
  - outcome labels + `token_id` mapping,
  - end/resolution timestamps,
  - conservative fallback matching with `match_confidence` and `match_reason`,
  - structured metadata errors for not-found/ambiguous/low-confidence scenarios.
- Upgraded `PolymarketOrderBookAdapter` to fetch public CLOB books per token outcome with normalized fields:
  - `token_id`, `outcome`, `best_bid`, `best_ask`, `mid_price`, `spread_bps`,
  - `depth_top_n`, `imbalance_top_n`, `raw_levels_summary`, `source_quality`, latency,
  - structured errors (`http_404`, `timeout`, `dns`, `parse_error`, etc.).
- Extended `research_collector_candidate` read-only mode:
  - emits per-slot `data_gap.detected` for metadata/orderbook failures,
  - enriches `market_snapshot.observed` metadata/orderbook payloads,
  - reports `metadata_found_count`, `orderbook_observed_count`, `data_gap_count` in `feed_health.checked`.
- Added `scripts/run_polymarket_orderbook_smoke.sh`:
  - read-only by default, no credentials, supports `ASSETS`, `WINDOWS`, `STRICT=1`,
  - writes to `var/events/polymarket_orderbook_smoke.jsonl`,
  - prints event counters + last feed health summary and fails strict mode on required conditions.
- Added tests:
  - `tests/test_polymarket_read_only_adapters.py`
  - read-only collector success/failure coverage in `tests/test_research_collector_candidate.py`.
- Added docs:
  - `docs/data_sources/polymarket_read_only_orderbook.md`
  - updates in `README.md`, `ROADMAP.md`, `EVENT_CATALOG.md`.

## 2026-05-07

### fix: improve read-only smoke diagnostics
- Fixed `feed_health.checked` semantics in `read_only` mode:
  - `ok=true` only when all requested spot assets are observed,
  - `partial=true` when coverage is mixed,
  - explicit `successful_assets` and `failed_assets`.
- Added nested `adapter_errors` diagnostics per source and per asset/slug (e.g. `binance_spot.BTC`), avoiding empty errors on spot failures.
- Ensured `read_only` emits `market_snapshot.observed` only when real spot is available and emits `data_gap.detected` with slot context when missing.
- Improved `scripts/run_read_only_market_data_smoke.sh`:
  - truncates output file for fresh-run diagnostics,
  - prints event counts and last feed health summary,
  - supports `STRICT=1` to fail when no real spot snapshots are collected.
- Expanded tests in `tests/test_read_only_market_data_adapters.py` for all-fail, partial, success, and missing-spot cases.

### feat: add read-only market data adapters
- Added `agents/adapters/` package with:
  - `binance_spot_adapter.py` (public spot prices BTC/ETH/SOL),
  - `polymarket_metadata_adapter.py` (public slug metadata lookup),
  - `polymarket_orderbook_adapter.py` (read-only interface, safe-disabled behavior),
  - `http_client.py` (timeout + retries + structured errors).
- Extended `agents/research_collector_candidate.py` with `--data-mode mock|read_only` and adapter controls:
  - `--network-timeout-sec`, `--max-retries`, `--fail-soft`,
  - `--polymarket-metadata-enabled`, `--polymarket-orderbook-enabled`.
- Added event payload fields in `market_snapshot.observed`: `data_mode`, `adapter_versions`, `network_latency_ms`, `adapter_errors`, expanded `source_quality`.
- Added `scripts/run_read_only_market_data_smoke.sh` and docs `docs/data_sources/read_only_market_data_adapters.md`.
- Added tests:
  - `tests/test_binance_spot_adapter.py`
  - `tests/test_read_only_market_data_adapters.py`
  - updated `tests/test_research_collector_candidate.py`.

### docs: summarize external edge candidates v1
- Added final PR-style summary at `docs/research/external_edge_candidates_v1_final_summary.md`.
- Documented branch scope, event types, scripts, tests, mock/dry-run execution, no-live boundaries, limitations, and next milestones.
- Added summary pointer in `README.md`.
- Updated `ROADMAP.md` external candidate phases to reflect implemented status and separated next milestones.

### chore: harden external candidate tooling
- Fixed deterministic staleness evaluation in `agents/oracle_lag_signal_candidate.py` (uses replay/eval timestamp instead of wall clock) to avoid false stale rejections in offline/test workflows.
- Added cross-links across `README.md`, `ROADMAP.md`, and `EVENT_CATALOG.md` for external candidate docs.
- Added developer guide `docs/development/external_candidate_dev_guide.md` with event envelope, test, docs, no-live policy, and promotion checklist requirements.

### feat: add research-only cross venue matcher skeleton
- Added `agents/cross_venue_matcher_candidate.py` for deterministic Polymarket/Kalshi market-match scoring.
- Added event type `cross_venue.market_match_scored` to external candidate event registry.
- Added tests in `tests/test_cross_venue_matcher_candidate.py` for exact match and key rejection cases.
- Added docs `docs/research/cross_venue_market_matching.md` with explicit “Matching is not edge” guidance.
- Updated `README.md`, `ROADMAP.md`, and `EVENT_CATALOG.md`.

### feat: add external candidate reporting
- Added `agents/external_candidate_report.py` to generate markdown observability reports from external candidate JSONL events.
- Report coverage includes per-strategy, per-asset/window, and per-run views.
- Added sections for summary, signal counts, rejection reasons, fill assumptions, PnL, feed health, data gaps, governance status, and next action.
- Added optional launcher `scripts/generate_external_candidate_report.sh`.
- Added tests in `tests/test_external_candidate_report.py`.
- Added docs in `docs/reports/external_candidate_reports.md`.
- Updated `README.md` and `ROADMAP.md`.

### chore: wire external candidate pipeline
- Added optional external-edge controls to `scripts/run_pipeline.sh`:
  - `EXTERNAL_EDGE_CANDIDATES_ENABLED` (default `0`)
  - `EXTERNAL_EDGE_MOCK_MODE` (default `1`)
  - `EXTERNAL_EDGE_OUTPUT_JSONL` (default `./var/events/external_candidates.jsonl`)
- Wired external candidate stages behind opt-in flag in safe order:
  - `market_slot_discovery_candidate`
  - `research_collector_candidate`
  - `oracle_lag_signal_candidate`
  - `shadow_execution_simulator`
  - `external_candidate_scorecard`
- Added `scripts/run_external_edge_candidates.sh` as dedicated runner for the full chain.
- Added configuration docs in `docs/config/external_edge_candidates.md`.
- Updated `README.md` and `ROADMAP.md` with opt-in execution guidance.

### feat: add offline backtest harness for external candidates
- Added `agents/backtest_external_candidate.py` for deterministic offline replay of external-candidate data.
- Emits:
  - `backtest.run_started`
  - `candidate_signal.scored`
  - `shadow_fill.simulated`
  - `strategy_round.scored`
  - `candidate_strategy.evaluated`
  - `backtest.run_completed`
- Added markdown report generation and standalone backtest event log.
- Added tests `tests/test_backtest_external_candidate.py`.
- Added docs `docs/backtesting/external_candidate_backtest.md`.
- Added launcher `scripts/run_external_candidate_backtest.sh`.
- Updated `README.md`, `ROADMAP.md`, and `EVENT_CATALOG.md`.

### feat: add external candidate scorecard governance
- Added `agents/external_candidate_scorecard.py` to aggregate external candidate metrics and evaluate governance state.
- Emits `candidate_strategy.evaluated`.
- Includes conservative default thresholds and no auto-promotion policy (promotion only suggested).
- Added tests in `tests/test_external_candidate_scorecard.py`.
- Added docs `docs/agents/external_candidate_scorecard.md`.
- Added optional launcher `scripts/run_external_candidate_scorecard.sh`.
- Updated `README.md`, `ROADMAP.md`, and `EVENT_CATALOG.md`.

### feat: add conservative shadow execution simulator
- Added `agents/shadow_execution_simulator.py` to transform `candidate_signal.scored` into:
  - `shadow_fill.simulated`
  - `strategy_round.scored`
- Added conservative fee/slippage/fill-probability assumptions with pure helper functions.
- Added tests in `tests/test_shadow_execution_simulator.py`.
- Added optional launcher `scripts/run_shadow_execution_simulator.sh`.
- Added docs `docs/agents/shadow_execution_simulator.md`.
- Updated `README.md`, `ROADMAP.md`, and `EVENT_CATALOG.md`.

### feat: add oracle lag signal candidate
- Added `agents/oracle_lag_signal_candidate.py` (deterministic, shadow-only scorer).
- Reads `market_snapshot.observed` and emits:
  - `oracle_lag.observed`
  - `candidate_signal.scored`
- Added pure scoring helpers and conservative hard filters (stale, spread, price band, slot timing, data sufficiency).
- Added tests in `tests/test_oracle_lag_signal_candidate.py`.
- Added optional launcher `scripts/run_oracle_lag_signal_candidate.sh`.
- Added docs in `docs/agents/oracle_lag_signal_candidate.md`.
- Updated `README.md`, `ROADMAP.md`, and `EVENT_CATALOG.md`.

### feat: add research snapshot collector candidate
- Added `agents/research_collector_candidate.py` with modular adapters:
  - `SpotPriceAdapter`
  - `OraclePriceAdapter`
  - `OrderBookAdapter`
  - slot JSONL input reader (`market_slot.discovered`)
- Added pure feature helpers for spread/mid/depth/imbalance/spot-delta/oracle-spot-delta.
- Added tests in `tests/test_research_collector_candidate.py`.
- Added optional launcher `scripts/run_research_collector_candidate.sh`.
- Added docs in `docs/agents/research_collector_candidate.md`.
- Updated `README.md`, `ROADMAP.md`, and `EVENT_CATALOG.md`.

### feat: add market slot discovery candidate
- Added `agents/market_slot_discovery_candidate.py` for deterministic UTC slot discovery (BTC/ETH/SOL, 5m/15m).
- Added optional script `scripts/run_slot_discovery_candidate.sh` (not mandatory in core pipeline).
- Added tests `tests/test_market_slot_discovery_candidate.py`.
- Added docs `docs/agents/slot_discovery_candidate.md`.
- Updated `README.md`, `ROADMAP.md`, and `EVENT_CATALOG.md` for Phase 1 slot-discovery scope.

### feat: add external candidate event envelope
- Added shared Python event envelope utility at `agents/core/event_envelope.py`.
- Added strict required-field validator for external-candidate JSONL events.
- Added deterministic helpers for timestamp/idempotency/event_id/aggregate/provenance.
- Added idempotent JSONL append integration via existing `append_event_idempotent` store path.
- Added event fixtures at `examples/events/external_candidate_events_v1.jsonl`.
- Added tests in `tests/test_event_envelope.py`.
- Updated `EVENT_CATALOG.md`, `README.md`, and `ROADMAP.md` for Phase 0 status and event registry.
