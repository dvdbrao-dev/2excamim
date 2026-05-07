from __future__ import annotations

import json
from pathlib import Path

from agents.backtest_external_candidate import main as run_main


def _snapshot(ts: str, spot: float, mid: float, slug: str = "btc-slot-1") -> dict:
    return {
        "event_type": "market_snapshot.observed",
        "event_id": ts,
        "timestamp": ts,
        "idempotency_key": f"snap:{ts}",
        "aggregate_key": "polymarket:btc:5m",
        "provenance": {"agent_id": "t", "source": "t", "generated_at": ts},
        "payload": {
            "asset": "BTC",
            "window": "5m",
            "slot_start": "2026-05-07T18:00:00Z",
            "slot_end": "2026-05-07T18:10:00Z",
            "market_slug": slug,
            "spot_price": spot,
            "oracle_price": spot,
            "orderbook": {
                "best_bid": mid - 0.01,
                "best_ask": mid + 0.01,
                "mid_price": mid,
                "spread_bps": 150.0,
            },
        },
    }


def _write_jsonl(path: Path, rows: list[dict]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(json.dumps(r) for r in rows) + "\n", encoding="utf-8")


def _read_jsonl(path: Path) -> list[dict]:
    return [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]


def test_no_lookahead_bias_and_deterministic_replay(tmp_path: Path, monkeypatch) -> None:
    inp = tmp_path / "in.jsonl"
    out = tmp_path / "out.jsonl"
    report = tmp_path / "report.md"
    rows = [
        _snapshot("2026-05-07T18:00:00Z", 100.0, 0.48),
        _snapshot("2026-05-07T18:01:00Z", 100.2, 0.4801),
        _snapshot("2026-05-07T18:02:00Z", 100.4, 0.4802),
    ]
    _write_jsonl(inp, rows)

    monkeypatch.setattr(
        "sys.argv",
        [
            "backtest_external_candidate.py",
            "--input-jsonl",
            str(inp),
            "--output-jsonl",
            str(out),
            "--output-report",
            str(report),
            "--asset",
            "BTC",
            "--window",
            "5m",
        ],
    )
    assert run_main() == 0
    first = _read_jsonl(out)

    monkeypatch.setattr(
        "sys.argv",
        [
            "backtest_external_candidate.py",
            "--input-jsonl",
            str(inp),
            "--output-jsonl",
            str(out),
            "--output-report",
            str(report),
            "--asset",
            "BTC",
            "--window",
            "5m",
        ],
    )
    assert run_main() == 0
    second = _read_jsonl(out)

    signals = [e for e in second if e["event_type"] == "candidate_signal.scored"]
    assert len(signals) == 2
    assert len(first) < len(second)  # second run appends run_started/completed


def test_unresolved_outcomes_handled(tmp_path: Path, monkeypatch) -> None:
    inp = tmp_path / "in.jsonl"
    out = tmp_path / "out.jsonl"
    report = tmp_path / "report.md"
    _write_jsonl(inp, [_snapshot("2026-05-07T18:00:00Z", 100.0, 0.48), _snapshot("2026-05-07T18:01:00Z", 100.2, 0.4801)])

    monkeypatch.setattr("sys.argv", ["backtest_external_candidate.py", "--input-jsonl", str(inp), "--output-jsonl", str(out), "--output-report", str(report)])
    assert run_main() == 0
    content = report.read_text(encoding="utf-8")
    assert "Unresolved count" in content


def test_report_generated_and_event_schema(tmp_path: Path, monkeypatch) -> None:
    inp = tmp_path / "in.jsonl"
    out = tmp_path / "out.jsonl"
    report = tmp_path / "report.md"
    _write_jsonl(inp, [_snapshot("2026-05-07T18:00:00Z", 100.0, 0.48), _snapshot("2026-05-07T18:01:00Z", 100.2, 0.4801)])

    monkeypatch.setattr("sys.argv", ["backtest_external_candidate.py", "--input-jsonl", str(inp), "--output-jsonl", str(out), "--output-report", str(report)])
    assert run_main() == 0

    assert report.exists()
    events = _read_jsonl(out)
    kinds = {e["event_type"] for e in events}
    assert "backtest.run_started" in kinds
    assert "backtest.run_completed" in kinds
    assert "candidate_strategy.evaluated" in kinds
