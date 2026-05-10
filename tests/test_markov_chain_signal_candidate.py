from __future__ import annotations

import json
from argparse import Namespace
from pathlib import Path

from agents.core.event_store import load_jsonl
from agents.markov_chain_signal_candidate import beta_smoothed_probability, run


def _event(ts: str, slot_start: str, slot_end: str, slug: str, spot: float, up_bid: float, up_ask: float, depth_bid: float = 900.0, depth_ask: float = 900.0, imb: float = 0.2, spread_bps: float = 100.0) -> dict:
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
            "market_slug": slug,
            "spot_price": spot,
            "orderbook": {
                "best_bid": up_bid,
                "best_ask": up_ask,
                "spread_bps": spread_bps,
                "depth_top_n": {"n": 2, "bid": depth_bid, "ask": depth_ask},
                "imbalance_top_n": imb,
                "books": [
                    {"outcome": "YES", "best_bid": up_bid, "best_ask": up_ask},
                    {"outcome": "NO", "best_bid": 1.0 - up_ask, "best_ask": 1.0 - up_bid},
                ],
            },
        },
    }


def _write(path: Path, rows: list[dict]) -> None:
    path.write_text("\n".join(json.dumps(r) for r in rows) + "\n", encoding="utf-8")


def _args(input_path: Path, output_path: Path, **overrides: object) -> Namespace:
    base = {
        "input_jsonl": str(input_path),
        "output_jsonl": str(output_path),
        "asset": "BTC",
        "window": "5m",
        "strategy_version": "markov_chain_v1",
        "lookback_slots": 2016,
        "min_state_samples": 5,
        "min_global_samples": 20,
        "alpha": 5.0,
        "beta": 5.0,
        "min_net_edge": 0.02,
        "cost_buffer": 0.015,
        "max_spread_bps": 300.0,
        "min_depth_usdc": 500.0,
        "min_seconds_to_expiry": 45,
        "dry_run": False,
    }
    base.update(overrides)
    return Namespace(**base)


def _build_dataset(n_slots: int = 60, favor: str = "UP") -> list[dict]:
    rows: list[dict] = []
    for i in range(n_slots):
        minute = i * 5
        hour = minute // 60
        mm = minute % 60
        slot_start = f"2026-05-07T{hour:02d}:{mm:02d}:00Z"
        slot_end = f"2026-05-07T{hour:02d}:{(mm + 4) % 60:02d}:59Z"
        slug = f"btc-up-or-down-may-07-{hour:02d}{mm:02d}-utc-5m"

        open_spot = 100.0 + (i * 0.1)
        if favor == "UP":
            close_spot = open_spot * (1.002 if i % 5 != 0 else 0.999)
            up_ask = 0.44
            up_bid = 0.43
        else:
            close_spot = open_spot * (0.998 if i % 5 != 0 else 1.001)
            up_ask = 0.56
            up_bid = 0.55

        rows.append(_event(f"2026-05-07T{hour:02d}:{mm:02d}:10Z", slot_start, slot_end, slug, open_spot, up_bid, up_ask))
        rows.append(_event(f"2026-05-07T{hour:02d}:{mm:02d}:40Z", slot_start, slot_end, slug, close_spot, up_bid, up_ask))
    return rows


def test_markov_probability_beta_smoothing() -> None:
    p = beta_smoothed_probability(8, 10, 5.0, 5.0)
    assert round(p, 6) == 0.65


def test_backoff_when_full_state_insufficient(tmp_path: Path) -> None:
    input_path = tmp_path / "in.jsonl"
    output_path = tmp_path / "out.jsonl"
    _write(input_path, _build_dataset(40, favor="UP"))

    run(_args(input_path, output_path, min_state_samples=100, min_global_samples=10))
    rows = load_jsonl(output_path)
    scored = [r for r in rows if r.get("event_type") == "candidate_signal.scored"][-1]["payload"]
    assert scored["rejected"] is True
    assert scored["reject_reason"] == "insufficient_state_samples"
    assert scored["model"]["backoff_level"] in {"previous_outcome", "prev_momentum", "prev_momentum_vol", "full_state"}


