"""Read-only market data adapters for external candidate research."""

from .binance_spot_adapter import BinanceSpotAdapter
from .polymarket_metadata_adapter import PolymarketMetadataAdapter
from .polymarket_orderbook_adapter import PolymarketOrderBookAdapter

__all__ = [
    "BinanceSpotAdapter",
    "PolymarketMetadataAdapter",
    "PolymarketOrderBookAdapter",
]
