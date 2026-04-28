from __future__ import annotations

import json
from pathlib import Path

from agents import crypto_strategy_incubator_agent as agent


def _write_json(path: Path, payload: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload) + "\n", encoding="utf-8")


def _write_events(path: Path, strategy_id: str, n: int) -> None:
    rows = []
    for i in range(n):
        rows.append(
            {
                "event_type": "crypto.signal.generated",
                "payload": {"strategy_id": strategy_id, "signal_id": f"sig-{i}"},
                "linkage": {"signal_id": f"sig-{i}"},
            }
        )
    path.write_text("\n".join(json.dumps(row) for row in rows) + "\n", encoding="utf-8")


def _registry(strategy_id: str = "s1") -> dict:
    return {
        "agent": "crypto_strategy_incubator",
        "mode": "paper_shadow_only",
        "timestamp": "2026-04-28T00:00:00Z",
        "strategies": [
            {
                "strategy_id": strategy_id,
                "agent_file": "agents/s1.py",
                "status": "candidate",
                "created_at": "2026-04-28T00:00:00Z",
                "last_evaluated_at": None,
                "promotion_count": 0,
                "freeze_count": 0,
                "rejection_reason": None,
                "notes": "test",
            }
        ],
    }


def _config(path: Path) -> None:
    path.write_text(
        "\n".join(
            [
                "min_signals_for_shadow: 20",
                "min_signals_for_promotion: 100",
                "min_expectancy_for_promotion: 0.0",
                "min_profit_factor_for_promotion: 1.2",
                "max_drawdown_allowed: 0.15",
                "min_confidence_for_promotion: 0.60",
                "freeze_after_negative_windows: 3",
                "reject_after_failed_runs: 5",
            ]
        )
        + "\n",
        encoding="utf-8",
    )


def test_candidate_or_shadow_with_few_signals(tmp_path: Path, monkeypatch, capsys) -> None:
    cfg = tmp_path / "cfg.yaml"
    reg = tmp_path / "registry.json"
    score = tmp_path / "score.json"
    store = tmp_path / "events.jsonl"

    _config(cfg)
    _write_json(reg, _registry())
    _write_json(score, {"strategies": [{"strategy_id": "s1", "signals_generated": 10}]})
    _write_events(store, "s1", 10)

    monkeypatch.setattr(
        "sys.argv",
        [
            "crypto_strategy_incubator_agent.py",
            "--json",
            "--config",
            str(cfg),
            "--registry",
            str(reg),
            "--scorecard",
            str(score),
            "--store",
            str(store),
        ],
    )

    assert agent.main() == 0
    out = json.loads(capsys.readouterr().out)
    assert out["evaluated_strategies"][0]["status"] in ("candidate", "shadow")


def test_promoted_with_good_metrics(tmp_path: Path, monkeypatch, capsys) -> None:
    cfg = tmp_path / "cfg.yaml"
    reg = tmp_path / "registry.json"
    score = tmp_path / "score.json"
    store = tmp_path / "events.jsonl"

    _config(cfg)
    _write_json(reg, _registry("s_promote"))
    _write_json(
        score,
        {
            "strategies": [
                {
                    "strategy_id": "s_promote",
                    "signals_generated": 120,
                    "expectancy": 0.02,
                    "profit_factor": 1.4,
                    "max_drawdown": 0.10,
                    "confidence": 0.8,
                    "negative_windows": 0,
                    "failed_runs": 0,
                    "winrate": 0.6,
                    "avg_return": 0.01,
                    "sharpe_like": 1.1,
                    "recent_performance": 0.03,
                    "symbol": "BTCUSDT",
                    "timeframe": "1h",
                }
            ]
        },
    )
    _write_events(store, "s_promote", 120)

    monkeypatch.setattr(
        "sys.argv",
        [
            "crypto_strategy_incubator_agent.py",
            "--json",
            "--config",
            str(cfg),
            "--registry",
            str(reg),
            "--scorecard",
            str(score),
            "--store",
            str(store),
        ],
    )

    assert agent.main() == 0
    out = json.loads(capsys.readouterr().out)
    assert "s_promote" in out["promoted"]
    assert out["evaluated_strategies"][0]["status"] == "promoted"


