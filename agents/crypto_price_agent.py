#!/usr/bin/env python3
"""Crypto price signal agent.

Fetches the latest Binance 1m klines for BTCUSDT, ETHUSDT, and SOLUSDT on
every run, computes a short-horizon price move, and appends
`crypto.signal.generated` events when the move clears the configured threshold.
"""

from __future__ import annotations

import argparse
import json
import math
import statistics
import sys
import uuid
from datetime import datetime, timezone
from pathlib import Path
from typing import Any
from urllib.error import HTTPError, URLError
from urllib.parse import urlencode
from urllib.request import Request, urlopen


AGENT_ID = "crypto-price-agent-v1"
PRODUCED_BY = "runtime.agent.crypto_price"
DEFAULT_STORE = Path("./var/events.jsonl")
DEFAULT_CACHE_DIR = Path("./var/crypto")
BINANCE_BASE_URL = "https://api.binance.com/api/v3/klines"
SYMBOLS = ("BTCUSDT", "ETHUSDT", "SOLUSDT")
INTERVAL = "1m"
LIMIT = 20
LOOKBACK_CANDLES = 15
IDEMPOTENCY_WINDOW_SECONDS = 5 * 60


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Generate crypto signals from Binance klines.")
    parser.add_argument("--store", default=str(DEFAULT_STORE), help="Path to the JSONL store.")
    parser.add_argument(
        "--cache-dir",
        default=str(DEFAULT_CACHE_DIR),
        help="Path to the crypto cache directory.",
    )
    parser.add_argument(
        "--threshold",
        type=float,
        default=2.0,
        help="Absolute percentage change threshold for signal generation.",
    )
    return parser.parse_args()


def utc_now_rfc3339() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def execution_run_id() -> str:
    timestamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S%fZ")
    return f"{AGENT_ID}-{timestamp}"


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    if not path.exists():
        return []

    records: list[dict[str, Any]] = []
    for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        stripped = line.strip()
        if not stripped:
            continue
        try:
            records.append(json.loads(stripped))
        except json.JSONDecodeError as error:
            raise SystemExit(f"invalid JSONL in {path} at line {line_number}: {error}") from error
    return records


def parse_timestamp(value: Any) -> datetime | None:
    if not isinstance(value, str):
        return None
    trimmed = value.strip()
    if not trimmed:
        return None
    try:
        return datetime.fromisoformat(trimmed.replace("Z", "+00:00")).astimezone(timezone.utc)
    except ValueError:
        return None


def numeric_value(value: Any) -> float | None:
    if isinstance(value, (int, float)) and math.isfinite(float(value)):
        return float(value)
    if isinstance(value, str):
        try:
            parsed = float(value)
        except ValueError:
            return None
        if math.isfinite(parsed):
            return parsed
    return None


def fetch_klines(symbol: str) -> list[list[Any]]:
    query = urlencode({"symbol": symbol, "interval": INTERVAL, "limit": LIMIT})
    request = Request(
        f"{BINANCE_BASE_URL}?{query}",
        headers={"User-Agent": "2excamim-crypto-price-agent/1.0"},
    )
    try:
        with urlopen(request, timeout=20) as response:
            payload = response.read().decode("utf-8")
    except HTTPError as error:
        raise SystemExit(f"binance request failed for {symbol}: HTTP {error.code}") from error
    except URLError as error:
        raise SystemExit(f"binance request failed for {symbol}: {error.reason}") from error

    try:
        data = json.loads(payload)
    except json.JSONDecodeError as error:
        raise SystemExit(f"invalid Binance response for {symbol}: {error}") from error

    if not isinstance(data, list):
        raise SystemExit(f"unexpected Binance payload for {symbol}: expected list")

    klines: list[list[Any]] = []
    for item in data:
        if isinstance(item, list):
            klines.append(item)
    return klines


def close_prices(klines: list[list[Any]]) -> list[float]:
    closes: list[float] = []
    for kline in klines:
        if len(kline) <= 4:
            continue
        close = numeric_value(kline[4])
        if close is not None:
            closes.append(close)
    return closes


def latest_close_time(klines: list[list[Any]]) -> datetime | None:
    if not klines:
        return None
    close_time_ms = numeric_value(klines[-1][6]) if len(klines[-1]) > 6 else None
    if close_time_ms is None:
        return None
    return datetime.fromtimestamp(close_time_ms / 1000.0, tz=timezone.utc)


def compute_metrics(closes: list[float]) -> dict[str, float] | None:
    if len(closes) < LOOKBACK_CANDLES + 1:
        return None

    price_now = closes[-1]
    price_15m_ago = closes[-(LOOKBACK_CANDLES + 1)]
    if price_15m_ago == 0:
        return None

    change_pct = ((price_now - price_15m_ago) / price_15m_ago) * 100.0
    last_15_closes = closes[-LOOKBACK_CANDLES:]
    volatility = 0.0
    if price_now != 0:
        volatility = statistics.pstdev(last_15_closes) / price_now

    return {
        "price_now": price_now,
        "price_15m_ago": price_15m_ago,
        "change_pct": change_pct,
        "volatility": volatility,
    }


def trend_from_change(change_pct: float) -> str:
    return "UP" if change_pct > 0 else "DOWN"


