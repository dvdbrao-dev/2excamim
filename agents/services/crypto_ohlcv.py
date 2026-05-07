from __future__ import annotations

import json
import math
from dataclasses import dataclass
from pathlib import Path
from typing import Any
from urllib.error import HTTPError, URLError
from urllib.parse import urlencode
from urllib.request import Request, urlopen

BINANCE_KLINES_URL = "https://api.binance.com/api/v3/klines"


@dataclass(frozen=True)
class Candle:
    open_time_ms: int
    open: float
    high: float
    low: float
    close: float
    volume: float
    close_time_ms: int


def _num(value: Any) -> float | None:
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


def fetch_klines(symbol: str, interval: str, limit: int) -> list[list[Any]]:
    query = urlencode({"symbol": symbol, "interval": interval, "limit": limit})
    request = Request(
        f"{BINANCE_KLINES_URL}?{query}",
        headers={"User-Agent": "2excamim-crypto-strategies/1.0"},
    )
    try:
        with urlopen(request, timeout=20) as response:
            payload = response.read().decode("utf-8")
    except HTTPError as error:
        raise SystemExit(f"binance request failed for {symbol} {interval}: HTTP {error.code}") from error
    except URLError as error:
        raise SystemExit(f"binance request failed for {symbol} {interval}: {error.reason}") from error

    try:
        data = json.loads(payload)
    except json.JSONDecodeError as error:
        raise SystemExit(f"invalid Binance response for {symbol} {interval}: {error}") from error

    if not isinstance(data, list):
        raise SystemExit(f"unexpected Binance payload for {symbol} {interval}: expected list")
    return [item for item in data if isinstance(item, list)]


def parse_candles(klines: list[list[Any]]) -> list[Candle]:
    candles: list[Candle] = []
    for row in klines:
        if len(row) < 7:
            continue
        open_time = _num(row[0])
        open_px = _num(row[1])
        high_px = _num(row[2])
        low_px = _num(row[3])
        close_px = _num(row[4])
        volume = _num(row[5])
        close_time = _num(row[6])
        if None in (open_time, open_px, high_px, low_px, close_px, volume, close_time):
            continue
        candles.append(
            Candle(
                open_time_ms=int(open_time),
                open=open_px,
                high=high_px,
                low=low_px,
                close=close_px,
                volume=volume,
                close_time_ms=int(close_time),
            )
        )
    return candles


def write_ohlcv_cache(cache_dir: Path, strategy_id: str, symbol: str, timeframe: str, candles: list[Candle]) -> None:
    cache_dir.mkdir(parents=True, exist_ok=True)
    path = cache_dir / f"{strategy_id}-{symbol}-{timeframe}.json"
    payload = {
        "strategy_id": strategy_id,
        "symbol": symbol,
        "timeframe": timeframe,
        "candles": [
            {
                "open_time_ms": c.open_time_ms,
                "open": c.open,
                "high": c.high,
                "low": c.low,
                "close": c.close,
                "volume": c.volume,
                "close_time_ms": c.close_time_ms,
            }
            for c in candles
        ],
    }
    path.write_text(json.dumps(payload, separators=(",", ":")), encoding="utf-8")


def sma(values: list[float], period: int) -> float | None:
    if period <= 0 or len(values) < period:
        return None
    window = values[-period:]
    return sum(window) / float(period)


def ema(values: list[float], period: int) -> float | None:
    if period <= 0 or len(values) < period:
        return None
    alpha = 2.0 / (period + 1.0)
    seed = sum(values[:period]) / float(period)
    current = seed
    for value in values[period:]:
        current = alpha * value + (1.0 - alpha) * current
    return current


