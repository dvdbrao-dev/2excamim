# Changelog

## 2026-05-07

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
