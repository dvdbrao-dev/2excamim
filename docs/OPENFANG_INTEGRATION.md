# OpenFang Integration

## Overview

`2EXCAMIM` exports a read-only JSON snapshot of its internal strategy state to
`runtime/openfang_state.json` after each pipeline run. OpenFang can consume this
file directly or receive it via HTTP POST when `OPENFANG_INGEST_URL` is set.

The bridge is strictly read-only. It has zero ability to place orders, modify
events, or influence the pipeline.

---

## Schema (`schema_version: 1`)

```json
{
  "schema_version": 1,
  "system_id": "2excamim",
  "timestamp": "2026-01-01T00:00:00Z",
  "mode": "paper",
  "kill_switch_active": false,
  "strategies": [
    {
      "id": "crypto_adx_ema_pullback_v1",
      "status": "shadow",
      "metrics": {
        "signals_generated": 42,
        "fills": 8,
        "profit_factor": 1.35,
        "max_drawdown": 0.07,
        "confidence": 0.65,
        "expectancy": 12.4,
        "winrate": 0.625,
        "negative_windows": 0,
        "failed_runs": 0
      },
      "last_evaluated_at": "2026-01-01T06:00:00Z"
    }
  ],
  "alerts": [
    {
      "level": "info|warning|critical",
      "message": "Human-readable description",
      "ts": "2026-01-01T06:00:00Z"
    }
  ]
}
```

---

## Strategy Status Values

| Status      | Meaning |
|-------------|---------|
| `candidate` | Newly registered, fewer than 20 signals |
| `shadow`    | Paper-trading mode, building track record |
| `promoted`  | All promotion thresholds met; ready for live consideration |
| `frozen`    | Temporarily halted due to drawdown or negative windows |
| `rejected`  | Permanently decommissioned |

---

## What OpenFang CAN read

- Current status of every crypto strategy
- Scorecard metrics: PnL, win-rate, drawdown, profit factor, confidence
- Kill switch state
- Structured alerts (info / warning / critical)
- Timestamps of last evaluation

## What OpenFang CANNOT do

- **Write** to `var/events.jsonl` or any event store
- **Trigger** orders or position changes
- **Modify** strategy state or the registry
- **Access** the live gateway, confirmation agent, sizing agent, veto agent, or exit agent
- **Override** the kill switch

The bridge enforces this at the code level: those modules are never imported.

---

## Kill Switch (`kill_switch_active`)

When `kill_switch_active` is `true`:

- The pipeline is paused. No new signals or decisions are being generated.
- OpenFang should **not** expect fresh data until the switch is cleared.
- A `critical`-level alert will be present in `alerts`.
- The switch is cleared by removing `var/.kill_switch` on the 2EXCAMIM host.

---

## HTTP Push (Optional)

If the environment variable `OPENFANG_INGEST_URL` is set, the bridge will POST
the JSON payload to that URL after writing the local file.

- **Method:** `POST`
- **Content-Type:** `application/json`
- **Timeout:** 5 seconds
- **Retries:** up to 2 attempts
- A POST failure never aborts the pipeline — the local file is always written first.

---

## Schema Versioning Policy

| Change type | Action |
|-------------|--------|
| New optional field added | No version bump |
| Field renamed or removed | Increment `schema_version` |
| Type of existing field changed | Increment `schema_version` |

OpenFang consumers **must** check `schema_version` before parsing. Unknown
versions should be handled gracefully (log and skip rather than crash).

---

## Local File Path

```
runtime/openfang_state.json
```

This file is gitignored and regenerated on every pipeline run. Do not rely on
its presence between runs.
