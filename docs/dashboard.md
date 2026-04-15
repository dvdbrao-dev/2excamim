# Dashboard / Control Room v1.1

The control room is a static local HTML dashboard. It does not start a server and does not perform live trading. It reads existing canonical data and projections, then writes a local file that can be opened in a browser.

Run:

```bash
cargo run -- serve-dashboard \
  --store ./var/events.jsonl \
  --policy-file ./policies/confirmation_policy.json \
  --output ./var/dashboard/control_room.html
```

Then open `./var/dashboard/control_room.html` locally.

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
