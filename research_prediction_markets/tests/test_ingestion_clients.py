from __future__ import annotations

from research_prediction_markets.ingestion.kalshi_client import KalshiClient
from research_prediction_markets.ingestion.polymarket_client import PolymarketClient


def test_polymarket_parse_market_normalizes_percentages_and_status(fixed_now) -> None:
    client = PolymarketClient()

    record = client._parse_market(
        {
            "conditionId": "pm-1",
            "question": "Will it rain?",
            "outcomePrices": "[\"62\", \"38\"]",
            "bestBid": None,
            "bestAsk": None,
            "volume24hr": "1500",
            "liquidity": "3000",
            "active": True,
            "closed": False,
            "endDate": "2026-04-15T00:00:00Z",
        },
        fetched_at=fixed_now,
    )

    assert record is not None
    assert record.market_id == "pm-1"
    assert record.probability == 0.62
    assert record.yes_bid == 0.61
    assert record.yes_ask == 0.63
    assert record.volume_24h == 1500.0
    assert record.open_interest == 3000.0
    assert record.status == "open"


def test_polymarket_parse_trade_defaults_unknown_outcome_to_yes(fixed_now) -> None:
    client = PolymarketClient()

    trade = client._parse_trade(
        {
            "transactionHash": "tx-1",
            "conditionId": "pm-1",
            "price": "55%",
            "size": "10",
            "outcome": "unknown",
            "timestamp": fixed_now.isoformat(),
            "side": "BUY",
        }
    )

    assert trade is not None
    assert trade.price == 0.55
    assert trade.size == 10.0
    assert trade.side == "yes"
    assert trade.taker is True


def test_kalshi_parse_market_coerces_invalid_ask_and_maps_status(fixed_now) -> None:
    client = KalshiClient()

    record = client._parse_market(
        {
            "ticker": "KX-1",
            "title": "Will x happen?",
            "yes_bid": 0.62,
            "yes_ask": 0.40,
            "last_price": 0.58,
            "status": "finalized",
            "close_time": "2026-04-16T00:00:00Z",
            "volume_24h": "99",
            "open_interest": "120",
        },
        fetched_at=fixed_now,
    )

    assert record is not None
    assert record.market_id == "KX-1"
    assert record.yes_bid == 0.62
    assert record.yes_ask == 0.62
    assert record.probability == 0.58
    assert record.status == "closed"
    assert record.volume_24h == 99.0
    assert record.open_interest == 120.0


def test_parse_market_returns_none_when_required_fields_are_missing(fixed_now) -> None:
    assert PolymarketClient()._parse_market({}, fetched_at=fixed_now) is None
    assert KalshiClient()._parse_market({}, fetched_at=fixed_now) is None
