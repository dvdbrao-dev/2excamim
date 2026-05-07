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
    atr,
    ema,
    fetch_klines,
    macd_histogram,
    parse_candles,
    sma,
    stddev,
    write_ohlcv_cache,
)

AGENT_ID = "crypto-volatility-breakout-agent-v1"
PRODUCED_BY = "runtime.agent.crypto_strategy"
STRATEGY_ID = "crypto_volatility_breakout_v1"
DEFAULT_STORE = Path("./var/events.jsonl")
DEFAULT_CACHE_DIR = Path("./var/crypto_ohlcv")
SYMBOLS = ("BTCUSDT", "ETHUSDT", "SOLUSDT")
TIMEFRAME = "1h"
LOOKBACK = 220


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Generate volatility-breakout crypto paper signals.")
    parser.add_argument("--store", default=str(DEFAULT_STORE))
    parser.add_argument("--cache-dir", default=str(DEFAULT_CACHE_DIR))
    return parser.parse_args()


def iso_from_ms(timestamp_ms: int) -> str:
    return datetime.fromtimestamp(timestamp_ms / 1000.0, tz=timezone.utc).isoformat().replace("+00:00", "Z")


def evaluate_signal(candles: list[Candle]) -> dict[str, float | str] | None:
    if len(candles) < 120:
        return None

    closes = [c.close for c in candles]
    volumes = [c.volume for c in candles]

    bb_mid = sma(closes, 20)
    bb_std = stddev(closes, 20)
    atr14 = atr(candles, 14)
    macd_hist = macd_histogram(closes, 12, 26, 9)
    volume_sma20 = sma(volumes, 20)

    if None in (bb_mid, bb_std, atr14, macd_hist, volume_sma20):
        return None

    upper_band = bb_mid + (2.0 * bb_std)

    atr_series: list[float] = []
    for i in range(65, len(candles) + 1):
        value = atr(candles[:i], 14)
        if value is not None:
            atr_series.append(value)
    atr_sma50 = sma(atr_series, 50)
    if atr_sma50 is None:
        return None

    last = candles[-1]
    breakout = last.close > upper_band
    rising_volatility = atr14 > atr_sma50
    bullish_macd = macd_hist > 0.0
    volume_confirmed = last.volume > volume_sma20

    candle_size = abs(last.close - last.open)
    anti_fomo_ok = candle_size <= (2.5 * atr14)

    if not (breakout and rising_volatility and bullish_macd and volume_confirmed and anti_fomo_ok):
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
        "upper_band": upper_band,
        "atr14": atr14,
        "atr_sma50": atr_sma50,
        "macd_histogram": macd_hist,
        "volume": last.volume,
        "volume_sma20": volume_sma20,
        "candle_size": candle_size,
    }


def build_event(symbol: str, timeframe: str, close_time_ms: int, run_id: str, metrics: dict[str, float | str]) -> dict:
    close_ts = iso_from_ms(close_time_ms)
    signal_id = f"{STRATEGY_ID}:{symbol}:{timeframe}:{close_time_ms}"
    reason = (
        "Close breakout above BB(20,2), ATR14>ATR_SMA50, MACD histogram>0, volume>VOLUME_SMA20, "
        "anti-FOMO candle<=2.5*ATR14, LONG-only paper setup"
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
                "bb_upper_20_2": metrics["upper_band"],
                "atr14": metrics["atr14"],
                "atr_sma50": metrics["atr_sma50"],
                "macd_histogram_12_26_9": metrics["macd_histogram"],
                "volume": metrics["volume"],
                "volume_sma20": metrics["volume_sma20"],
                "breakout_candle_size": metrics["candle_size"],
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
        try:
            klines = fetch_klines(symbol, TIMEFRAME, LOOKBACK)
            candles = parse_candles(klines)
            write_ohlcv_cache(cache_dir, STRATEGY_ID, symbol, TIMEFRAME, candles)
            metrics = evaluate_signal(candles)
            if metrics is None:
                skipped += 1
                continue
            close_time_ms = candles[-1].close_time_ms
            event = build_event(symbol, TIMEFRAME, close_time_ms, run_id, metrics)
            if append_event_idempotent(store_path, event):
                generated += 1
            else:
                skipped += 1
        except SystemExit:
            raise
        except Exception as error:
            failed.append(f"{symbol}:{error}")

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
