# External Candidate Offline Backtest

Harness: `agents/backtest_external_candidate.py`

- Offline-only replay over JSONL snapshots/signals.
- No network, no keys, no live execution.
- Deterministic sequencing and explicit separation of:
  - signal time
  - simulated fill time
  - resolution/outcome time

Outputs:
- Event log: `var/events/external_backtest.jsonl`
- Markdown report: `reports/external_candidates/oracle_lag_v1_backtest.md`

Supports optional resolution fixture (`--resolution-jsonl`) when historical outcomes are available.
