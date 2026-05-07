# External Candidate Development Guide

This guide defines the minimum engineering bar for adding new externally-inspired candidate components in 2EXCAMIM.

## Scope and policy

- Candidate/shadow/research only.
- No live trading.
- No order placement.
- No credentials in code.
- No default network dependency.

## How to add a new external-inspired candidate

1. Keep implementation small and deterministic.
2. Place agent under `agents/` using existing naming style.
3. Expose CLI with `--help` and safe defaults.
4. Prefer writing to JSONL event streams under `var/events/`.
5. Add optional launcher script under `scripts/` when useful.

## Required event envelope

Every emitted event must include:

- `event_type`
- `event_id`
- `timestamp`
- `idempotency_key`
- `aggregate_key`
- `provenance`
- `payload`

Use `agents/core/event_envelope.py`:

- `build_event(...)`
- `build_provenance(...)`
- `append_event_jsonl(...)`

Only allowed external event types must be used (`ALLOWED_EXTERNAL_EVENT_TYPES`).

## Required tests

At minimum add tests for:

- valid event emission and schema shape
- rejection path(s) for bad/insufficient input
- idempotency stability where relevant
- dry-run behavior when supported

Put tests under `tests/` following current naming (`test_<agent>.py`).

## Required docs

Update all of:

- `README.md`
- `ROADMAP.md`
- `EVENT_CATALOG.md`
- `CHANGELOG.md`
- specific agent/research docs under `docs/agents/`, `docs/research/`, `docs/reports/`, or `docs/backtesting/`

## No-live policy checklist

Before merge, confirm:

- no live order calls
- no private key usage
- no auto-promotion to executable/live state
- default mode remains safe (`dry-run`/`mock` when applicable)

## Promotion checklist (candidate -> suggested promoted)

Promotion can only be suggested, never auto-applied by default.
Require evidence from:

- sufficient sample size
- stable data quality (low stale feeds/data gaps)
- positive net expectancy after fees/slippage assumptions
- acceptable drawdown profile
- reproducible offline/backtest + shadow results
- explicit human review