def rsi(values: list[float], period: int) -> float | None:
    if period <= 0 or len(values) < period + 1:
        return None

    gains: list[float] = []
    losses: list[float] = []
    for idx in range(1, len(values)):
        delta = values[idx] - values[idx - 1]
        gains.append(max(delta, 0.0))
        losses.append(max(-delta, 0.0))

    avg_gain = sum(gains[:period]) / float(period)
    avg_loss = sum(losses[:period]) / float(period)

    for idx in range(period, len(gains)):
        avg_gain = ((avg_gain * (period - 1)) + gains[idx]) / float(period)
        avg_loss = ((avg_loss * (period - 1)) + losses[idx]) / float(period)

    if avg_loss == 0.0:
        return 100.0
    rs = avg_gain / avg_loss
    return 100.0 - (100.0 / (1.0 + rs))


def atr(candles: list[Candle], period: int) -> float | None:
    if period <= 0 or len(candles) < period + 1:
        return None

    tr_values: list[float] = []
    prev_close = candles[0].close
    for candle in candles[1:]:
        tr = max(
            candle.high - candle.low,
            abs(candle.high - prev_close),
            abs(candle.low - prev_close),
        )
        tr_values.append(tr)
        prev_close = candle.close

    if len(tr_values) < period:
        return None

    atr_value = sum(tr_values[:period]) / float(period)
    for tr in tr_values[period:]:
        atr_value = ((atr_value * (period - 1)) + tr) / float(period)
    return atr_value


def adx(candles: list[Candle], period: int) -> float | None:
    if period <= 0 or len(candles) < (2 * period + 1):
        return None

    plus_dm: list[float] = []
    minus_dm: list[float] = []
    tr_values: list[float] = []

    for i in range(1, len(candles)):
        current = candles[i]
        prev = candles[i - 1]

        up_move = current.high - prev.high
        down_move = prev.low - current.low

        plus_dm.append(up_move if up_move > down_move and up_move > 0 else 0.0)
        minus_dm.append(down_move if down_move > up_move and down_move > 0 else 0.0)

        tr_values.append(
            max(
                current.high - current.low,
                abs(current.high - prev.close),
                abs(current.low - prev.close),
            )
        )

    if len(tr_values) < period:
        return None

    tr_n = sum(tr_values[:period])
    plus_dm_n = sum(plus_dm[:period])
    minus_dm_n = sum(minus_dm[:period])

    dx_values: list[float] = []
    for i in range(period, len(tr_values)):
        tr_n = tr_n - (tr_n / period) + tr_values[i]
        plus_dm_n = plus_dm_n - (plus_dm_n / period) + plus_dm[i]
        minus_dm_n = minus_dm_n - (minus_dm_n / period) + minus_dm[i]

        if tr_n == 0:
            continue

        plus_di = 100.0 * (plus_dm_n / tr_n)
        minus_di = 100.0 * (minus_dm_n / tr_n)
        denom = plus_di + minus_di
        if denom == 0:
            dx = 0.0
        else:
            dx = 100.0 * abs(plus_di - minus_di) / denom
        dx_values.append(dx)

    if len(dx_values) < period:
        return None

    adx_value = sum(dx_values[:period]) / float(period)
    for dx in dx_values[period:]:
        adx_value = ((adx_value * (period - 1)) + dx) / float(period)
    return adx_value


def stddev(values: list[float], period: int) -> float | None:
    if period <= 0 or len(values) < period:
        return None
    subset = values[-period:]
    mean = sum(subset) / float(period)
    variance = sum((value - mean) ** 2 for value in subset) / float(period)
    return variance**0.5


def macd_histogram(values: list[float], fast: int, slow: int, signal: int) -> float | None:
    if len(values) < slow + signal:
        return None

    macd_line: list[float] = []
    for i in range(slow, len(values) + 1):
        window = values[:i]
        fast_ema = ema(window, fast)
        slow_ema = ema(window, slow)
        if fast_ema is None or slow_ema is None:
            continue
        macd_line.append(fast_ema - slow_ema)

    signal_line = ema(macd_line, signal)
    if signal_line is None or not macd_line:
        return None
    return macd_line[-1] - signal_line
