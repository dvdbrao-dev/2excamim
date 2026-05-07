#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import sys
import uuid
from datetime import datetime, timezone
from pathlib import Path

from agents.core.event_store import append_event_idempotent
from agents.core.time import execution_run_id, utc_now_rfc3339
from agents.services.crypto_ohlcv import (
    Candle,
    adx,
    atr,
    ema,
    fetch_klines,
    parse_candles,
    rsi,
    write_ohlcv_cache,
)

AGENT_ID = "crypto-adx-ema-pullback-agent-v1"
PRODUCED_BY = "runtime.agent.crypto_strategy"
STRATEGY_ID = "crypto_adx_ema_pullback_v1"
DEFAULT_STORE = Path("./var/events.jsonl")
DEFAULT_CACHE_DIR = Path("./var/crypto_ohlcv")
SYMBOLS = ("BTCUSDT", "ETHUSDT", "SOLUSDT")
TIMEFRAMES = ("1h", "4h")
LOOKBACK = 220


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Generate ADX+EMA pullback crypto paper signals.")
    parser.add_argument("--store", default=str(DEFAULT_STORE))
    parser.add_argument("--cache-dir", default=str(DEFAULT_CACHE_DIR))
    return parser.parse_args()


def iso_from_ms(timestamp_ms: int) -> str:
    return datetime.fromtimestamp(timestamp_ms / 1000.0, tz=timezone.utc).isoformat().replace("+00:00", "Z")


def evaluate_signal(candles: list[Candle]) -> dict[str, float | str] | None:
    if len(candles) < 120:
        return None

    closes = [c.close for c in candles]
    ema21 = ema(closes, 21)
    ema50 = ema(closes, 50)
    adx14 = adx(candles, 14)
    atr14 = atr(candles, 14)
    rsi14 = rsi(closes, 14)

    if None in (ema21, ema50, adx14, atr14, rsi14):
        return None

    prev_closes = closes[:-1]
    prev_rsi = rsi(prev_closes, 14)
    if prev_rsi is None:
        return None

    last = candles[-1]
    touched_ema21 = abs(last.close - ema21) <= (0.003 * last.close)
    rsi_recovered = prev_rsi < 50.0 and rsi14 >= 50.0

    if not (ema21 > ema50 and adx14 > 25.0 and touched_ema21 and rsi_recovered):
        return None

    entry = last.close
    stop = entry - (1.5 * atr14)
    if stop <= 0 or stop >= entry:
        return None
    take = entry + 2.0 * (entry - stop)

    return {
        "entry_price": entry,
        "stop_loss": stop,
        "take_profit": take,
        "risk_reward": 2.0,
        "ema21": ema21,
        "ema50": ema50,
        "adx14": adx14,
        "rsi14": rsi14,
        "atr14": atr14,
        "prev_rsi14": prev_rsi,
    }


def build_event(symbol: str, timeframe: str, close_time_ms: int, run_id: str, metrics: dict[str, float | str]) -> dict:
    close_ts = iso_from_ms(close_time_ms)
    signal_id = f"{STRATEGY_ID}:{symbol}:{timeframe}:{close_time_ms}"
    reason = (
        "EMA21>EMA50, ADX14>25, pullback to EMA21, RSI14 recovers/crosses 50, "
        "LONG-only paper setup"
    )

    return {
        "event_id": str(uuid.uuid4()),
        "event_type": "crypto.signal.generated",
        "schema_version": "v1",
        "occurred_at": utc_now_rfc3339(),
        "produced_by": PRODUCED_BY,
        "idempotency_key": f"crypto.signal.generated:v1:{STRATEGY_ID}:{symbol}:{timeframe}:{close_time_ms}",
        "aggregate_key": f"crypto:{symbol}:{timeframe}",
        "linkage": {
            "signal_id": signal_id,
            "correlation_id": signal_id,
            "hypothesis_id": None,
            "decision_id": None,
            "order_id": None,
            "position_id": None,
            "parent_event_id": None,
        },
        "provenance": {
            "source_kind": "Runtime",
            "source_ref": "https://api.binance.com/api/v3/klines",
            "producer_run_id": run_id,
            "actor": AGENT_ID,
            "trace_id": f"{run_id}:{symbol}:{timeframe}",
            "notes": f"strategy_id={STRATEGY_ID} candle_close_time={close_ts}",
        },
        "payload": {
            "signal_id": signal_id,
            "strategy_id": STRATEGY_ID,
            "symbol": symbol,
            "timeframe": timeframe,
            "side": "LONG",
            "entry_price": metrics["entry_price"],
            "stop_loss": metrics["stop_loss"],
            "take_profit": metrics["take_profit"],
            "risk_reward": metrics["risk_reward"],
            "indicators": {
                "ema21": metrics["ema21"],
                "ema50": metrics["ema50"],
                "adx14": metrics["adx14"],
                "rsi14": metrics["rsi14"],
                "atr14": metrics["atr14"],
                "prev_rsi14": metrics["prev_rsi14"],
            },
            "reason": reason,
            "generated_at": utc_now_rfc3339(),
            "candle_close_time": close_ts,
        },
    }


def main() -> int:
    args = parse_args()
    store_path = Path(args.store)
    cache_dir = Path(args.cache_dir)
    run_id = execution_run_id(AGENT_ID)

    generated = 0
    skipped = 0
    failed: list[str] = []

    for symbol in SYMBOLS:
        for timeframe in TIMEFRAMES:
            try:
                klines = fetch_klines(symbol, timeframe, LOOKBACK)
                candles = parse_candles(klines)
                write_ohlcv_cache(cache_dir, STRATEGY_ID, symbol, timeframe, candles)
                metrics = evaluate_signal(candles)
                if metrics is None:
                    skipped += 1
                    continue
                close_time_ms = candles[-1].close_time_ms
                event = build_event(symbol, timeframe, close_time_ms, run_id, metrics)
                if append_event_idempotent(store_path, event):
                    generated += 1
                else:
                    skipped += 1
            except SystemExit:
                raise
            except Exception as error:
                failed.append(f"{symbol}:{timeframe}:{error}")

    print(
        json.dumps(
            {
                "actor": AGENT_ID,
                "producer_run_id": run_id,
                "strategy_id": STRATEGY_ID,
                "signals_generated": generated,
                "signals_skipped": skipped,
                "failures": failed,
                "store": str(store_path),
            },
            separators=(",", ":"),
        )
    )
    return 0 if not failed else 1


if __name__ == "__main__":
    sys.exit(main())
