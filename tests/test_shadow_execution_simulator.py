from __future__ import annotations

import json
from argparse import Namespace
from pathlib import Path

from agents.core.event_store import load_jsonl
from agents.shadow_execution_simulator import (
    apply_slippage,
    compute_net_pnl,
    polymarket_taker_fee_usdc,
    run,
)


def _signal_event(event_id: str, rejected: bool = False) -> dict:
    return {
        "event_type": "candidate_signal.scored",
        "event_id": event_id,
        "timestamp": "2026-05-07T18:05:00Z",
        "idempotency_key": f"candidate:{event_id}",
        "aggregate_key": "polymarket:btc:5m",
        "provenance": {"agent_id": "test", "source": "test", "generated_at": "2026-05-07T18:05:00Z"},
        "payload": {
            "strategy_version": "oracle_lag_v1",
            "asset": "BTC",
            "window": "5m",
            "side": "UP",
            "best_bid": 0.47,
            "best_ask": 0.49,
            "spread_bps": 120.0,
            "orderbook": {"depth_top_n": {"n": 2, "bid": 1200.0, "ask": 1100.0}},
            "rejected": rejected,
        },
    }


def _write(path: Path, rows: list[dict]) -> None:
    path.write_text("\n".join(json.dumps(r) for r in rows) + "\n", encoding="utf-8")


def test_fee_function() -> None:
    fee = polymarket_taker_fee_usdc(size=100.0, price=0.5, fee_rate_bps=25.0)
    assert round(fee, 6) == 0.0625


def test_slippage_function() -> None:
    slipped = apply_slippage(price=0.50, side="UP", slippage_bps=20.0)
    assert round(slipped, 6) == 0.501


def test_rejected_signal_not_filled(tmp_path: Path) -> None:
    inp = tmp_path / "in.jsonl"
    out = tmp_path / "out.jsonl"
    _write(inp, [_signal_event("sig-rej", rejected=True)])
    args = Namespace(
        input_jsonl=str(inp),
        output_jsonl=str(out),
        strategy_version="oracle_lag_v1",
        fee_rate_bps=25.0,
        slippage_bps=20.0,
        latency_ms=800,
        max_notional_usdc=25.0,
        mock_resolution="",
        dry_run=False,
    )
    run(args)
    rows = load_jsonl(out)
    shadow = [r for r in rows if r.get("event_type") == "shadow_fill.simulated"][0]
    assert shadow["payload"]["rejected"] is True


def test_valid_signal_produces_shadow_fill(tmp_path: Path) -> None:
    inp = tmp_path / "in.jsonl"
    out = tmp_path / "out.jsonl"
    _write(inp, [_signal_event("sig-ok", rejected=False)])
    args = Namespace(
        input_jsonl=str(inp),
        output_jsonl=str(out),
        strategy_version="oracle_lag_v1",
        fee_rate_bps=25.0,
        slippage_bps=20.0,
        latency_ms=100,
        max_notional_usdc=25.0,
        mock_resolution="UP",
        dry_run=False,
    )
    run(args)
    rows = load_jsonl(out)
    shadow = [r for r in rows if r.get("event_type") == "shadow_fill.simulated"][0]
    assert shadow["payload"]["rejected"] is False
    assert shadow["payload"]["simulated_fill_price"] is not None


def test_net_pnl_calculation() -> None:
    net = compute_net_pnl("UP", fill_price=0.5, size=10.0, fees=0.1, slippage_usdc=0.05)
    assert round(net, 4) == 4.85


def test_event_schema_valid(tmp_path: Path) -> None:
    inp = tmp_path / "in.jsonl"
    out = tmp_path / "out.jsonl"
    _write(inp, [_signal_event("sig-schema", rejected=False)])
    args = Namespace(
        input_jsonl=str(inp),
        output_jsonl=str(out),
        strategy_version="oracle_lag_v1",
        fee_rate_bps=25.0,
        slippage_bps=20.0,
        latency_ms=100,
        max_notional_usdc=25.0,
        mock_resolution="UP",
        dry_run=False,
    )
    run(args)
    rows = load_jsonl(out)
    shadow = [r for r in rows if r.get("event_type") == "shadow_fill.simulated"][0]
    round_event = [r for r in rows if r.get("event_type") == "strategy_round.scored"][0]
    for key in ["strategy_version", "signal_event_id", "asset", "window", "fill_probability_estimate", "rejected"]:
        assert key in shadow["payload"]
    for key in ["strategy_version", "signal_event_id", "fill_event_id", "outcome_known", "notes"]:
        assert key in round_event["payload"]
