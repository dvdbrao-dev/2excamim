from __future__ import annotations

import json
from pathlib import Path

from agents import crypto_strategy_scorecard_agent as agent

# Keys the incubator reads from each scorecard row (sourced from
# crypto_strategy_incubator_agent.py evaluate_status + evaluated_strategies).
INCUBATOR_REQUIRED_KEYS = frozenset(
    {
        "strategy_id",
        "signals_generated",
        "expectancy",
        "profit_factor",
        "max_drawdown",
        "confidence",
        "negative_windows",
        "failed_runs",
        "winrate",
        "avg_return",
        "sharpe_like",
        "recent_performance",
        "symbol",
        "timeframe",
        "trades",
        "wins",
        "losses",
        "pnl_gross",
        "costs",
        "pnl_net",
        "status",
        "warnings",
    }
)


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _write(path: Path, rows: list[dict]) -> None:
    path.write_text("\n".join(json.dumps(row) for row in rows) + "\n", encoding="utf-8")


def _run(monkeypatch, capsys, store: Path, scorecard_out: Path, extra_args: list[str] | None = None) -> dict:
    argv = [
        "crypto_strategy_scorecard_agent.py",
        "--store", str(store),
        "--scorecard-output", str(scorecard_out),
        "--json",
    ]
    if extra_args:
        argv.extend(extra_args)
    monkeypatch.setattr("sys.argv", argv)
    rc = agent.main()
    assert rc == 0
    return json.loads(capsys.readouterr().out)


def _strategy(payload: dict, strategy_id: str) -> dict:
    return {item["strategy_id"]: item for item in payload["strategies"]}[strategy_id]


def _fill_events(strategy_id: str, signal_id: str, pairs: list[tuple[float, float]]) -> list[dict]:
    """Return one signal event + alternating BUY/SELL fill pairs."""
    events: list[dict] = [
        {
            "event_type": "crypto.signal.generated",
            "payload": {"strategy_id": strategy_id, "signal_id": signal_id},
            "linkage": {"signal_id": signal_id},
        }
    ]
    for buy_px, sell_px in pairs:
        events.append(
            {
                "event_type": "fill.received",
                "payload": {"side": "BUY", "quantity": 1.0, "price": buy_px},
                "linkage": {"signal_id": signal_id},
            }
        )
        events.append(
            {
                "event_type": "fill.received",
                "payload": {"side": "SELL", "quantity": 1.0, "price": sell_px},
                "linkage": {"signal_id": signal_id},
            }
        )
    return events


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


# ---------------------------------------------------------------------------
# 1. PnL sign correctness
# ---------------------------------------------------------------------------

def test_pnl_signs_match_directional_outcome(tmp_path: Path, capsys, monkeypatch) -> None:
    """Buy 10 @ 0.20 then Sell 10 @ 0.40 must yield pnl_gross ≈ +2.0 with wins=1, losses=0."""
    store = tmp_path / "events.jsonl"
    scorecard_out = tmp_path / "sc.json"
    events = [
        {
            "event_type": "crypto.signal.generated",
            "payload": {"strategy_id": "strat_dir", "signal_id": "s-dir"},
            "linkage": {"signal_id": "s-dir"},
        },
        {
            "event_type": "fill.received",
            "payload": {"side": "BUY", "quantity": 10, "price": 0.20},
            "linkage": {"signal_id": "s-dir"},
        },
        {
            "event_type": "fill.received",
            "payload": {"side": "SELL", "quantity": 10, "price": 0.40},
            "linkage": {"signal_id": "s-dir"},
        },
    ]
    _write(store, events)
    payload = _run(monkeypatch, capsys, store, scorecard_out, ["--fee-bps", "0", "--slippage-bps", "0"])
    row = _strategy(payload, "strat_dir")

    assert abs(row["pnl_gross"] - 2.0) < 1e-6, f"expected pnl_gross≈2.0, got {row['pnl_gross']}"
    assert row["pnl_net"] == row["pnl_gross"], "pnl_net must equal pnl_gross when fees=0"
    assert row["trades"] == 1
    assert row["wins"] == 1
    assert row["losses"] == 0