def test_up_signal_generated_when_edge_positive(tmp_path: Path) -> None:
    input_path = tmp_path / "in.jsonl"
    output_path = tmp_path / "out.jsonl"
    _write(input_path, _build_dataset(70, favor="UP"))

    run(_args(input_path, output_path))
    payload = [r for r in load_jsonl(output_path) if r.get("event_type") == "candidate_signal.scored"][-1]["payload"]
    assert payload["rejected"] is False
    assert payload["side"] == "UP"


def test_down_signal_generated_when_edge_positive(tmp_path: Path) -> None:
    input_path = tmp_path / "in.jsonl"
    output_path = tmp_path / "out.jsonl"
    _write(input_path, _build_dataset(70, favor="DOWN"))

    run(_args(input_path, output_path))
    payload = [r for r in load_jsonl(output_path) if r.get("event_type") == "candidate_signal.scored"][-1]["payload"]
    assert payload["rejected"] is False
    assert payload["side"] == "DOWN"


def test_signal_rejected_when_spread_too_wide(tmp_path: Path) -> None:
    rows = _build_dataset(50, favor="UP")
    rows[-1]["payload"]["orderbook"]["spread_bps"] = 900.0
    input_path = tmp_path / "in.jsonl"
    output_path = tmp_path / "out.jsonl"
    _write(input_path, rows)

    run(_args(input_path, output_path))
    payload = [r for r in load_jsonl(output_path) if r.get("event_type") == "candidate_signal.scored"][-1]["payload"]
    assert payload["rejected"] is True
    assert payload["reject_reason"] == "spread_too_wide"


def test_signal_rejected_when_orderbook_missing(tmp_path: Path) -> None:
    rows = _build_dataset(50, favor="UP")
    rows[-1]["payload"]["orderbook"] = {}
    input_path = tmp_path / "in.jsonl"
    output_path = tmp_path / "out.jsonl"
    _write(input_path, rows)

    run(_args(input_path, output_path))
    payload = [r for r in load_jsonl(output_path) if r.get("event_type") == "candidate_signal.scored"][-1]["payload"]
    assert payload["rejected"] is True
    assert payload["reject_reason"] == "orderbook_missing"


def test_event_envelope_valid_and_fields_present(tmp_path: Path) -> None:
    input_path = tmp_path / "in.jsonl"
    output_path = tmp_path / "out.jsonl"
    _write(input_path, _build_dataset(70, favor="UP"))

    run(_args(input_path, output_path))
    event = [r for r in load_jsonl(output_path) if r.get("event_type") == "candidate_signal.scored"][-1]
    assert isinstance(event.get("event_id"), str)
    assert isinstance(event.get("idempotency_key"), str)
    for key in ["asset", "window", "slot_start", "slot_end", "market_slug", "confidence", "raw_edge_bps", "rejected", "strategy_version"]:
        assert key in event["payload"]


def test_dry_run_does_not_persist(tmp_path: Path) -> None:
    input_path = tmp_path / "in.jsonl"
    output_path = tmp_path / "out.jsonl"
    _write(input_path, _build_dataset(70, favor="UP"))

    summary = run(_args(input_path, output_path, dry_run=True))
    assert summary["events_generated"] == 1
    assert summary["events_persisted"] == 0
    assert not output_path.exists()


def test_non_dry_run_persists_and_reports_one(tmp_path: Path) -> None:
    input_path = tmp_path / "in.jsonl"
    output_path = tmp_path / "out.jsonl"
    _write(input_path, _build_dataset(70, favor="UP"))

    summary = run(_args(input_path, output_path, dry_run=False))
    assert summary["events_generated"] == 1
    assert summary["events_persisted"] == 1
    assert output_path.exists()
    persisted = [r for r in load_jsonl(output_path) if r.get("event_type") == "candidate_signal.scored"]
    assert len(persisted) == 1
