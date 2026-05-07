from __future__ import annotations

import json
from argparse import Namespace
from pathlib import Path

from agents.core.event_store import load_jsonl
from agents.oracle_lag_signal_candidate import run


def _snapshot_event(
    ts: str,
    slot_start: str,
    slot_end: str,
    spot: float,
    mid: float,
    bid: float,
    ask: float,
    spread_bps: float,
    oracle: float | None = None,
) -> dict:
    return {
        "event_type": "market_snapshot.observed",
        "event_id": ts.replace(":", "").replace("-", ""),
        "timestamp": ts,
        "idempotency_key": f"snap:{ts}",
        "aggregate_key": "polymarket:btc:5m",
        "provenance": {"agent_id": "test", "source": "test", "generated_at": ts},
        "payload": {
            "asset": "BTC",
            "window": "5m",
            "slot_start": slot_start,
            "slot_end": slot_end,
            "market_slug": "btc-up-or-down-test",
            "spot_price": spot,
            "oracle_price": oracle,
            "orderbook": {
                "best_bid": bid,
                "best_ask": ask,
                "mid_price": mid,
                "spread_bps": spread_bps,
            },
        },
    }


def _write(path: Path, rows: list[dict]) -> None:
    path.write_text("\n".join(json.dumps(r) for r in rows) + "\n", encoding="utf-8")


def _args(input_path: Path, output_path: Path) -> Namespace:
    return Namespace(
        input_jsonl=str(input_path),
        output_jsonl=str(output_path),
        asset="BTC",
        window="5m",
        strategy_version="oracle_lag_v1",
        min_spot_move_bps=8.0,
        max_market_price=0.92,
        min_edge_bps=5.0,
        dry_run=False,
    )


def test_positive_lag_creates_non_rejected_signal(tmp_path: Path) -> None:
    input_path = tmp_path / "in.jsonl"
    output_path = tmp_path / "out.jsonl"
    _write(
        input_path,
        [
            _snapshot_event(
                "2026-05-07T18:00:10Z",
                "2026-05-07T18:00:00Z",
                "2026-05-07T23:59:00Z",
                100.0,
                0.48,
                0.47,
                0.49,
                120.0,
                100.2,
            ),
            _snapshot_event(
                "2026-05-07T18:01:10Z",
                "2026-05-07T18:00:00Z",
                "2026-05-07T23:59:00Z",
                100.25,
                0.4802,
                0.4701,
                0.4903,
                130.0,
                100.45,
            ),
        ],
    )

    run(_args(input_path, output_path))
    rows = load_jsonl(output_path)
    scored = [r for r in rows if r.get("event_type") == "candidate_signal.scored"][0]
    assert scored["payload"]["rejected"] is False


def test_wide_spread_rejects(tmp_path: Path) -> None:
    input_path = tmp_path / "in.jsonl"
    output_path = tmp_path / "out.jsonl"
    _write(
        input_path,
        [
            _snapshot_event("2026-05-07T18:00:10Z", "2026-05-07T18:00:00Z", "2026-05-07T23:59:00Z", 100.0, 0.48, 0.47, 0.49, 120.0),
            _snapshot_event("2026-05-07T18:01:10Z", "2026-05-07T18:00:00Z", "2026-05-07T23:59:00Z", 100.3, 0.4801, 0.40, 0.60, 1200.0),
        ],
    )
    run(_args(input_path, output_path))
    rows = load_jsonl(output_path)
    scored = [r for r in rows if r.get("event_type") == "candidate_signal.scored"][0]
    assert scored["payload"]["rejected"] is True
    assert scored["payload"]["reject_reason"] == "spread_too_wide"


def test_stale_snapshot_rejects(tmp_path: Path) -> None:
    input_path = tmp_path / "in.jsonl"
    output_path = tmp_path / "out.jsonl"
    _write(
        input_path,
        [
            _snapshot_event("2020-05-07T18:00:10Z", "2020-05-07T18:00:00Z", "2030-05-07T23:59:00Z", 100.0, 0.48, 0.47, 0.49, 100.0),
            _snapshot_event("2020-05-07T18:01:10Z", "2020-05-07T18:00:00Z", "2030-05-07T23:59:00Z", 100.4, 0.4802, 0.47, 0.49, 110.0),
        ],
    )
    run(_args(input_path, output_path))
    rows = load_jsonl(output_path)
    scored = [r for r in rows if r.get("event_type") == "candidate_signal.scored"][0]
    assert scored["payload"]["reject_reason"] == "stale_feed"


def test_too_close_to_resolution_rejects(tmp_path: Path) -> None:
    input_path = tmp_path / "in.jsonl"
    output_path = tmp_path / "out.jsonl"
    _write(
        input_path,
        [
            _snapshot_event("2026-05-07T18:00:10Z", "2026-05-07T18:00:00Z", "2026-05-07T18:00:20Z", 100.0, 0.48, 0.47, 0.49, 100.0),
            _snapshot_event("2026-05-07T18:00:11Z", "2026-05-07T18:00:00Z", "2026-05-07T18:00:20Z", 100.4, 0.4802, 0.47, 0.49, 100.0),
        ],
    )
    run(_args(input_path, output_path))
    rows = load_jsonl(output_path)
    scored = [r for r in rows if r.get("event_type") == "candidate_signal.scored"][0]
    assert scored["payload"]["reject_reason"] == "market_too_close_to_resolution"


def test_idempotency_and_schema_valid(tmp_path: Path) -> None:
    input_path = tmp_path / "in.jsonl"
    output_path = tmp_path / "out.jsonl"
    _write(
        input_path,
        [
            _snapshot_event("2026-05-07T18:00:10Z", "2026-05-07T18:00:00Z", "2026-05-07T23:59:00Z", 100.0, 0.48, 0.47, 0.49, 120.0),
            _snapshot_event("2026-05-07T18:01:10Z", "2026-05-07T18:00:00Z", "2026-05-07T23:59:00Z", 100.25, 0.4802, 0.4701, 0.4903, 130.0),
        ],
    )
    args = _args(input_path, output_path)
    run(args)
    first = load_jsonl(output_path)
    run(args)
    second = load_jsonl(output_path)
    assert len(first) == len(second)
    scored = [r for r in second if r.get("event_type") == "candidate_signal.scored"][0]
    for key in [
        "asset",
        "window",
        "slot_start",
        "slot_end",
        "market_slug",
        "spot_delta_bps",
        "book_mid_delta_bps",
        "lag_gap_bps",
        "confidence",
        "raw_edge_bps",
        "rejected",
        "strategy_version",
    ]:
        assert key in scored["payload"]
