from __future__ import annotations

import json
from datetime import datetime, timezone
from pathlib import Path
from unittest.mock import patch

from agents import crypto_adx_ema_pullback_agent as agent


def _kline(ts_open_ms: int, open_px: float, high: float, low: float, close: float, vol: float, tf_ms: int) -> list:
    return [ts_open_ms, str(open_px), str(high), str(low), str(close), str(vol), ts_open_ms + tf_ms - 1]


def _build_trending_klines(length: int = 180, base: float = 100.0, tf_ms: int = 60 * 60 * 1000) -> list[list]:
    klines: list[list] = []
    current = base
    for i in range(length):
        current = current + 0.2
        close = current
        open_px = current - 0.1
        high = current + 0.6
        low = current - 0.6
        klines.append(_kline(i * tf_ms, open_px, high, low, close, 1000 + i, tf_ms))

    # Pullback/recovery candle near EMA and RSI recovery to satisfy rule.
    last_ts = length * tf_ms
    klines.append(_kline(last_ts, current - 0.1, current + 0.4, current - 0.9, current + 0.05, 2000, tf_ms))
    return klines


def test_generates_and_is_idempotent(tmp_path: Path, monkeypatch) -> None:
    store = tmp_path / "events.jsonl"
    cache = tmp_path / "crypto_ohlcv"

    klines = _build_trending_klines()
    monkeypatch.setattr(
        "sys.argv",
        [
            "crypto_adx_ema_pullback_agent.py",
            "--store",
            str(store),
            "--cache-dir",
            str(cache),
        ],
    )

    def fake_fetch(symbol: str, timeframe: str, limit: int):
        assert symbol in ("BTCUSDT", "ETHUSDT", "SOLUSDT")
        assert timeframe in ("1h", "4h")
        return klines

    fixed_metrics = {
        "entry_price": 101.0,
        "stop_loss": 99.5,
        "take_profit": 104.0,
        "risk_reward": 2.0,
        "ema21": 100.5,
        "ema50": 99.8,
        "adx14": 27.0,
        "rsi14": 51.0,
        "atr14": 1.0,
        "prev_rsi14": 48.0,
    }
    fixed_candle = agent.Candle(
        open_time_ms=0,
        open=100.0,
        high=102.0,
        low=99.0,
        close=101.0,
        volume=1234.0,
        close_time_ms=int(datetime.now(timezone.utc).timestamp() * 1000),
    )

    with (
        patch("agents.crypto_adx_ema_pullback_agent.fetch_klines", side_effect=fake_fetch),
        patch("agents.crypto_adx_ema_pullback_agent.evaluate_signal", return_value=fixed_metrics),
        patch("agents.crypto_adx_ema_pullback_agent.parse_candles", return_value=[fixed_candle]),
    ):
        rc1 = agent.main()
        rc2 = agent.main()

    assert rc1 == 0
    assert rc2 == 0
    lines = [json.loads(line) for line in store.read_text(encoding="utf-8").splitlines() if line.strip()]

    assert len(lines) >= 1
    keys = [item["idempotency_key"] for item in lines]
    assert len(keys) == len(set(keys))
    sample = lines[0]
    assert sample["event_type"] == "crypto.signal.generated"
    assert sample["payload"]["strategy_id"] == "crypto_adx_ema_pullback_v1"
    assert sample["payload"]["side"] == "LONG"
    assert (cache / "crypto_adx_ema_pullback_v1-BTCUSDT-1h.json").exists()