def test_frozen_with_excessive_drawdown(tmp_path: Path, monkeypatch, capsys) -> None:
    cfg = tmp_path / "cfg.yaml"
    reg = tmp_path / "registry.json"
    score = tmp_path / "score.json"
    store = tmp_path / "events.jsonl"

    _config(cfg)
    _write_json(reg, _registry("s_frozen"))
    _write_json(
        score,
        {
            "strategies": [
                {
                    "strategy_id": "s_frozen",
                    "signals_generated": 130,
                    "expectancy": 0.01,
                    "profit_factor": 1.5,
                    "max_drawdown": 0.30,
                    "confidence": 0.9,
                    "negative_windows": 0,
                    "failed_runs": 0,
                }
            ]
        },
    )
    _write_events(store, "s_frozen", 130)

    monkeypatch.setattr(
        "sys.argv",
        [
            "crypto_strategy_incubator_agent.py",
            "--json",
            "--config",
            str(cfg),
            "--registry",
            str(reg),
            "--scorecard",
            str(score),
            "--store",
            str(store),
        ],
    )

    assert agent.main() == 0
    out = json.loads(capsys.readouterr().out)
    assert "s_frozen" in out["frozen"]


def test_rejected_with_repeated_failures(tmp_path: Path, monkeypatch, capsys) -> None:
    cfg = tmp_path / "cfg.yaml"
    reg = tmp_path / "registry.json"
    score = tmp_path / "score.json"
    store = tmp_path / "events.jsonl"

    _config(cfg)
    _write_json(reg, _registry("s_reject"))
    _write_json(
        score,
        {
            "strategies": [
                {
                    "strategy_id": "s_reject",
                    "signals_generated": 200,
                    "expectancy": 0.05,
                    "profit_factor": 2.0,
                    "max_drawdown": 0.05,
                    "confidence": 0.9,
                    "negative_windows": 0,
                    "failed_runs": 7,
                }
            ]
        },
    )
    _write_events(store, "s_reject", 200)

    monkeypatch.setattr(
        "sys.argv",
        [
            "crypto_strategy_incubator_agent.py",
            "--json",
            "--config",
            str(cfg),
            "--registry",
            str(reg),
            "--scorecard",
            str(score),
            "--store",
            str(store),
        ],
    )

    assert agent.main() == 0
    out = json.loads(capsys.readouterr().out)
    assert "s_reject" in out["rejected"]


def test_warning_on_incomplete_data_without_breaking(tmp_path: Path, monkeypatch, capsys) -> None:
    cfg = tmp_path / "cfg.yaml"
    reg = tmp_path / "registry.json"
    score = tmp_path / "score.json"
    store = tmp_path / "events.jsonl"

    _config(cfg)
    _write_json(reg, _registry("s_warn"))
    _write_json(score, {"strategies": [{"strategy_id": "s_warn", "signals_generated": 40}]})
    _write_events(store, "s_warn", 40)

    monkeypatch.setattr(
        "sys.argv",
        [
            "crypto_strategy_incubator_agent.py",
            "--json",
            "--config",
            str(cfg),
            "--registry",
            str(reg),
            "--scorecard",
            str(score),
            "--store",
            str(store),
        ],
    )

    assert agent.main() == 0
    out = json.loads(capsys.readouterr().out)
    assert any(item.startswith("incomplete_metrics:s_warn") for item in out["warnings"])


def test_json_stable_shape(tmp_path: Path, monkeypatch, capsys) -> None:
    cfg = tmp_path / "cfg.yaml"
    reg = tmp_path / "registry.json"
    score = tmp_path / "score.json"
    store = tmp_path / "events.jsonl"

    _config(cfg)
    _write_json(reg, _registry("s_shape"))
    _write_json(score, {"strategies": [{"strategy_id": "s_shape", "signals_generated": 1}]})
    _write_events(store, "s_shape", 1)

    monkeypatch.setattr(
        "sys.argv",
        [
            "crypto_strategy_incubator_agent.py",
            "--json",
            "--config",
            str(cfg),
            "--registry",
            str(reg),
            "--scorecard",
            str(score),
            "--store",
            str(store),
        ],
    )

    assert agent.main() == 0
    out = json.loads(capsys.readouterr().out)
    assert set(out.keys()) == {
        "agent",
        "mode",
        "timestamp",
        "evaluated_strategies",
        "promoted",
        "frozen",
        "rejected",
        "warnings",
        "summary",
    }
    assert set(out["summary"].keys()) == {"total", "promoted", "frozen", "rejected"}
