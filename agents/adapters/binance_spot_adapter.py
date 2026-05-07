from __future__ import annotations

from dataclasses import dataclass

from .http_client import HttpJsonClient


@dataclass(frozen=True)
class SpotPriceResult:
    price: float | None
    symbol: str
    latency_ms: int
    error: str | None


class BinanceSpotAdapter:
    BASE_URL = "https://api.binance.com/api/v3/ticker/price"
    SYMBOLS = {
        "BTC": "BTCUSDT",
        "ETH": "ETHUSDT",
        "SOL": "SOLUSDT",
    }
    VERSION = "binance_spot_adapter_v1"

    def __init__(self, timeout_sec: float = 5.0, max_retries: int = 2, client: HttpJsonClient | None = None) -> None:
        self.client = client or HttpJsonClient(timeout_sec=timeout_sec, max_retries=max_retries)

    def observe(self, asset: str) -> SpotPriceResult:
        symbol = self.SYMBOLS.get(asset.upper(), "")
        if not symbol:
            return SpotPriceResult(price=None, symbol="", latency_ms=0, error=f"unsupported_asset:{asset}")

        result = self.client.get_json(self.BASE_URL, params={"symbol": symbol})
        if not result.ok or not isinstance(result.data, dict):
            return SpotPriceResult(None, symbol, result.latency_ms, result.error or "http_failed")

        raw_price = result.data.get("price")
        try:
            return SpotPriceResult(float(raw_price), symbol, result.latency_ms, None)
        except (TypeError, ValueError):
            return SpotPriceResult(None, symbol, result.latency_ms, "invalid_price")
