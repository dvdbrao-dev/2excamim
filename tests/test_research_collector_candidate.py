from __future__ import annotations

import json
from argparse import Namespace
from pathlib import Path

from agents.core.event_store import load_jsonl
from agents.research_collector_candidate import (
    OrderBookSnapshot,
    OrderLevel,
    best_ask,
    best_bid,
    mid_price,
    oracle_spot_delta_bps,
    orderbook_imbalance_top_n,
    run,
    spread_bps,
)


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
    book = OrderBookSnapshot(
        bids=[OrderLevel(price=0.47, size=100), OrderLevel(price=0.46, size=50)],
        asks=[OrderLevel(price=0.49, size=80), OrderLevel(price=0.50, size=70)],
    )
    bid = best_bid(book)
    ask = best_ask(book)
    assert bid == 0.47
    assert ask == 0.49
    assert mid_price(bid, ask) == 0.48
    assert round(spread_bps(bid, ask) or 0.0, 2) == 416.67
    assert round(orderbook_imbalance_top_n(book, 2) or 0.0, 5) == 0.0
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
        mock=True,
    )
    summary = run(args)
    assert summary["events_persisted"] >= 2

    rows = load_jsonl(out)
    assert any(row.get("event_type") == "market_snapshot.observed" for row in rows)


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
        mock=True,
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
        mock=True,
    )
    summary = run(args)
    assert summary["events_generated"] >= 1
    assert not out.exists()
