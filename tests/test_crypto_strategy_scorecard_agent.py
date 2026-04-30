from __future__ import annotations

import json
from pathlib import Path

from agents import crypto_strategy_scorecard_agent as agent


def _write(path: Path, rows: list[dict]) -> None:
    path.write_text("\n".join(json.dumps(row) for row in rows) + "\n", encoding="utf-8")


def _base_events() -> list[dict]:
    return [
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


def test_scorecard_aggregates_by_strategy(tmp_path: Path, capsys, monkeypatch) -> None:
    store = tmp_path / "events.jsonl"
    scorecard_out = tmp_path / "scorecard.json"
    _write(store, _base_events())
    monkeypatch.setattr(
        "sys.argv",
        [
            "crypto_strategy_scorecard_agent.py",
            "--store", str(store),
            "--scorecard-output", str(scorecard_out),
            "--json",
        ],
    )

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
    scorecard_out = tmp_path / "scorecard.json"
    _write(
        store,
        [
            {
                "event_type": "crypto.signal.generated",
                "payload": {"strategy_id": "crypto_adx_ema_pullback_v1", "signal_id": "sig-1"},
                "linkage": {"signal_id": "sig-1"},
            }
        ],
    )
    monkeypatch.setattr(
        "sys.argv",
        [
            "crypto_strategy_scorecard_agent.py",
            "--store", str(store),
            "--scorecard-output", str(scorecard_out),
            "--json",
        ],
    )

    rc = agent.main()
    assert rc == 0
    _ = capsys.readouterr().out

    assert scorecard_out.exists()
    payload = json.loads(scorecard_out.read_text(encoding="utf-8"))
    assert payload["actor"] == agent.AGENT_ID
    assert isinstance(payload["strategies"], list)


def test_scorecard_emits_all_incubator_keys(tmp_path: Path, capsys, monkeypatch) -> None:
    """Every row must carry the keys consumed by crypto_strategy_incubator_agent."""
    store = tmp_path / "events.jsonl"
    scorecard_out = tmp_path / "scorecard.json"
    _write(store, _base_events())
    monkeypatch.setattr(
        "sys.argv",
        [
            "crypto_strategy_scorecard_agent.py",
            "--store", str(store),
            "--scorecard-output", str(scorecard_out),
            "--json",
        ],
    )

    rc = agent.main()
    assert rc == 0

    payload = json.loads(capsys.readouterr().out)
    required_keys = {
        "strategy_id", "signals_generated", "expectancy", "profit_factor",
        "max_drawdown", "confidence", "negative_windows", "failed_runs",
        "winrate", "avg_return", "sharpe_like", "recent_performance",
        "symbol", "timeframe", "trades", "wins", "losses",
        "pnl_gross", "costs", "pnl_net", "status", "warnings",
    }
    for row in payload["strategies"]:
        missing = required_keys - set(row.keys())
        assert not missing, f"Missing keys in row for {row.get('strategy_id')}: {missing}"


def test_scorecard_real_pnl_buy_then_sell(tmp_path: Path, capsys, monkeypatch) -> None:
    """Buy 10 @ 0.20 then Sell 10 @ 0.40 -> pnl_gross ~ +2.0."""
    store = tmp_path / "events.jsonl"
    scorecard_out = tmp_path / "scorecard.json"
    events = [
        {
            "event_type": "crypto.signal.generated",
            "payload": {"strategy_id": "strat_x", "signal_id": "s1"},
            "linkage": {"signal_id": "s1"},
        },
        {
            "event_type": "fill.received",
            "payload": {"side": "BUY", "quantity": 10, "price": 0.20},
            "linkage": {"signal_id": "s1"},
        },
        {
            "event_type": "fill.received",
            "payload": {"side": "SELL", "quantity": 10, "price": 0.40},
            "linkage": {"signal_id": "s1"},
        },
    ]
    _write(store, events)
    monkeypatch.setattr(
        "sys.argv",
        [
            "crypto_strategy_scorecard_agent.py",
            "--store", str(store),
            "--scorecard-output", str(scorecard_out),
            "--fee-bps", "0",
            "--slippage-bps", "0",
            "--json",
        ],
    )

    rc = agent.main()
    assert rc == 0

    payload = json.loads(capsys.readouterr().out)
    row = {item["strategy_id"]: item for item in payload["strategies"]}["strat_x"]
    assert abs(row["pnl_gross"] - 2.0) < 1e-6
    assert row["trades"] == 1
    assert row["wins"] == 1
    assert row["losses"] == 0


def test_scorecard_fees_reduce_pnl_net(tmp_path: Path, capsys, monkeypatch) -> None:
    store = tmp_path / "events.jsonl"
    scorecard_out = tmp_path / "scorecard.json"
    events = [
        {
            "event_type": "crypto.signal.generated",
            "payload": {"strategy_id": "strat_y", "signal_id": "s2"},
            "linkage": {"signal_id": "s2"},
        },
        {
            "event_type": "fill.received",
            "payload": {"side": "BUY", "quantity": 10, "price": 0.20},
            "linkage": {"signal_id": "s2"},
        },
        {
            "event_type": "fill.received",
            "payload": {"side": "SELL", "quantity": 10, "price": 0.40},
            "linkage": {"signal_id": "s2"},
        },
    ]
    _write(store, events)

    def run(fee: float) -> dict:
        monkeypatch.setattr(
            "sys.argv",
            [
                "crypto_strategy_scorecard_agent.py",
                "--store", str(store),
                "--scorecard-output", str(scorecard_out),
                "--fee-bps", str(fee),
                "--slippage-bps", "0",
                "--json",
            ],
        )
        agent.main()
        payload = json.loads(capsys.readouterr().out)
        return {item["strategy_id"]: item for item in payload["strategies"]}["strat_y"]

    no_fee = run(0.0)
    with_fee = run(15.0)
    assert with_fee["pnl_net"] < no_fee["pnl_net"]
    assert with_fee["costs"] > 0.0


def test_scorecard_no_fake_notional_formula(tmp_path: Path, capsys, monkeypatch) -> None:
    """Verify pnl_gross comes from price difference, not 0.001 * notional."""
    store = tmp_path / "events.jsonl"
    scorecard_out = tmp_path / "scorecard.json"
    events = [
        {
            "event_type": "crypto.signal.generated",
            "payload": {"strategy_id": "strat_z", "signal_id": "s3"},
            "linkage": {"signal_id": "s3"},
        },
        {
            "event_type": "fill.received",
            "payload": {"side": "BUY", "quantity": 5, "price": 100.0},
            "linkage": {"signal_id": "s3"},
        },
        {
            "event_type": "fill.received",
            "payload": {"side": "SELL", "quantity": 5, "price": 80.0},
            "linkage": {"signal_id": "s3"},
        },
    ]
    _write(store, events)
    monkeypatch.setattr(
        "sys.argv",
        [
            "crypto_strategy_scorecard_agent.py",
            "--store", str(store),
            "--scorecard-output", str(scorecard_out),
            "--fee-bps", "0",
            "--slippage-bps", "0",
            "--json",
        ],
    )
    agent.main()
    payload = json.loads(capsys.readouterr().out)
    row = {item["strategy_id"]: item for item in payload["strategies"]}["strat_z"]

    fake_pnl = 5 * 80.0 * 0.001
    real_pnl = 5 * (80.0 - 100.0)
    assert abs(row["pnl_gross"] - real_pnl) < 1e-6
    assert abs(row["pnl_gross"] - fake_pnl) > 1.0


def test_scorecard_deterministic(tmp_path: Path, capsys, monkeypatch) -> None:
    store = tmp_path / "events.jsonl"
    scorecard_out = tmp_path / "scorecard.json"
    _write(store, _base_events())

    def run() -> str:
        monkeypatch.setattr(
            "sys.argv",
            [
                "crypto_strategy_scorecard_agent.py",
                "--store", str(store),
                "--scorecard-output", str(scorecard_out),
                "--json",
            ],
        )
        agent.main()
        return capsys.readouterr().out

    assert run() == run()


def test_scorecard_kill_status_all_vetoed(tmp_path: Path, capsys, monkeypatch) -> None:
    store = tmp_path / "events.jsonl"
    scorecard_out = tmp_path / "scorecard.json"
    events = [
        {
            "event_type": "crypto.signal.generated",
            "payload": {"strategy_id": "strat_kill", "signal_id": "k1"},
            "linkage": {"signal_id": "k1"},
        },
        {
            "event_type": "veto.raised",
            "payload": {},
            "linkage": {"signal_id": "k1"},
        },
    ]
    _write(store, events)
    monkeypatch.setattr(
        "sys.argv",
        [
            "crypto_strategy_scorecard_agent.py",
            "--store", str(store),
            "--scorecard-output", str(scorecard_out),
            "--json",
        ],
    )
    agent.main()
    payload = json.loads(capsys.readouterr().out)
    row = {item["strategy_id"]: item for item in payload["strategies"]}["strat_kill"]
    assert row["status"] == "KILL"
