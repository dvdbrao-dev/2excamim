# Operational Summary

`generate-operational-summary` produces a compact local artifact for reviewing the latest paper run and current canonical paper state.

The summary reads existing state only:

- latest pipeline report from `dashboard/latest_pipeline.json` beside the event store
- canonical JSONL events through the paper ledger projection
- readiness artifact from `var/readiness/confirmation_readiness.json` if present
- policy file passed with `--policy-file` if present

Default output is JSON:

```sh
twoexcamim generate-operational-summary --store ./var/events.jsonl
```

Markdown can be generated for human review or archival:

```sh
twoexcamim generate-operational-summary \
  --store ./var/events.jsonl \
  --output ./var/operations/latest_summary.md \
  --format markdown
```

After a successful non-dry-run `run-paper-pipeline`, the runtime also writes `operations/latest_summary.json` beside the event store. The artifact is observational only; it does not submit orders, alter risk rules, or create new trading state.