# ---------------------------------------------------------------------------
# 2. Fees and slippage reduce pnl_net
# ---------------------------------------------------------------------------

def test_pnl_with_fees_and_slippage(tmp_path: Path, capsys, monkeypatch) -> None:
    """fee_bps and slippage_bps independently reduce pnl_net and appear in costs."""
    store = tmp_path / "events.jsonl"
    scorecard_out = tmp_path / "sc.json"
    events = _fill_events("strat_fee", "s-fee", [(0.20, 0.40)])
    _write(store, events)

    def run(fee: float, slip: float) -> dict:
        return _strategy(
            _run(monkeypatch, capsys, store, scorecard_out, [f"--fee-bps={fee}", f"--slippage-bps={slip}"]),
            "strat_fee",
        )

    no_cost   = run(0.0, 0.0)
    only_fee  = run(15.0, 0.0)
    only_slip = run(0.0, 10.0)
    both      = run(15.0, 10.0)

    assert no_cost["costs"] == 0.0
    assert only_fee["costs"] > 0.0
    assert only_slip["costs"] > 0.0
    assert both["costs"] > only_fee["costs"]

    assert only_fee["pnl_net"] < no_cost["pnl_net"]
    assert only_slip["pnl_net"] < no_cost["pnl_net"]
    assert both["pnl_net"] < only_fee["pnl_net"]
    assert both["pnl_net"] < only_slip["pnl_net"]

    # costs tracks all fills (buy + sell); pnl_net deducts only sell-side costs,
    # so pnl_net < pnl_gross when any sell-side fee exists.
    assert both["pnl_net"] < both["pnl_gross"]
    assert both["pnl_gross"] - both["pnl_net"] > 0.0


# ---------------------------------------------------------------------------
# 3. max_drawdown is monotonic when losses are added
# ---------------------------------------------------------------------------

def test_max_drawdown_monotonic(tmp_path: Path, capsys, monkeypatch) -> None:
    """Adding a losing trade must not decrease max_drawdown."""
    store_win = tmp_path / "win.jsonl"
    store_loss = tmp_path / "loss.jsonl"
    scorecard_out = tmp_path / "sc.json"

    # Baseline: one winning trade.
    _write(store_win, _fill_events("strat_dd", "s-dd", [(100.0, 101.0)]))
    row_win = _strategy(
        _run(monkeypatch, capsys, store_win, scorecard_out, ["--fee-bps=0", "--slippage-bps=0"]),
        "strat_dd",
    )

    # Same strategy with an additional losing trade.
    _write(store_loss, _fill_events("strat_dd", "s-dd", [(100.0, 101.0), (100.0, 99.0)]))
    row_loss = _strategy(
        _run(monkeypatch, capsys, store_loss, scorecard_out, ["--fee-bps=0", "--slippage-bps=0"]),
        "strat_dd",
    )

    assert row_loss["max_drawdown"] >= row_win["max_drawdown"], (
        f"drawdown should not decrease after a loss: "
        f"before={row_win['max_drawdown']}, after={row_loss['max_drawdown']}"
    )
    assert row_loss["losses"] > row_win["losses"]


# ---------------------------------------------------------------------------
# 4. profit_factor is 0 when there are no winning trades
# ---------------------------------------------------------------------------

