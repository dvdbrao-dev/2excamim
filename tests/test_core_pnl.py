from __future__ import annotations

import pytest

from agents.core.pnl import (
    FillInput,
    bps_cost,
    calculate_realized_pnl_from_fills,
    derive_outcome,
    max_drawdown,
    summarize_trades,
)


def test_bps_cost_basic() -> None:
    assert bps_cost(1000.0, 15.0) == pytest.approx(1.5)
    assert bps_cost(0.0, 15.0) == 0.0
    assert bps_cost(1000.0, 0.0) == 0.0


def test_max_drawdown_empty() -> None:
    assert max_drawdown([]) == 0.0


def test_max_drawdown_flat() -> None:
    assert max_drawdown([1.0, 1.0, 1.0]) == 0.0


def test_max_drawdown_simple() -> None:
    assert max_drawdown([0.0, 5.0, 2.0, 8.0, 3.0]) == pytest.approx(5.0)


def test_max_drawdown_increases_with_loss() -> None:
    curve_no_loss = [0.0, 2.0, 4.0]
    curve_with_loss = [0.0, 2.0, 4.0, 1.0]
    assert max_drawdown(curve_with_loss) >= max_drawdown(curve_no_loss)


def test_derive_outcome_yes() -> None:
    assert derive_outcome("pm-paper-order-yes-abc") == "yes"


def test_derive_outcome_no() -> None:
    assert derive_outcome("pm-paper-order-no-abc") == "no"


def test_derive_outcome_signal_direction() -> None:
    assert derive_outcome("pm-paper-order-sig-42", "sig-42") == "signal_direction"


def test_derive_outcome_unknown() -> None:
    assert derive_outcome("random-order-id") == "unknown"


def test_buy_sell_pnl_gross() -> None:
    fills = [
        FillInput(side="BUY", quantity=10.0, price=0.20),
        FillInput(side="SELL", quantity=10.0, price=0.40),
    ]
    result = calculate_realized_pnl_from_fills(fills)
    assert result.pnl_gross == pytest.approx(2.0)


def test_fees_reduce_pnl_net() -> None:
    fills = [
        FillInput(side="BUY", quantity=10.0, price=0.20),
        FillInput(side="SELL", quantity=10.0, price=0.40),
    ]
    result_no_fee = calculate_realized_pnl_from_fills(fills, fee_bps=0.0)
    result_with_fee = calculate_realized_pnl_from_fills(fills, fee_bps=15.0, slippage_bps=10.0)
    assert result_with_fee.pnl_net < result_no_fee.pnl_net
    assert result_with_fee.costs > 0.0


def test_losing_trade_pnl_negative() -> None:
    fills = [
        FillInput(side="BUY", quantity=10.0, price=0.50),
        FillInput(side="SELL", quantity=10.0, price=0.20),
    ]
    result = calculate_realized_pnl_from_fills(fills)
    assert result.pnl_gross < 0.0
    assert result.losses == 1
    assert result.wins == 0


def test_max_drawdown_after_loss() -> None:
    fills_win = [
        FillInput(side="BUY", quantity=10.0, price=0.20),
        FillInput(side="SELL", quantity=10.0, price=0.40),
    ]
    fills_loss = [
        FillInput(side="BUY", quantity=10.0, price=0.50),
        FillInput(side="SELL", quantity=10.0, price=0.20),
    ]
    result_win = calculate_realized_pnl_from_fills(fills_win)
    result_loss = calculate_realized_pnl_from_fills(fills_win + fills_loss)
    assert result_loss.max_drawdown >= result_win.max_drawdown


def test_profit_factor_zero_when_no_wins() -> None:
    fills = [
        FillInput(side="BUY", quantity=5.0, price=1.0),
        FillInput(side="SELL", quantity=5.0, price=0.5),
    ]
    result = calculate_realized_pnl_from_fills(fills)
    assert result.profit_factor == 0.0


def test_profit_factor_with_wins() -> None:
    fills = [
        FillInput(side="BUY", quantity=10.0, price=1.0),
        FillInput(side="SELL", quantity=10.0, price=2.0),
    ]
    result = calculate_realized_pnl_from_fills(fills)
    assert result.profit_factor is None or result.profit_factor > 0


def test_deterministic_output() -> None:
    fills = [
        FillInput(side="BUY", quantity=10.0, price=0.20),
        FillInput(side="SELL", quantity=10.0, price=0.40),
    ]
    r1 = summarize_trades(calculate_realized_pnl_from_fills(fills, fee_bps=15.0))
    r2 = summarize_trades(calculate_realized_pnl_from_fills(fills, fee_bps=15.0))
    assert r1 == r2


def test_winrate_none_with_no_trades() -> None:
    result = calculate_realized_pnl_from_fills([])
    assert result.winrate is None
    assert result.expectancy is None


def test_summarize_trades_keys() -> None:
    fills = [
        FillInput(side="BUY", quantity=1.0, price=100.0),
        FillInput(side="SELL", quantity=1.0, price=110.0),
    ]
    summary = summarize_trades(calculate_realized_pnl_from_fills(fills))
    for key in ("pnl_gross", "costs", "pnl_net", "trades", "wins", "losses",
                "winrate", "profit_factor", "expectancy", "max_drawdown"):
        assert key in summary
