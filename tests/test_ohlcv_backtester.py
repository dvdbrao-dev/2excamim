from __future__ import annotations

import pytest
from agents.services.crypto_ohlcv import Candle
from scripts.ohlcv_backtester import run_backtest


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
# Tests
# ---------------------------------------------------------------------------

def test_stop_loss_triggers_on_low_below_stop():
    candles = [
        make_candle(0),                       # signal fires here
        make_candle(1, low=90.0),             # low=90 < stop=95
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
        make_candle(0),                       # signal fires here
        make_candle(1, high=115.0),           # high=115 > take=110
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
    # Both low<stop and high>take on the same candle — stop wins.
    candles = [
        make_candle(0),
        make_candle(1, high=115.0, low=90.0),  # both triggered
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
        return None  # never signals — forces evaluation on every candle

    run_backtest(tracking_strategy, candles, fee_bps=0.0, slippage_bps=0.0)

    # evaluate_signal must be called once per candle with exactly i+1 candles
    assert call_log == list(range(1, n + 1))


def test_fees_and_slippage_subtract_from_pnl():
    candles = [
        make_candle(0),
        make_candle(1, high=125.0),  # take_profit=120 hit
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
    # 155 candles flat at 100; candle 149 triggers the signal,
    # candle 154 hits take_profit (high=115 > take=110).
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

    # position_usd = 10000 * 0.1 = 1000, qty = 1000/100 = 10
    # gross = 10 * (110 - 100) = 100
    # buy_costs  = 1000 * 15/10000 = 1.5
    # sell_costs = 1100 * 15/10000 = 1.65
    # pnl_net (pnl.py) = gross - sell_costs = 100 - 1.65 = 98.35
    # total costs = 1.5 + 1.65 = 3.15

    assert result["trades"] == 1
    assert result["wins"] == 1
    assert result["losses"] == 0
    assert result["pnl_gross"] == pytest.approx(100.0)
    assert result["costs"] == pytest.approx(3.15)
    assert result["pnl_net"] == pytest.approx(98.35)
    assert result["winrate"] == pytest.approx(1.0)
    assert result["max_drawdown"] == pytest.approx(0.0)
    assert result["profit_factor"] is None  # infinite (no losses)
    assert result["expectancy"] == pytest.approx(98.35)
    assert result["trade_details"][0]["exit_reason"] == "take_profit"
    assert result["trade_details"][0]["entry_idx"] == 149
    assert result["trade_details"][0]["exit_idx"] == 154
