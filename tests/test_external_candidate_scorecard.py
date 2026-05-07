from __future__ import annotations

import json
from argparse import Namespace
from pathlib import Path

from agents.core.event_store import load_jsonl
from agents.external_candidate_scorecard import run


def _signal(i: int, rejected: bool = False) -> dict:
    return {
        "event_type": "candidate_signal.scored",
        "event_id": f"sig-{i}",
        "timestamp": "2026-05-07T18:00:00Z",
        "idempotency_key": f"sig:{i}",
        "aggregate_key": "strategy:oracle_lag_v1",
        "provenance": {"agent_id": "t", "source": "t", "generated_at": "2026-05-07T18:00:00Z"},
        "payload": {"strategy_version": "oracle_lag_v1", "rejected": rejected},
    }


def _fill(i: int, fee: float = 0.1, slip: float = 0.1, lat: int = 100) -> dict:
    return {
        "event_type": "shadow_fill.simulated",
        "event_id": f"fill-{i}",
        "timestamp": "2026-05-07T18:00:00Z",
        "idempotency_key": f"fill:{i}",
        "aggregate_key": "strategy:oracle_lag_v1",
        "provenance": {"agent_id": "t", "source": "t", "generated_at": "2026-05-07T18:00:00Z"},
        "payload": {
            "strategy_version": "oracle_lag_v1",
            "signal_event_id": f"sig-{i}",
            "notional_usdc": 25.0,
            "fee_usdc": fee,
            "slippage_usdc": slip,
            "latency_ms": lat,
        },
    }


def _round(i: int, net: float, gross: float | None = None, known: bool = True) -> dict:
    return {
        "event_type": "strategy_round.scored",
        "event_id": f"round-{i}",
        "timestamp": "2026-05-07T18:00:00Z",
        "idempotency_key": f"round:{i}",
        "aggregate_key": "strategy:oracle_lag_v1",
        "provenance": {"agent_id": "t", "source": "t", "generated_at": "2026-05-07T18:00:00Z"},
        "payload": {
            "strategy_version": "oracle_lag_v1",
            "signal_event_id": f"sig-{i}",
            "outcome_known": known,
            "gross_pnl_usdc": net if gross is None else gross,
            "net_pnl_usdc": net,
        },
    }


def _gap() -> dict:
    return {
        "event_type": "data_gap.detected",
        "event_id": "gap-1",
        "timestamp": "2026-05-07T18:00:00Z",
        "idempotency_key": "gap:1",
        "aggregate_key": "feed:research-collector",
        "provenance": {"agent_id": "t", "source": "t", "generated_at": "2026-05-07T18:00:00Z"},
        "payload": {"gap_type": "x"},
    }


def _write(path: Path, rows: list[dict]) -> None:
    path.write_text("\n".join(json.dumps(r) for r in rows) + "\n", encoding="utf-8")


def _args(inp: Path, out: Path, min_signals: int = 200, min_resolved: int = 100) -> Namespace:
    return Namespace(
        input_jsonl=str(inp),
        output_jsonl=str(out),
        strategy_version="oracle_lag_v1",
        min_signals=min_signals,
        min_resolved_rounds=min_resolved,
        max_drawdown_usdc=50.0,
        min_net_expectancy_bps=3.0,
        dry_run=False,
    )


def _latest_eval(out: Path) -> dict:
    rows = load_jsonl(out)
    ev = [r for r in rows if r.get("event_type") == "candidate_strategy.evaluated"]
    return ev[-1]


def test_insufficient_sample_stays_candidate(tmp_path: Path) -> None:
    inp, out = tmp_path / "in.jsonl", tmp_path / "out.jsonl"
    rows = [_signal(1), _fill(1), _round(1, 0.5)]
    _write(inp, rows)
    run(_args(inp, out, min_signals=10, min_resolved=10))
    payload = _latest_eval(out)["payload"]
    assert payload["status"] == "candidate"


def test_negative_expectancy_rejected(tmp_path: Path) -> None:
    inp, out = tmp_path / "in.jsonl", tmp_path / "out.jsonl"
    rows: list[dict] = []
    for i in range(1, 6):
        rows += [_signal(i), _fill(i), _round(i, -1.0)]
    _write(inp, rows)
    run(_args(inp, out, min_signals=5, min_resolved=5))
    payload = _latest_eval(out)["payload"]
    assert payload["status"] == "rejected"


def test_data_quality_issues_frozen(tmp_path: Path) -> None:
    inp, out = tmp_path / "in.jsonl", tmp_path / "out.jsonl"
    rows: list[dict] = []
    for i in range(1, 6):
        rows += [_signal(i), _fill(i), _round(i, 0.5)]
    rows += [_gap() for _ in range(10)]
    _write(inp, rows)
    run(_args(inp, out, min_signals=5, min_resolved=5))
    payload = _latest_eval(out)["payload"]
    assert payload["status"] == "frozen"


def test_strong_result_suggests_promoted(tmp_path: Path) -> None:
    inp, out = tmp_path / "in.jsonl", tmp_path / "out.jsonl"
    rows: list[dict] = []
    for i in range(1, 6):
        rows += [_signal(i), _fill(i, fee=0.05, slip=0.02), _round(i, 1.0)]
    _write(inp, rows)
    run(_args(inp, out, min_signals=5, min_resolved=5))
    payload = _latest_eval(out)["payload"]
    assert payload["status"] == "candidate"
    assert payload["suggested_status"] == "promoted"
    assert payload["auto_promoted"] is False


def test_event_schema_valid(tmp_path: Path) -> None:
    inp, out = tmp_path / "in.jsonl", tmp_path / "out.jsonl"
    _write(inp, [_signal(1), _fill(1), _round(1, 1.0)])
    run(_args(inp, out, min_signals=1, min_resolved=1))
    payload = _latest_eval(out)["payload"]
    for key in [
        "signal_count",
        "rejection_rate",
        "shadow_fill_rate",
        "resolved_round_count",
        "net_pnl_usdc",
        "avg_net_edge_bps",
        "max_drawdown_usdc",
        "status",
        "suggested_status",
        "thresholds",
    ]:
        assert key in payload
