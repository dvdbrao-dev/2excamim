from __future__ import annotations

import json
from pathlib import Path

from agents import crypto_strategy_scorecard_agent as agent


def _write(path: Path, rows: list[dict]) -> None:
    path.write_text("\n".join(json.dumps(row) for row in rows) + "\n", encoding="utf-8")


def test_scorecard_aggregates_by_strategy(tmp_path: Path, capsys, monkeypatch) -> None:
    store = tmp_path / "events.jsonl"

    rows = [
        {
            "event_type": "crypto.signal.generated",
            "payload": {"strategy_id": "crypto_adx_ema_pullback_v1", "signal_id": "sig-1"},
            "linkage": {"signal_id": "sig-1"},
        },
        {
            "event_type": "crypto.signal.generated",
            "payload": {"strategy_id": "crypto_volatility_breakout_v1", "signal_id": "sig-2"},
            "linkage": {"signal_id": "sig-2"},
        },
        {
            "event_type": "decision.formed",
            "payload": {},
            "linkage": {"signal_id": "sig-1"},
        },
        {
            "event_type": "veto.raised",
            "payload": {},
            "linkage": {"signal_id": "sig-2"},
        },
        {
            "event_type": "fill.received",
            "payload": {"side": "SELL", "quantity": 10, "price": 2},
            "linkage": {"signal_id": "sig-1"},
        },
    ]
    _write(store, rows)
    monkeypatch.setattr("sys.argv", ["crypto_strategy_scorecard_agent.py", "--store", str(store), "--json"])

    rc = agent.main()
    assert rc == 0

    captured = capsys.readouterr().out
    payload = json.loads(captured)
    strategies = {item["strategy_id"]: item for item in payload["strategies"]}

    assert strategies["crypto_adx_ema_pullback_v1"]["signals_generated"] == 1
    assert strategies["crypto_adx_ema_pullback_v1"]["decisions_formed"] == 1
    assert strategies["crypto_adx_ema_pullback_v1"]["fills"] == 1
    assert strategies["crypto_volatility_breakout_v1"]["vetoes"] == 1


def test_scorecard_writes_runtime_file(tmp_path: Path, capsys, monkeypatch) -> None:
    store = tmp_path / "events.jsonl"
    runtime_file = Path("runtime/crypto_strategy_scorecard.json")
    if runtime_file.exists():
        runtime_file.unlink()

    rows = [
        {
            "event_type": "crypto.signal.generated",
            "payload": {"strategy_id": "crypto_adx_ema_pullback_v1", "signal_id": "sig-1"},
            "linkage": {"signal_id": "sig-1"},
        }
    ]
    _write(store, rows)
    monkeypatch.setattr("sys.argv", ["crypto_strategy_scorecard_agent.py", "--store", str(store), "--json"])

    rc = agent.main()
    assert rc == 0
    _ = capsys.readouterr().out

    assert runtime_file.exists()
    payload = json.loads(runtime_file.read_text(encoding="utf-8"))
    assert payload["actor"] == agent.AGENT_ID
    assert isinstance(payload["strategies"], list)
