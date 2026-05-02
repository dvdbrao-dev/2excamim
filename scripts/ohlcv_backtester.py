#!/usr/bin/env python3
from __future__ import annotations

import argparse
import importlib
import json
import sys
from datetime import datetime, timezone
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

MONTH_MS: int = 30 * 24 * 3600 * 1000  # ~30 days in milliseconds

EvaluateFn = Callable[[list[Candle]], dict[str, Any] | None]


def _load_strategy(strategy_id: str) -> EvaluateFn:
    module_name = STRATEGY_MODULES.get(strategy_id)
    if module_name is None:
        raise ValueError(
            f"Unknown strategy_id: {strategy_id!r}. Available: {sorted(STRATEGY_MODULES)}"
        )
    module = importlib.import_module(module_name)
    return module.evaluate_signal  # type: ignore[attr-defined]


def _date_to_ms(date_str: str) -> int:
    dt = datetime.strptime(date_str, "%Y-%m-%d").replace(tzinfo=timezone.utc)
    return int(dt.timestamp() * 1000)


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


def _is_pf_below_1(profit_factor: float | None) -> bool:
    # None means infinite profit_factor (only wins) — not below 1
    if profit_factor is None:
        return False
    return profit_factor < 1.0


def run_walk_forward(
    strategy: str | EvaluateFn,
    candles: list[Candle],
    train_months: int = 6,
    test_months: int = 1,
    fee_bps: float = 10.0,
    slippage_bps: float = 5.0,
    initial_bankroll: float = 10_000.0,
    max_position_fraction: float = 0.10,
    _month_ms: int = MONTH_MS,
) -> dict[str, Any]:
    evaluate_fn: EvaluateFn = (
        _load_strategy(strategy) if isinstance(strategy, str) else strategy
    )

    if not candles:
        return {
            "strategy_id": strategy if isinstance(strategy, str) else getattr(evaluate_fn, "__module__", "custom"),
            "total_candles": 0,
            "train_months": train_months,
            "test_months": test_months,
            "windows": [],
            "oos_windows_total": 0,
            "oos_windows_pf_below_1": 0,
            "oos_pf_below_1_rate": None,
            "recommended_decision": "INCONCLUSIVE",
        }

    train_ms = train_months * _month_ms
    test_ms = test_months * _month_ms
    first_ts = candles[0].open_time_ms
    last_ts = candles[-1].open_time_ms

    windows: list[dict[str, Any]] = []
    window_start = first_ts

    while True:
        train_start = window_start
        train_end = window_start + train_ms
        test_start = train_end
        test_end = test_start + test_ms

        if test_start > last_ts:
            break

        train_candles = [c for c in candles if train_start <= c.open_time_ms < train_end]
        test_candles = [c for c in candles if test_start <= c.open_time_ms < test_end]

        if not test_candles:
            break

        train_result = run_backtest(
            evaluate_fn, train_candles, fee_bps, slippage_bps, initial_bankroll, max_position_fraction
        )
        oos_result = run_backtest(
            evaluate_fn, test_candles, fee_bps, slippage_bps, initial_bankroll, max_position_fraction
        )

        # Strip verbose trade_details from window summaries to keep output concise
        train_summary = {k: v for k, v in train_result.items() if k != "trade_details"}
        oos_summary = {k: v for k, v in oos_result.items() if k != "trade_details"}

        windows.append(
            {
                "window_idx": len(windows),
                "train_start_ms": train_start,
                "train_end_ms": train_end,
                "test_start_ms": test_start,
                "test_end_ms": test_end,
                "train_candles": len(train_candles),
                "test_candles": len(test_candles),
                "train": train_summary,
                "oos": oos_summary,
            }
        )

        window_start += test_ms

    oos_total = len(windows)
    pf_below_1 = sum(
        1 for w in windows if _is_pf_below_1(w["oos"]["profit_factor"])
    )
    rate = pf_below_1 / oos_total if oos_total > 0 else 0.0

    if oos_total < 2:
        recommended = "INCONCLUSIVE"
    elif rate > 0.40:
        recommended = "REJECT"
    else:
        recommended = "PASS"

    return {
        "strategy_id": strategy if isinstance(strategy, str) else getattr(evaluate_fn, "__module__", "custom"),
        "total_candles": len(candles),
        "train_months": train_months,
        "test_months": test_months,
        "windows": windows,
        "oos_windows_total": oos_total,
        "oos_windows_pf_below_1": pf_below_1,
        "oos_pf_below_1_rate": round(rate, 6) if oos_total > 0 else None,
        "recommended_decision": recommended,
    }


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Minimal OHLCV backtester.")
    parser.add_argument("--strategy", required=True, choices=list(STRATEGY_MODULES))

    # Input mode 1: single JSONL file
    parser.add_argument("--input", help="Path to JSONL OHLCV file (single backtest)")

    # Input mode 2: history download + walk-forward
    parser.add_argument("--symbols", help="Comma-separated symbols, e.g. BTCUSDT,ETHUSDT,SOLUSDT")
    parser.add_argument("--interval", default="1h")
    parser.add_argument("--from", dest="from_date", help="Start date YYYY-MM-DD")
    parser.add_argument("--to", dest="to_date", help="End date YYYY-MM-DD")
    parser.add_argument("--cache-dir", default="data/ohlcv")

    # Walk-forward params
    parser.add_argument("--walk-forward", action="store_true")
    parser.add_argument("--train-months", type=int, default=6)
    parser.add_argument("--test-months", type=int, default=1)

    # Common params
    parser.add_argument("--fee-bps", type=float, default=10.0)
    parser.add_argument("--slippage-bps", type=float, default=5.0)
    parser.add_argument("--initial-bankroll", type=float, default=10_000.0)
    parser.add_argument("--max-position-fraction", type=float, default=0.10)
    parser.add_argument("--json", action="store_true", help="Output JSON")
    return parser.parse_args()


def main() -> int:
    args = _parse_args()

    if args.walk_forward:
        from agents.services.ohlcv_history import fetch_history

        if not args.symbols or not args.from_date or not args.to_date:
            print(
                "Error: --symbols, --from, and --to are required with --walk-forward",
                file=sys.stderr,
            )
            return 1

        symbols = [s.strip() for s in args.symbols.split(",")]
        start_ms = _date_to_ms(args.from_date)
        end_ms = _date_to_ms(args.to_date)
        cache_dir = Path(args.cache_dir)

        all_results: dict[str, Any] = {}
        for symbol in symbols:
            candles = fetch_history(symbol, args.interval, start_ms, end_ms, cache_dir)
            all_results[symbol] = run_walk_forward(
                strategy=args.strategy,
                candles=candles,
                train_months=args.train_months,
                test_months=args.test_months,
                fee_bps=args.fee_bps,
                slippage_bps=args.slippage_bps,
                initial_bankroll=args.initial_bankroll,
                max_position_fraction=args.max_position_fraction,
            )

        if args.json:
            print(json.dumps(all_results, separators=(",", ":")))
        else:
            for symbol, result in all_results.items():
                print(f"\n=== {symbol} ===")
                print(f"Decision: {result['recommended_decision']}")
                print(f"Windows:  {result['oos_windows_total']}")
                print(f"OOS PF<1: {result['oos_windows_pf_below_1']}/{result['oos_windows_total']}")
        return 0

    # Single backtest mode (original behaviour)
    if not args.input:
        print("Error: --input required for single backtest mode", file=sys.stderr)
        return 1

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
