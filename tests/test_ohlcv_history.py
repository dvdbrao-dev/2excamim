from __future__ import annotations

from pathlib import Path
from unittest.mock import call, patch

import pytest

from agents.services.crypto_ohlcv import Candle
from agents.services.ohlcv_history import (
    _read_cache,
    _write_cache,
    fetch_history,
    validate_monotonic,
)


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def make_candle(open_time_ms: int, price: float = 100.0) -> Candle:
    return Candle(
        open_time_ms=open_time_ms,
        open=price,
        high=price,
        low=price,
        close=price,
        volume=1_000.0,
        close_time_ms=open_time_ms + 9,
    )


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------

@patch("agents.services.ohlcv_history._fetch_page")
def test_history_cache_only_fetches_delta(mock_fetch, tmp_path: Path) -> None:
    # Pre-populate cache with candles at t=0 and t=10 (close_time_ms = 9 and 19)
    cache_dir = tmp_path / "ohlcv"
    cache_dir.mkdir()
    _write_cache(cache_dir / "BTCUSDT-1h.jsonl", [make_candle(0), make_candle(10)])

    mock_fetch.return_value = []  # no new data available

    fetch_history("BTCUSDT", "1h", start_ms=0, end_ms=50, cache_dir=cache_dir)

    # _fetch_page must be called exactly once, starting after the last cached close_time_ms
    # last cached candle: open_time_ms=10, close_time_ms=19  →  fetch_start = 20
    assert mock_fetch.call_count == 1
    called_start_ms = mock_fetch.call_args[0][2]  # positional arg: start_ms
    assert called_start_ms == 20


@patch("agents.services.ohlcv_history._fetch_page")
def test_history_dedup_and_monotonicity(mock_fetch, tmp_path: Path) -> None:
    # Cache has candles at t=0 and t=10
    cache_dir = tmp_path / "ohlcv"
    cache_dir.mkdir()
    _write_cache(cache_dir / "BTCUSDT-1h.jsonl", [make_candle(0), make_candle(10)])

    # Mock returns candle at t=10 (duplicate) and a new one at t=20
    mock_fetch.return_value = [make_candle(10), make_candle(20)]

    result = fetch_history("BTCUSDT", "1h", start_ms=0, end_ms=30, cache_dir=cache_dir)

    open_times = [c.open_time_ms for c in result]
    # Deduplicated: [0, 10, 20] — no duplicate t=10
    assert open_times == [0, 10, 20]
    assert len(open_times) == len(set(open_times))

    # Monotonicity guaranteed
    validate_monotonic(result)  # raises ValueError if violated


@patch("agents.services.ohlcv_history._fetch_page")
def test_history_full_cache_hit_skips_fetch(mock_fetch, tmp_path: Path) -> None:
    # Cache already covers the entire requested window
    cache_dir = tmp_path / "ohlcv"
    cache_dir.mkdir()
    candles = [make_candle(i * 10) for i in range(5)]  # t=0..40, close_time up to 49
    _write_cache(cache_dir / "BTCUSDT-1h.jsonl", candles)

    # Request window fully inside cache: start=0, end=50 → last close_time=49, fetch_start=50
    # fetch_start(50) >= end_ms(50) → no fetch
    result = fetch_history("BTCUSDT", "1h", start_ms=0, end_ms=50, cache_dir=cache_dir)

    mock_fetch.assert_not_called()
    assert [c.open_time_ms for c in result] == [0, 10, 20, 30, 40]


def test_validate_monotonic_raises_on_violation() -> None:
    good = [make_candle(0), make_candle(10), make_candle(20)]
    validate_monotonic(good)  # must not raise

    bad = [make_candle(0), make_candle(20), make_candle(10)]
    with pytest.raises(ValueError, match="Non-monotonic"):
        validate_monotonic(bad)