def test_profit_factor_zero_when_no_wins(tmp_path: Path, capsys, monkeypatch) -> None:
    """When every closed trade is a loss, profit_factor must be 0.0."""
    store = tmp_path / "events.jsonl"
    scorecard_out = tmp_path / "sc.json"
    # Buy high, sell low → unambiguous loss.
    events = _fill_events("strat_nw", "s-nw", [(100.0, 90.0)])
    _write(store, events)
    row = _strategy(
        _run(monkeypatch, capsys, store, scorecard_out, ["--fee-bps=0", "--slippage-bps=0"]),
        "strat_nw",
    )

    assert row["wins"] == 0
    assert row["losses"] >= 1
    assert row["profit_factor"] == 0.0, f"expected 0.0, got {row['profit_factor']}"
    assert row["pnl_gross"] < 0.0


# ---------------------------------------------------------------------------
# 5. classify does NOT promote when profit_factor is below threshold
# ---------------------------------------------------------------------------

def test_classify_promotes_only_when_pf_gt_threshold(tmp_path: Path, capsys, monkeypatch) -> None:
    """A strategy with MIN_FILLS_FOR_STATUS trades but PF < 1.2 must NOT be PROMOTE_CANDIDATE."""
    store = tmp_path / "events.jsonl"
    scorecard_out = tmp_path / "sc.json"

    # 2 wins of +0.05 and 1 loss of -0.09 → PF = 0.10/0.09 ≈ 1.11, pnl_gross > 0.
    # max_drawdown stays well below FREEZE_MAX_DRAWDOWN (0.15).
    pairs = [
        (100.0, 100.05),   # +0.05
        (100.0, 100.05),   # +0.05
        (100.0,  99.91),   # -0.09
    ]
    _write(store, _fill_events("strat_pf", "s-pf", pairs))
    row = _strategy(
        _run(monkeypatch, capsys, store, scorecard_out, ["--fee-bps=0", "--slippage-bps=0"]),
        "strat_pf",
    )

    assert row["trades"] >= agent.MIN_FILLS_FOR_STATUS, "test setup: need enough trades"
    assert row["profit_factor"] is not None
    assert row["profit_factor"] < agent.PROMOTE_MIN_PROFIT_FACTOR, (
        f"test setup: PF must be below threshold, got {row['profit_factor']}"
    )
    assert row["status"] != "PROMOTE_CANDIDATE", (
        f"strategy with PF={row['profit_factor']} must not be promoted"
    )


# ---------------------------------------------------------------------------
# 6. scorecard output goes to tmp_path, never to repo runtime/
# ---------------------------------------------------------------------------

def test_scorecard_writes_runtime_file_does_not_pollute_repo(tmp_path: Path, capsys, monkeypatch) -> None:
    """--scorecard-output must write only to the given path, not to runtime/ in the repo."""
    store = tmp_path / "events.jsonl"
    scorecard_out = tmp_path / "scorecard.json"
    repo_runtime = Path("runtime/crypto_strategy_scorecard.json")

    _write(
        store,
        [
            {
                "event_type": "crypto.signal.generated",
                "payload": {"strategy_id": "strat_rt", "signal_id": "srt"},
                "linkage": {"signal_id": "srt"},
            }
        ],
    )

    existed_before = repo_runtime.exists()
    mtime_before = repo_runtime.stat().st_mtime if existed_before else None

    _run(monkeypatch, capsys, store, scorecard_out)

    # The file we requested must exist and be valid.
    assert scorecard_out.exists()
    disk = json.loads(scorecard_out.read_text(encoding="utf-8"))
    assert disk["actor"] == agent.AGENT_ID
    assert isinstance(disk["strategies"], list)

    # The repo runtime file must not have been created or modified by this run.
    if not existed_before:
        assert not repo_runtime.exists(), "run must not create runtime/crypto_strategy_scorecard.json"
    else:
        assert repo_runtime.stat().st_mtime == mtime_before, (
            "run must not modify the existing runtime/crypto_strategy_scorecard.json"
        )


# ---------------------------------------------------------------------------
# 7. every key the incubator reads is present in each scorecard row
# ---------------------------------------------------------------------------

