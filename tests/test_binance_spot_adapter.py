from __future__ import annotations

from agents.adapters.binance_spot_adapter import BinanceSpotAdapter
from agents.adapters.http_client import HttpResult


class OkClient:
    def get_json(self, url: str, params: dict[str, str] | None = None) -> HttpResult:
        _ = (url, params)
        return HttpResult(ok=True, status_code=200, data={"symbol": "BTCUSDT", "price": "100000.5"}, error=None, latency_ms=12, url="x")


class FailClient:
    def get_json(self, url: str, params: dict[str, str] | None = None) -> HttpResult:
        _ = (url, params)
        return HttpResult(ok=False, status_code=None, data=None, error="timeout", latency_ms=51, url="x")


def test_binance_adapter_parses_btc_price() -> None:
    adapter = BinanceSpotAdapter(client=OkClient())
    row = adapter.observe("BTC")
    assert row.symbol == "BTCUSDT"
    assert row.price == 100000.5
    assert row.error is None


def test_binance_adapter_handles_timeout_error() -> None:
    adapter = BinanceSpotAdapter(client=FailClient())
    row = adapter.observe("BTC")
    assert row.price is None
    assert row.error == "timeout"
