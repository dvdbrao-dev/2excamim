# Dashboard / Control Room v1.2

The control room stays paper-first and read-only, but `serve-dashboard` can now run as a tiny HTTP server for browser access from a VPS. It still reads canonical data and projections only; it does not perform live trading.

Run:

```bash
cargo run -- serve-dashboard \
  --store ./var/events.jsonl \
  --policy-file ./policies/confirmation_policy.json \
  --output ./var/dashboard/control_room.html \
  --host 0.0.0.0 \
  --port 8000
```

Then open `http://SERVER_IP:8000/` in the browser.

For a static local export, omit `--host` and `--port`; `serve-dashboard` will still write the HTML file to `--output`.

Data sources:

- canonical JSONL event store
- paper ledger projection derived from canonical events
- latest pipeline report at `var/dashboard/latest_pipeline.json`, when produced by `run-paper-pipeline`
- operational summary artifact at `var/operations/latest_summary.json`, when produced by `run-paper-pipeline`
- readiness artifact at `./var/readiness/confirmation_readiness.json`, when present
- active confirmation policy file, when provided with `--policy-file`

Usability sections:

- pipeline recap with stage order and key counters
- positions with a clearer open-position table and closed-position summary
- recent activity with orders, fills, and latest risk blocks when available
- governance with promoted / frozen / candidate / experimental breakdown and active policy summary

The dashboard is intentionally read-only. It is a human-facing operational summary, not a trading interface and not a new source of truth.

Deployment notes:

- `deploy/systemd/twoexcamim-dashboard.service` serves the dashboard on `0.0.0.0:8000`
- `deploy/systemd/twoexcamim-paper-pipeline.service` runs the paper pipeline as a oneshot job
- `deploy/systemd/twoexcamim-paper-pipeline.timer` schedules the paper pipeline on a repeat interval
- each pipeline run refreshes `dashboard/latest_pipeline.json`, `operations/latest_summary.json`, and `dashboard/control_room.html`