def test_scorecard_emits_all_keys_incubator_expects(tmp_path: Path, capsys, monkeypatch) -> None:
    """Every row must carry all keys consumed by crypto_strategy_incubator_agent.evaluate_status
    and the evaluated_strategies block."""
    store = tmp_path / "events.jsonl"
    scorecard_out = tmp_path / "sc.json"
    _write(store, _base_events())
    payload = _run(monkeypatch, capsys, store, scorecard_out)

    assert payload["strategies"], "need at least one row to validate keys"
    for row in payload["strategies"]:
        missing = INCUBATOR_REQUIRED_KEYS - set(row.keys())
        assert not missing, (
            f"Row for '{row.get('strategy_id')}' is missing keys: {sorted(missing)}"
        )
        # Values must be serialisable (None is allowed, not float inf/nan).
        re_encoded = json.loads(json.dumps(row))
        assert re_encoded == row, "row must be JSON-safe"


# ---------------------------------------------------------------------------
# 8. deterministic output — same store + same params => identical JSON
# ---------------------------------------------------------------------------

def test_scorecard_deterministic_output(tmp_path: Path, capsys, monkeypatch) -> None:
    """Running the scorecard twice on identical input must produce byte-identical JSON."""
    store = tmp_path / "events.jsonl"
    scorecard_out = tmp_path / "sc.json"
    _write(store, _base_events())

    run1 = _run(monkeypatch, capsys, store, scorecard_out)
    run2 = _run(monkeypatch, capsys, store, scorecard_out)

    # Compare as dicts (order-insensitive for top-level keys, but strategies list order matters).
    assert run1 == run2, "scorecard output must be deterministic"

    # Also confirm the serialised JSON strings are identical (no floating timestamps).
    out1 = json.dumps(run1, sort_keys=True)
    out2 = json.dumps(run2, sort_keys=True)
    assert out1 == out2


# ---------------------------------------------------------------------------
# Retained regression tests
# ---------------------------------------------------------------------------

def test_scorecard_aggregates_by_strategy(tmp_path: Path, capsys, monkeypatch) -> None:
    store = tmp_path / "events.jsonl"
    scorecard_out = tmp_path / "sc.json"
    _write(store, _base_events())
    payload = _run(monkeypatch, capsys, store, scorecard_out)
    strategies = {item["strategy_id"]: item for item in payload["strategies"]}

    assert strategies["crypto_adx_ema_pullback_v1"]["signals_generated"] == 1
    assert strategies["crypto_adx_ema_pullback_v1"]["decisions_formed"] == 1
    assert strategies["crypto_adx_ema_pullback_v1"]["fills"] == 1
    assert strategies["crypto_volatility_breakout_v1"]["vetoes"] == 1


def test_scorecard_no_fake_notional_formula(tmp_path: Path, capsys, monkeypatch) -> None:
    """pnl_gross must equal qty*(sell-buy), not 0.001*notional."""
    store = tmp_path / "events.jsonl"
    scorecard_out = tmp_path / "sc.json"
    events = _fill_events("strat_nf", "s-nf", [(100.0, 80.0)])
    _write(store, events)
    row = _strategy(
        _run(monkeypatch, capsys, store, scorecard_out, ["--fee-bps=0", "--slippage-bps=0"]),
        "strat_nf",
    )

    real_pnl = 1.0 * (80.0 - 100.0)     # -20.0
    fake_pnl = 1.0 * 80.0 * 0.001       # 0.08
    assert abs(row["pnl_gross"] - real_pnl) < 1e-6
    assert abs(row["pnl_gross"] - fake_pnl) > 1.0


def test_scorecard_kill_status_all_vetoed(tmp_path: Path, capsys, monkeypatch) -> None:
    store = tmp_path / "events.jsonl"
    scorecard_out = tmp_path / "sc.json"
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
    row = _strategy(_run(monkeypatch, capsys, store, scorecard_out), "strat_kill")
    assert row["status"] == "KILL"
