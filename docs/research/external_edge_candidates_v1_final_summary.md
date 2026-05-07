# External Edge Candidates V1 — Final Branch Summary

## Problem
2EXCAMIM needed a controlled way to extract hypotheses from external repos without importing untrusted execution paths, changing core architecture, or enabling live trading by accident.

## Why external repos were not imported wholesale
- Preserve 2EXCAMIM contracts (event envelope + JSONL append-only flow) instead of inheriting foreign architecture.
- Avoid hidden live-trading behavior, credential handling, and unclear risk controls.
- Keep deterministic, testable, reviewable components aligned with existing pipeline governance.

## Implemented components
- `agents/core/event_envelope.py`
- `agents/market_slot_discovery_candidate.py`
- `agents/research_collector_candidate.py`
- `agents/oracle_lag_signal_candidate.py`
- `agents/shadow_execution_simulator.py`
- `agents/external_candidate_scorecard.py`
- `agents/backtest_external_candidate.py`
- `agents/external_candidate_report.py`
- `agents/cross_venue_matcher_candidate.py`

## Event types added
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
- `backtest.run_started`
- `backtest.run_completed`

## Scripts added
- `scripts/run_slot_discovery_candidate.sh`
- `scripts/run_research_collector_candidate.sh`
- `scripts/run_oracle_lag_signal_candidate.sh`
- `scripts/run_shadow_execution_simulator.sh`
- `scripts/run_external_candidate_scorecard.sh`
- `scripts/run_external_candidate_backtest.sh`
- `scripts/run_external_edge_candidates.sh`
- `scripts/generate_external_candidate_report.sh`

## Docs added/updated
- `docs/research/external_edge_candidates_v1.md`
- `docs/adr/ADR-externally-derived-edge-candidates-v1.md`
- `docs/agents/slot_discovery_candidate.md`
- `docs/agents/research_collector_candidate.md`
- `docs/agents/oracle_lag_signal_candidate.md`
- `docs/agents/shadow_execution_simulator.md`
- `docs/agents/external_candidate_scorecard.md`
- `docs/backtesting/external_candidate_backtest.md`
- `docs/reports/external_candidate_reports.md`
- `docs/research/cross_venue_market_matching.md`
- `docs/development/external_candidate_dev_guide.md`

## Tests added
- `tests/test_event_envelope.py`
- `tests/test_market_slot_discovery_candidate.py`
- `tests/test_research_collector_candidate.py`
- `tests/test_oracle_lag_signal_candidate.py`
- `tests/test_shadow_execution_simulator.py`
- `tests/test_external_candidate_scorecard.py`
- `tests/test_backtest_external_candidate.py`
- `tests/test_external_candidate_report.py`
- `tests/test_cross_venue_matcher_candidate.py`

## How to run in mock mode
```bash
EXTERNAL_EDGE_CANDIDATES_ENABLED=1 EXTERNAL_EDGE_MOCK_MODE=1 bash scripts/run_pipeline.sh
```

```bash
EXTERNAL_EDGE_MOCK_MODE=1 bash scripts/run_external_edge_candidates.sh
```

## How to run in dry-run mode
```bash
python3 agents/market_slot_discovery_candidate.py --dry-run
python3 agents/research_collector_candidate.py --mock --dry-run
python3 agents/oracle_lag_signal_candidate.py --dry-run
python3 agents/shadow_execution_simulator.py --dry-run
python3 agents/external_candidate_scorecard.py --dry-run
python3 agents/cross_venue_matcher_candidate.py \
  --polymarket-markets-json ./data/polymarket_markets.json \
  --kalshi-markets-json ./data/kalshi_markets.json \
  --dry-run
```

## What remains research-only
- Cross-venue matching output (`cross_venue.market_match_scored`) is research evidence only.
- Candidate scorecard can suggest promotion but does not auto-promote to live execution.
- External candidate chain remains opt-in and isolated from default execution path.

## What is explicitly not live
- No order placement or exchange execution from external candidate agents.
- No credential-required external execution path added in this branch.
- No automatic enabling of candidate chain in default pipeline (`EXTERNAL_EDGE_CANDIDATES_ENABLED=0` by default).

## Known limitations
- Research quality depends on input snapshot coverage and data freshness.
- Mock/dry-run paths validate logic and contracts, not market execution quality.
- `ruff`/`mypy` may be unavailable depending on runtime environment.
- Cross-venue matcher is structural matching, not validated tradable edge.

## Next recommended milestones
1. Expand deterministic datasets/fixtures for broader stress scenarios.
2. Tighten governance thresholds from observed offline evidence windows.
3. Add periodic external candidate report generation to operational cadence.
4. Keep candidate/shadow-only scope until scorecard + replay evidence stays stable across multiple windows.
