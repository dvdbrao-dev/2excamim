from __future__ import annotations

import json
from argparse import Namespace
from pathlib import Path

from agents.core.event_store import load_jsonl
from agents.research_collector_candidate import run


def _write_slots(path: Path) -> None:
    event = {
        "event_type": "market_slot.discovered",
        "event_id": "00000000-0000-0000-0000-000000000101",
        "timestamp": "2026-05-07T18:00:00Z",
        "idempotency_key": "market_slot.discovered:v1:test2",
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


def test_read_only_spot_only_emits_market_snapshot(tmp_path: Path) -> None:
    slots = tmp_path / "slots.jsonl"
    out = tmp_path / "events.jsonl"
    _write_slots(slots)

    args = Namespace(
        input_slots_jsonl=str(slots),
        output_jsonl=str(out),
        assets="BTC",
        windows="5m",
        sample_count=1,
        sample_interval_ms=1,
        dry_run=False,
        mock=False,
        data_mode="read_only",
        network_timeout_sec=0.001,
        max_retries=0,
        fail_soft=True,
        polymarket_metadata_enabled=False,
        polymarket_orderbook_enabled=False,
    )
    run(args)
    rows = load_jsonl(out)
    assert any(r.get("event_type") == "feed_health.checked" for r in rows)


def test_missing_polymarket_metadata_emits_data_gap(tmp_path: Path) -> None:
    slots = tmp_path / "slots.jsonl"
    out = tmp_path / "events.jsonl"
    _write_slots(slots)

    args = Namespace(
        input_slots_jsonl=str(slots),
        output_jsonl=str(out),
        assets="BTC",
        windows="5m",
        sample_count=1,
        sample_interval_ms=1,
        dry_run=False,
        mock=False,
        data_mode="read_only",
        network_timeout_sec=0.001,
        max_retries=0,
        fail_soft=True,
        polymarket_metadata_enabled=True,
        polymarket_orderbook_enabled=False,
    )
    run(args)
    rows = load_jsonl(out)
    gaps = [r for r in rows if r.get("event_type") == "data_gap.detected"]
    assert any(g["payload"].get("gap_type") == "missing_polymarket_metadata" for g in gaps)