def recent_signal_exists(
    events: list[dict[str, Any]], symbol: str, now: datetime
) -> tuple[bool, datetime | None]:
    latest_seen: datetime | None = None
    for event in events:
        if event.get("event_type") != "crypto.signal.generated":
            continue
        payload = event.get("payload") or {}
        if payload.get("symbol") != symbol:
            continue
        occurred_at = parse_timestamp(event.get("occurred_at"))
        if occurred_at is None:
            continue
        if latest_seen is None or occurred_at > latest_seen:
            latest_seen = occurred_at

    if latest_seen is None:
        return False, None

    return (now - latest_seen).total_seconds() < IDEMPOTENCY_WINDOW_SECONDS, latest_seen


def append_event(store_path: Path, event: dict[str, Any]) -> None:
    store_path.parent.mkdir(parents=True, exist_ok=True)
    with store_path.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(event, separators=(",", ":")) + "\n")


def write_cache(cache_dir: Path, payload: dict[str, Any]) -> None:
    cache_dir.mkdir(parents=True, exist_ok=True)
    cache_path = cache_dir / "latest_prices.json"
    cache_path.write_text(json.dumps(payload, indent=2, sort_keys=True), encoding="utf-8")


def build_signal_event(
    symbol: str,
    metrics: dict[str, float],
    candle_close_time: datetime | None,
    threshold: float,
    run_id: str,
) -> dict[str, Any]:
    price_now = metrics["price_now"]
    price_15m_ago = metrics["price_15m_ago"]
    change_pct = metrics["change_pct"]
    volatility = metrics["volatility"]
    trend = trend_from_change(change_pct)
    signal_strength = min(abs(change_pct) / 5.0, 1.0)
    signal_id = (
        f"crypto-{symbol}-"
        f"{candle_close_time.strftime('%Y%m%dT%H%M%SZ') if candle_close_time else datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')}"
    )

    return {
        "event_id": str(uuid.uuid4()),
        "event_type": "crypto.signal.generated",
        "schema_version": "v1",
        "occurred_at": utc_now_rfc3339(),
        "produced_by": PRODUCED_BY,
        "idempotency_key": f"crypto.signal.generated:v1:{signal_id}",
        "aggregate_key": f"crypto:{symbol}",
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
            "trace_id": f"{run_id}:{symbol}",
            "notes": (
                f"threshold={threshold:.2f} change_pct={change_pct:.6f} "
                f"trend={trend} volatility={volatility:.6f}"
            ),
        },
        "payload": {
            "signal_id": signal_id,
            "symbol": symbol,
            "price_now": price_now,
            "price_15m_ago": price_15m_ago,
            "change_pct": change_pct,
            "trend": trend,
            "volatility": volatility,
            "signal_strength": signal_strength,
            "suggested_markets": [],
            "threshold": threshold,
            "window_minutes": LOOKBACK_CANDLES,
            "source": "binance",
            "candle_close_time": candle_close_time.isoformat().replace("+00:00", "Z")
            if candle_close_time
            else None,
        },
    }


def main() -> int:
    args = parse_args()
    store_path = Path(args.store)
    cache_dir = Path(args.cache_dir)
    threshold = float(args.threshold)
    run_id = execution_run_id()

    events = load_jsonl(store_path)
    now = datetime.now(timezone.utc)

    generated = 0
    skipped_threshold = 0
    skipped_recent = 0
    failed_symbols: list[str] = []
    cache_summary: dict[str, Any] = {
        "actor": AGENT_ID,
        "producer_run_id": run_id,
        "generated_at": utc_now_rfc3339(),
        "threshold": threshold,
        "symbols": {},
    }

    for symbol in SYMBOLS:
        try:
            klines = fetch_klines(symbol)
            closes = close_prices(klines)
            metrics = compute_metrics(closes)
            candle_close_time = latest_close_time(klines)
        except SystemExit:
            raise
        except Exception as error:  # pragma: no cover - defensive guard for runtime failures
            failed_symbols.append(symbol)
            print(
                json.dumps(
                    {"actor": AGENT_ID, "symbol": symbol, "error": str(error)},
                    separators=(",", ":"),
                ),
                file=sys.stderr,
            )
            continue

        cache_summary["symbols"][symbol] = {
            "klines_read": len(klines),
            "closes_read": len(closes),
            "candle_close_time": candle_close_time.isoformat().replace("+00:00", "Z")
            if candle_close_time
            else None,
            "metrics": metrics,
        }

        if metrics is None:
            skipped_threshold += 1
            continue

        change_pct = metrics["change_pct"]
        if abs(change_pct) < threshold:
            skipped_threshold += 1
            continue

        recent, _ = recent_signal_exists(events, symbol, now)
        if recent:
            skipped_recent += 1
            continue

        event = build_signal_event(symbol, metrics, candle_close_time, threshold, run_id)
        append_event(store_path, event)
        events.append(event)
        generated += 1

    write_cache(cache_dir, cache_summary)

    print(
        json.dumps(
            {
                "actor": AGENT_ID,
                "producer_run_id": run_id,
                "store": str(store_path),
                "cache_dir": str(cache_dir),
                "threshold": threshold,
                "symbols_checked": len(SYMBOLS),
                "signals_generated": generated,
                "skipped_threshold": skipped_threshold,
                "skipped_recent": skipped_recent,
                "failed_symbols": failed_symbols,
            },
            separators=(",", ":"),
        )
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
