from __future__ import annotations

from research_prediction_markets.ingestion.kalshi_client import KalshiClient
from research_prediction_markets.ingestion.polymarket_client import PolymarketClient
from research_prediction_markets.schemas.records import MarketRecord, TradeRecord


def fetch_kalshi_markets() -> list[MarketRecord]:
    return KalshiClient().get_markets()


def fetch_kalshi_trades() -> list[TradeRecord]:
    return KalshiClient().get_trades()


def fetch_polymarket_markets() -> list[MarketRecord]:
    return PolymarketClient().get_markets()


def fetch_polymarket_trades() -> list[TradeRecord]:
    return PolymarketClient().get_recent_trades()


def fetch_all_markets() -> list[MarketRecord]:
    return fetch_kalshi_markets() + fetch_polymarket_markets()


def fetch_all_trades() -> list[TradeRecord]:
    return fetch_kalshi_trades() + fetch_polymarket_trades()
