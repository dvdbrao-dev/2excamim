from __future__ import annotations

import pytest
from agents.services.crypto_ohlcv import Candle
from scripts.ohlcv_backtester import run_backtest, run_walk_forward


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def make_candle(
    i: int,
    open_: float = 100.0,
    high: float = 100.0,
    low: float = 100.0,
    close: float = 100.0,
    volume: float = 1_000.0,
) -> Candle:
    return Candle(
        open_time_ms=i * 3_600_000,
        open=open_,
        high=high,
        low=low,
        close=close,
        volume=volume,
        close_time_ms=i * 3_600_000 + 3_599_999,
    )


def make_candle_at(
    ts_ms: int,
    open_: float = 100.0,
    high: float = 100.0,
    low: float = 100.0,
    close: float = 100.0,
    volume: float = 1_000.0,
) -> Candle:
    return Candle(
        open_time_ms=ts_ms,
        open=open_,
        high=high,
        low=low,
        close=close,
        volume=volume,
        close_time_ms=ts_ms + 9,
    )


def flat_candles(n: int, price: float = 100.0) -> list[Candle]:
    return [make_candle(i, open_=price, high=price, low=price, close=price) for i in range(n)]


def one_shot_signal(entry: float, stop: float, take: float):
    """Signals exactly once on the very first evaluate call."""
    signaled = False

    def _fn(candles: list[Candle]):
        nonlocal signaled
        if not signaled:
            signaled = True
            return {"entry_price": entry, "stop_loss": stop, "take_profit": take}
        return None

    return _fn


def signal_at_idx(target: int, entry: float, stop: float, take: float):
    """Signals only when len(candles_slice) == target + 1."""

    def _fn(candles: list[Candle]):
        if len(candles) == target + 1:
            return {"entry_price": entry, "stop_loss": stop, "take_profit": take}
        return None

    return _fn


# ---------------------------------------------------------------------------
# Single-backtest tests (unchanged)
# ---------------------------------------------------------------------------

def test_stop_loss_triggers_on_low_below_stop():
    candles = [
        make_candle(0),
        make_candle(1, low=90.0),
        make_candle(2),
    ]
    result = run_backtest(
        one_shot_signal(entry=100.0, stop=95.0, take=110.0),
        candles,
        fee_bps=0.0,
        slippage_bps=0.0,
        initial_bankroll=10_000.0,
        max_position_fraction=0.1,
    )
    assert len(result["trade_details"]) == 1
    trade = result["trade_details"][0]
    assert trade["exit_reason"] == "stop_loss"
    assert trade["exit_price"] == 95.0
    assert trade["exit_idx"] == 1


def test_take_profit_triggers_on_high_above_target():
    candles = [
        make_candle(0),
        make_candle(1, high=115.0),
        make_candle(2),
    ]
    result = run_backtest(
        one_shot_signal(entry=100.0, stop=95.0, take=110.0),
        candles,
        fee_bps=0.0,
        slippage_bps=0.0,
        initial_bankroll=10_000.0,
        max_position_fraction=0.1,
    )
    assert len(result["trade_details"]) == 1
    trade = result["trade_details"][0]
    assert trade["exit_reason"] == "take_profit"
    assert trade["exit_price"] == 110.0
    assert trade["exit_idx"] == 1


def test_stop_and_take_in_same_candle_takes_stop_first():
    candles = [
        make_candle(0),
        make_candle(1, high=115.0, low=90.0),
    ]
    result = run_backtest(
        one_shot_signal(entry=100.0, stop=95.0, take=110.0),
        candles,
        fee_bps=0.0,
        slippage_bps=0.0,
        initial_bankroll=10_000.0,
        max_position_fraction=0.1,
    )
    assert len(result["trade_details"]) == 1
    trade = result["trade_details"][0]
    assert trade["exit_reason"] == "stop_loss"
    assert trade["exit_price"] == 95.0


def test_no_lookahead():
    n = 200
    candles = flat_candles(n)
    call_log: list[int] = []

    def tracking_strategy(candles_slice: list[Candle]):
        call_log.append(len(candles_slice))
        return None

    run_backtest(tracking_strategy, candles, fee_bps=0.0, slippage_bps=0.0)

    assert call_log == list(range(1, n + 1))


