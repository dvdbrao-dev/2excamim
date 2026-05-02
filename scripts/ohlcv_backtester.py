#!/usr/bin/env python3
from __future__ import annotations

import argparse
import importlib
import json
import sys
from pathlib import Path
from typing import Any, Callable

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from agents.core.pnl import FillInput, calculate_realized_pnl_from_fills, summarize_trades
from agents.services.crypto_ohlcv import Candle

STRATEGY_MODULES: dict[str, str] = {
    "crypto_adx_ema_pullback_v1": "agents.crypto_adx_ema_pullback_agent",
    "crypto_volatility_breakout_v1": "agents.crypto_volatility_breakout_agent",
}

EvaluateFn = Callable[[list[Candle]], dict[str, Any] | None]


def _load_strategy(strategy_id: str) -> EvaluateFn:
    module_name = STRATEGY_MODULES.get(strategy_id)
    if module_name is None:
        raise ValueError(
            f"Unknown strategy_id: {strategy_id!r}. Available: {sorted(STRATEGY_MODULES)}"
        )
    module = importlib.import_module(module_name)
    return module.evaluate_signal  # type: ignore[attr-defined]


def load_candles_jsonl(path: Path) -> list[Candle]:
    candles: list[Candle] = []
    with path.open(encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            obj = json.loads(line)
            candles.append(
                Candle(
                    open_time_ms=int(obj["open_time_ms"]),
                    open=float(obj["open"]),
                    high=float(obj["high"]),
                    low=float(obj["low"]),
                    close=float(obj["close"]),
                    volume=float(obj["volume"]),
                    close_time_ms=int(obj["close_time_ms"]),
                )
            )
    return candles


def run_backtest(
    strategy: str | EvaluateFn,
    candles: list[Candle],
    fee_bps: float = 10.0,
    slippage_bps: float = 5.0,
    initial_bankroll: float = 10_000.0,
    max_position_fraction: float = 0.10,
) -> dict[str, Any]:
    evaluate_signal: EvaluateFn = (
        _load_strategy(strategy) if isinstance(strategy, str) else strategy
    )
    total_bps = fee_bps + slippage_bps

    fills: list[FillInput] = []
    trade_details: list[dict[str, Any]] = []
    current_equity = initial_bankroll
    position: dict[str, Any] | None = None

    for i, candle in enumerate(candles):
        if position is not None:
            hit_stop = candle.low <= position["stop_loss"]
            hit_take = candle.high >= position["take_profit"]

            if hit_stop or hit_take:
                # Stop takes priority when both trigger in the same candle
                if hit_stop:
                    exit_price = position["stop_loss"]
                    exit_reason = "stop_loss"
                else:
                    exit_price = position["take_profit"]
                    exit_reason = "take_profit"

                qty = position["quantity"]
                fills.append(FillInput(side="SELL", quantity=qty, price=exit_price))

                gross = qty * (exit_price - position["entry_price"])
                exit_cost = qty * exit_price * total_bps / 10_000.0
                net = gross - exit_cost
                current_equity += net

                trade_details.append(
                    {
                        "entry_idx": position["entry_idx"],
                        "exit_idx": i,
                        "entry_price": position["entry_price"],
                        "exit_price": exit_price,
                        "exit_reason": exit_reason,
                        "quantity": qty,
                        "pnl_gross": round(gross, 8),
                        "pnl_net": round(net, 8),
                    }
                )
                position = None

        if position is None:
            signal = evaluate_signal(candles[: i + 1])
            if signal is not None:
                entry_price: float = float(signal["entry_price"])
                stop_loss: float = float(signal["stop_loss"])
                take_profit: float = float(signal["take_profit"])

                position_usd = current_equity * max_position_fraction
                quantity = position_usd / entry_price

                fills.append(FillInput(side="BUY", quantity=quantity, price=entry_price))
                position = {
                    "entry_price": entry_price,
                    "stop_loss": stop_loss,
                    "take_profit": take_profit,
                    "quantity": quantity,
                    "entry_idx": i,
                }

    # Force-close any open position at end of data
    if position is not None:
        last = candles[-1]
        exit_price = last.close
        qty = position["quantity"]
        fills.append(FillInput(side="SELL", quantity=qty, price=exit_price))

        gross = qty * (exit_price - position["entry_price"])
        exit_cost = qty * exit_price * total_bps / 10_000.0
        net = gross - exit_cost
        trade_details.append(
            {
                "entry_idx": position["entry_idx"],
                "exit_idx": len(candles) - 1,
                "entry_price": position["entry_price"],
                "exit_price": exit_price,
                "exit_reason": "end_of_data",
                "quantity": qty,
                "pnl_gross": round(gross, 8),
                "pnl_net": round(net, 8),
            }
        )

    summary = calculate_realized_pnl_from_fills(
        fills, fee_bps=fee_bps, slippage_bps=slippage_bps
    )
    metrics = summarize_trades(summary)

    return {
        "strategy_id": strategy if isinstance(strategy, str) else getattr(strategy, "__module__", "custom"),
        "total_candles": len(candles),
        "fee_bps": fee_bps,
        "slippage_bps": slippage_bps,
        "initial_bankroll": initial_bankroll,
        "max_position_fraction": max_position_fraction,
        "trade_details": trade_details,
        **metrics,
    }


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Minimal OHLCV backtester.")
    parser.add_argument("--strategy", required=True, choices=list(STRATEGY_MODULES))
    parser.add_argument("--input", required=True, help="Path to JSONL OHLCV file")
    parser.add_argument("--fee-bps", type=float, default=10.0)
    parser.add_argument("--slippage-bps", type=float, default=5.0)
    parser.add_argument("--initial-bankroll", type=float, default=10_000.0)
    parser.add_argument("--max-position-fraction", type=float, default=0.10)
    parser.add_argument("--json", action="store_true", help="Output JSON")
    return parser.parse_args()


def main() -> int:
    args = _parse_args()
    candles = load_candles_jsonl(Path(args.input))
    result = run_backtest(
        strategy=args.strategy,
        candles=candles,
        fee_bps=args.fee_bps,
        slippage_bps=args.slippage_bps,
        initial_bankroll=args.initial_bankroll,
        max_position_fraction=args.max_position_fraction,
    )
    if args.json:
        print(json.dumps(result, separators=(",", ":")))
    else:
        print(f"Strategy:   {result['strategy_id']}")
        print(f"Candles:    {result['total_candles']}")
        print(f"Trades:     {result['trades']}")
        print(f"Wins:       {result['wins']}")
        print(f"Losses:     {result['losses']}")
        print(f"Win rate:   {result['winrate']}")
        print(f"PnL gross:  {result['pnl_gross']}")
        print(f"PnL net:    {result['pnl_net']}")
        print(f"Max DD:     {result['max_drawdown']}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
