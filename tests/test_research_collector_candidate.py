from __future__ import annotations

import json
from argparse import Namespace
from collections import deque
from pathlib import Path

from agents.core.event_store import load_jsonl
from agents.research_collector_candidate import oracle_spot_delta_bps, run, spot_delta_bps


def _write_slots(path: Path) -> None:
    event = {
        "event_type": "market_slot.discovered",
        "event_id": "00000000-0000-0000-0000-000000000001",
        "timestamp": "2026-05-07T18:00:00Z",
        "idempotency_key": "market_slot.discovered:v1:test",
        "aggregate_key": "polymarket:btc:5m",
        "provenance": {"agent_id": "test", "source": "test", "generated_at": "2026-05-07T18:00:00Z"},
        "payload": {
            "asset": "BTC",
            "window": "5m",
            "slot_start": "2026-05-07T18:00:00Z",
            "slot_end": "2026-05-07T18:05:00Z",
            "candidate_slug": "btc-up-or-down-may-07-1800-utc-5m",
        },
    }
    path.write_text(json.dumps(event) + "\n", encoding="utf-8")


def test_feature_helpers_calculate_correctly() -> None:
    assert round((spot_delta_bps(deque([100.0, 101.0])) or 0.0), 2) == 100.0
    assert round(oracle_spot_delta_bps(101.0, 100.0) or 0.0, 2) == 100.0


def test_mock_collector_emits_valid_jsonl(tmp_path: Path) -> None:
    slots = tmp_path / "slots.jsonl"
    out = tmp_path / "external_candidates.jsonl"
    _write_slots(slots)

    args = Namespace(
        input_slots_jsonl=str(slots),
        output_jsonl=str(out),
        assets="BTC",
        windows="5m",
        sample_count=2,
        sample_interval_ms=1000,
        dry_run=False,
        mock=False,
        data_mode="mock",
        network_timeout_sec=5.0,
        max_retries=2,
        fail_soft=False,
        polymarket_metadata_enabled=False,
        polymarket_orderbook_enabled=False,
    )
    summary = run(args)
    assert summary["events_persisted"] >= 2

    rows = load_jsonl(out)
    snapshots = [row for row in rows if row.get("event_type") == "market_snapshot.observed"]
    health = [row for row in rows if row.get("event_type") == "feed_health.checked"][-1]["payload"]
    assert snapshots
    assert snapshots[0]["payload"]["source_quality"] == "mock"
    assert health["ok"] is True
    assert health["partial"] is False


def test_missing_data_emits_data_gap_detected(tmp_path: Path) -> None:
    out = tmp_path / "external_candidates.jsonl"
    args = Namespace(
        input_slots_jsonl=str(tmp_path / "missing-slots.jsonl"),
        output_jsonl=str(out),
        assets="BTC",
        windows="5m",
        sample_count=1,
        sample_interval_ms=1000,
        dry_run=False,
        mock=False,
        data_mode="read_only",
        network_timeout_sec=0.01,
        max_retries=0,
        fail_soft=True,
        polymarket_metadata_enabled=True,
        polymarket_orderbook_enabled=False,
    )
    run(args)
    rows = load_jsonl(out)
    assert any(row.get("event_type") == "data_gap.detected" for row in rows)


def test_event_idempotency_stable(tmp_path: Path) -> None:
    slots = tmp_path / "slots.jsonl"
    out = tmp_path / "external_candidates.jsonl"
    _write_slots(slots)

    args = Namespace(
        input_slots_jsonl=str(slots),
        output_jsonl=str(out),
        assets="BTC",
        windows="5m",
        sample_count=1,
        sample_interval_ms=1000,
        dry_run=False,
        mock=False,
        data_mode="mock",
        network_timeout_sec=5.0,
        max_retries=2,
        fail_soft=False,
        polymarket_metadata_enabled=False,
        polymarket_orderbook_enabled=False,
    )
    run(args)
    first = load_jsonl(out)
    run(args)
    second = load_jsonl(out)
    assert len(first) == len(second)


def test_dry_run_behavior(tmp_path: Path) -> None:
    slots = tmp_path / "slots.jsonl"
    out = tmp_path / "external_candidates.jsonl"
    _write_slots(slots)

    args = Namespace(
        input_slots_jsonl=str(slots),
        output_jsonl=str(out),
        assets="BTC",
        windows="5m",
        sample_count=1,
        sample_interval_ms=1000,
        dry_run=True,
        mock=False,
        data_mode="mock",
        network_timeout_sec=5.0,
        max_retries=2,
        fail_soft=False,
        polymarket_metadata_enabled=False,
        polymarket_orderbook_enabled=False,
    )
    summary = run(args)
    assert summary["events_generated"] >= 1
    assert not out.exists()
