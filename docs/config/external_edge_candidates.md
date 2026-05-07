# External Edge Candidates Configuration

The external-edge chain is optional and shadow-only.
Default behavior keeps the main pipeline unchanged.

## Environment variables

- `EXTERNAL_EDGE_CANDIDATES_ENABLED`:
  - `0` (default): do not run external-edge stages from `scripts/run_pipeline.sh`.
  - `1`: run the optional chain after core agents.
- `EXTERNAL_EDGE_MOCK_MODE`:
  - `1` (default): force safe mode (`--dry-run` and `--mock` where supported).
  - `0`: run without forced mock/dry-run flags.
- `EXTERNAL_EDGE_OUTPUT_JSONL`:
  - default: `./var/events/external_candidates.jsonl`
  - shared JSONL event stream for the external candidate stages.

## Stage order (when enabled)

1. `market_slot_discovery_candidate`
2. `research_collector_candidate`
3. `oracle_lag_signal_candidate`
4. `shadow_execution_simulator`
5. `external_candidate_scorecard`

`backtest_external_candidate` is intentionally excluded from default pipeline runs.

## Safe execution

- Main pipeline path remains unchanged unless `EXTERNAL_EDGE_CANDIDATES_ENABLED=1`.
- External stages never place orders and remain candidate/shadow-only.
- Failures are visible in logs. No silent failure mode is used.

## Commands

Default pipeline (external chain disabled):

```bash
bash scripts/run_pipeline.sh
```

Pipeline with external chain enabled in mock mode:

```bash
EXTERNAL_EDGE_CANDIDATES_ENABLED=1 EXTERNAL_EDGE_MOCK_MODE=1 bash scripts/run_pipeline.sh
```

Run only the external chain:

```bash
EXTERNAL_EDGE_MOCK_MODE=1 bash scripts/run_external_edge_candidates.sh
```
