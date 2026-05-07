from __future__ import annotations

import json
from pathlib import Path
from urllib.error import HTTPError, URLError
from urllib.parse import urlencode
from urllib.request import Request, urlopen

from agents.services.crypto_ohlcv import Candle, parse_candles

BINANCE_KLINES_URL = "https://api.binance.com/api/v3/klines"
PAGE_SIZE = 1000


def _fetch_page(
    symbol: str,
    interval: str,
    start_ms: int,
    end_ms: int,
    limit: int = PAGE_SIZE,
) -> list[Candle]:
    query = urlencode(
        {
            "symbol": symbol,
            "interval": interval,
            "startTime": start_ms,
            "endTime": end_ms,
            "limit": limit,
        }
    )
    request = Request(
        f"{BINANCE_KLINES_URL}?{query}",
        headers={"User-Agent": "2excamim-ohlcv-history/1.0"},
    )
    try:
        with urlopen(request, timeout=30) as response:
            payload = response.read().decode("utf-8")
    except HTTPError as error:
        raise SystemExit(
            f"Binance request failed for {symbol} {interval}: HTTP {error.code}"
        ) from error
    except URLError as error:
        raise SystemExit(
            f"Binance request failed for {symbol} {interval}: {error.reason}"
        ) from error

    try:
        data = json.loads(payload)
    except json.JSONDecodeError as error:
        raise SystemExit(
            f"Invalid Binance response for {symbol} {interval}: {error}"
        ) from error

    if not isinstance(data, list):
        raise SystemExit(
            f"Unexpected Binance payload for {symbol} {interval}: expected list"
        )

    return parse_candles([row for row in data if isinstance(row, list)])


def _read_cache(path: Path) -> list[Candle]:
    if not path.exists():
        return []
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


def _write_cache(path: Path, candles: list[Candle]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as fh:
        for c in candles:
            fh.write(
                json.dumps(
                    {
                        "open_time_ms": c.open_time_ms,
                        "open": c.open,
                        "high": c.high,
                        "low": c.low,
                        "close": c.close,
                        "volume": c.volume,
                        "close_time_ms": c.close_time_ms,
                    },
                    separators=(",", ":"),
                )
                + "\n"
            )


def validate_monotonic(candles: list[Candle]) -> None:
    for i in range(1, len(candles)):
        if candles[i].open_time_ms <= candles[i - 1].open_time_ms:
            raise ValueError(
                f"Non-monotonic candle at index {i}: "
                f"open_time_ms {candles[i].open_time_ms} <= {candles[i - 1].open_time_ms}"
            )


def fetch_history(
    symbol: str,
    interval: str,
    start_ms: int,
    end_ms: int,
    cache_dir: Path,
) -> list[Candle]:
    cache_path = cache_dir / f"{symbol}-{interval}.jsonl"
    cached = _read_cache(cache_path)

    in_range = [
        c for c in cached if c.open_time_ms >= start_ms and c.open_time_ms < end_ms
    ]
    fetch_start = in_range[-1].close_time_ms + 1 if in_range else start_ms

    new_candles: list[Candle] = []
    if fetch_start < end_ms:
        batch_start = fetch_start
        while batch_start < end_ms:
            page = [
                c
                for c in _fetch_page(symbol, interval, batch_start, end_ms)
                if c.open_time_ms < end_ms
            ]
            if not page:
                break
            new_candles.extend(page)
            last_open = page[-1].open_time_ms
            if last_open <= batch_start:
                break  # no progress — avoid infinite loop
            batch_start = page[-1].close_time_ms + 1

    merged_map: dict[int, Candle] = {c.open_time_ms: c for c in cached}
    for c in new_candles:
        merged_map[c.open_time_ms] = c
    merged = sorted(merged_map.values(), key=lambda c: c.open_time_ms)
    validate_monotonic(merged)

    if new_candles:
        _write_cache(cache_path, merged)

    return [c for c in merged if c.open_time_ms >= start_ms and c.open_time_ms < end_ms]
