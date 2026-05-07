# Changelog

## 2026-05-07

### feat: add external candidate event envelope
- Added shared Python event envelope utility at `agents/core/event_envelope.py`.
- Added strict required-field validator for external-candidate JSONL events.
- Added deterministic helpers for timestamp/idempotency/event_id/aggregate/provenance.
- Added idempotent JSONL append integration via existing `append_event_idempotent` store path.
- Added event fixtures at `examples/events/external_candidate_events_v1.jsonl`.
- Added tests in `tests/test_event_envelope.py`.
- Updated `EVENT_CATALOG.md`, `README.md`, and `ROADMAP.md` for Phase 0 status and event registry.