def test_fees_and_slippage_subtract_from_pnl():
    candles = [
        make_candle(0),
        make_candle(1, high=125.0),
    ]

    result_with_costs = run_backtest(
        one_shot_signal(entry=100.0, stop=90.0, take=120.0),
        candles,
        fee_bps=10.0,
        slippage_bps=5.0,
        initial_bankroll=10_000.0,
        max_position_fraction=0.1,
    )
    result_no_costs = run_backtest(
        one_shot_signal(entry=100.0, stop=90.0, take=120.0),
        candles,
        fee_bps=0.0,
        slippage_bps=0.0,
        initial_bankroll=10_000.0,
        max_position_fraction=0.1,
    )

    assert result_with_costs["costs"] > 0.0
    assert result_no_costs["costs"] == 0.0
    assert result_with_costs["pnl_net"] < result_no_costs["pnl_net"]


def test_metrics_match_known_fixture():
    candles = flat_candles(155)
    candles[154] = make_candle(154, open_=100.0, high=115.0, low=99.0, close=100.0)

    result = run_backtest(
        signal_at_idx(149, entry=100.0, stop=95.0, take=110.0),
        candles,
        fee_bps=10.0,
        slippage_bps=5.0,
        initial_bankroll=10_000.0,
        max_position_fraction=0.1,
    )

    assert result["trades"] == 1
    assert result["wins"] == 1
    assert result["losses"] == 0
    assert result["pnl_gross"] == pytest.approx(100.0)
    assert result["costs"] == pytest.approx(3.15)
    assert result["pnl_net"] == pytest.approx(98.35)
    assert result["winrate"] == pytest.approx(1.0)
    assert result["max_drawdown"] == pytest.approx(0.0)
    assert result["profit_factor"] is None
    assert result["expectancy"] == pytest.approx(98.35)
    assert result["trade_details"][0]["exit_reason"] == "take_profit"
    assert result["trade_details"][0]["entry_idx"] == 149
    assert result["trade_details"][0]["exit_idx"] == 154


# ---------------------------------------------------------------------------
# Walk-forward tests
# ---------------------------------------------------------------------------

def test_walkforward_outputs_windows():
    # 80 candles at 10 ms apart: open_time_ms = 0, 10, 20, …, 790
    # With _month_ms=100: train_ms=600, test_ms=100
    # Window 1: train=[0,600),  test=[600,700)  → included (test_start=600 ≤ last_ts=790)
    # Window 2: train=[100,700), test=[700,800) → included (test_start=700 ≤ last_ts=790)
    # Window 3: test_start=800 > last_ts=790    → break
    candles = [make_candle_at(i * 10) for i in range(80)]

    result = run_walk_forward(
        lambda c: None,  # never signals
        candles,
        train_months=6,
        test_months=1,
        fee_bps=0.0,
        slippage_bps=0.0,
        _month_ms=100,
    )

    assert len(result["windows"]) == 2
    assert result["windows"][0]["train_start_ms"] == 0
    assert result["windows"][0]["test_start_ms"] == 600
    assert result["windows"][1]["train_start_ms"] == 100
    assert result["windows"][1]["test_start_ms"] == 700


def test_walkforward_rejects_when_pf_bad():
    # All candles have low=95, so every trade stops out (stop=99 is always hit)
    candles = [make_candle_at(i * 10, low=95.0) for i in range(80)]

    def always_losing(c: list[Candle]):
        return {"entry_price": 100.0, "stop_loss": 99.0, "take_profit": 200.0}

    result = run_walk_forward(
        always_losing,
        candles,
        train_months=6,
        test_months=1,
        fee_bps=0.0,
        slippage_bps=0.0,
        initial_bankroll=1_000.0,
        max_position_fraction=0.1,
        _month_ms=100,
    )

    # Both OOS windows have only losses → 100 % > 40 % → REJECT
    assert result["recommended_decision"] == "REJECT"
    assert result["oos_windows_total"] >= 2
    assert result["oos_windows_pf_below_1"] == result["oos_windows_total"]


def test_deterministic_output_same_data_same_params():
    candles = [make_candle(0), make_candle(1, high=115.0), make_candle(2)]

    r1 = run_backtest(
        one_shot_signal(100.0, 90.0, 110.0), candles, fee_bps=10.0, slippage_bps=5.0
    )
    r2 = run_backtest(
        one_shot_signal(100.0, 90.0, 110.0), candles, fee_bps=10.0, slippage_bps=5.0
    )

    assert r1["pnl_net"] == r2["pnl_net"]
    assert r1["trades"] == r2["trades"]
    assert r1["trade_details"] == r2["trade_details"]
