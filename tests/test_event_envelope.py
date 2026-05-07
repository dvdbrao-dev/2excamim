from __future__ import annotations

import json
from pathlib import Path

import pytest

from agents.core.event_envelope import (
    append_event_jsonl,
    build_aggregate_key,
    build_event,
    build_idempotency_key,
    build_provenance,
    deterministic_event_id,
    validate_event,
)
from agents.core.event_store import load_jsonl


def _sample_event() -> dict:
    aggregate_key = build_aggregate_key("polymarket", "btc-5m", "5m")
    return build_event(
        event_type="oracle_lag.observed",
        aggregate_key=aggregate_key,
        payload={"lag_bps": 12.5, "market": "BTC"},
        provenance=build_provenance("oracle-lag-candidate", "shadow-research"),
        unique_components=["BTC", "5m", "2026-05-07T17:30:00Z"],
        timestamp="2026-05-07T17:30:00Z",
    )


def test_valid_event_passes_validation() -> None:
    event = _sample_event()
    validate_event(event, strict_type=True)


def test_missing_required_field_fails_validation() -> None:
    event = _sample_event()
    del event["aggregate_key"]

    with pytest.raises(ValueError, match="missing required fields"):
        validate_event(event)


def test_idempotency_key_is_deterministic() -> None:
    key_a = build_idempotency_key("oracle_lag.observed", "polymarket:btc:5m", ["BTC", "5m", "bar-1"])
    key_b = build_idempotency_key("oracle_lag.observed", "polymarket:btc:5m", ["BTC", "5m", "bar-1"])

    assert key_a == key_b


def test_event_id_is_deterministic() -> None:
    idem = build_idempotency_key("feed_health.checked", "feed:binance:btc", ["20260507T173000Z"])
    event_id_a = deterministic_event_id("feed_health.checked", "feed:binance:btc", idem)
    event_id_b = deterministic_event_id("feed_health.checked", "feed:binance:btc", idem)

    assert event_id_a == event_id_b


def test_jsonl_append_writes_valid_json(tmp_path: Path) -> None:
    store = tmp_path / "test_event_envelope.jsonl"
    event = _sample_event()
    persisted = append_event_jsonl(store, event)
    assert persisted is True

    records = load_jsonl(store)
    assert len(records) == 1
    assert records[0]["event_type"] == "oracle_lag.observed"

    # Ensure line is valid compact JSON.
    raw_line = store.read_text(encoding="utf-8").strip()
    parsed = json.loads(raw_line)
    assert parsed["event_id"] == event["event_id"]
