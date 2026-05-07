from __future__ import annotations

import json
from pathlib import Path
from unittest.mock import patch

from agents import crypto_volatility_breakout_agent as agent


def _kline(ts_open_ms: int, open_px: float, high: float, low: float, close: float, vol: float, tf_ms: int) -> list:
    return [ts_open_ms, str(open_px), str(high), str(low), str(close), str(vol), ts_open_ms + tf_ms - 1]


def _build_breakout_klines(length: int = 220, base: float = 100.0, tf_ms: int = 60 * 60 * 1000) -> list[list]:
    klines: list[list] = []
    for i in range(length - 1):
        px = base + (i * 0.08)
        klines.append(_kline(i * tf_ms, px - 0.3, px + 0.8, px - 0.8, px, 1500 + i, tf_ms))

    i = length - 1
    px = base + (i * 0.08)
    # Breakout candle with controlled size (anti-FOMO) and high volume.
    klines.append(_kline(i * tf_ms, px - 0.2, px + 2.2, px - 1.0, px + 1.7, 9000, tf_ms))
    return klines


def test_generates_and_is_idempotent(tmp_path: Path, monkeypatch) -> None:
    store = tmp_path / "events.jsonl"
    cache = tmp_path / "crypto_ohlcv"
    monkeypatch.setattr(
        "sys.argv",
        [
            "crypto_volatility_breakout_agent.py",
            "--store",
            str(store),
            "--cache-dir",
            str(cache),
        ],
    )

    klines = _build_breakout_klines()

    with patch("agents.crypto_volatility_breakout_agent.fetch_klines", return_value=klines):
        rc1 = agent.main()
        rc2 = agent.main()

    assert rc1 == 0
    assert rc2 == 0

    lines = [json.loads(line) for line in store.read_text(encoding="utf-8").splitlines() if line.strip()]
    assert len(lines) >= 1
    keys = [item["idempotency_key"] for item in lines]
    assert len(keys) == len(set(keys))
    assert all(item["payload"]["strategy_id"] == "crypto_volatility_breakout_v1" for item in lines)
    assert (cache / "crypto_volatility_breakout_v1-BTCUSDT-1h.json").exists()
