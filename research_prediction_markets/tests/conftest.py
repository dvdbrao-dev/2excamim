from __future__ import annotations

from datetime import datetime, timezone

import pytest

from research_prediction_markets.schemas.records import FeatureRecord, MarketRecord, TradeRecord


@pytest.fixture
def fixed_now() -> datetime:
    return datetime(2026, 4, 14, 12, 0, tzinfo=timezone.utc)


@pytest.fixture
def sample_market(fixed_now: datetime) -> MarketRecord:
    return MarketRecord(
        market_id="market-1",
        source="kalshi",
        question="Will event happen?",
        probability=0.40,
        yes_bid=0.39,
        yes_ask=0.41,
        volume_24h=300.0,
        open_interest=1200.0,
        status="open",
        resolution_time=fixed_now.replace(hour=18),
        fetched_at=fixed_now,
    )


@pytest.fixture
def sample_trades(fixed_now: datetime) -> list[TradeRecord]:
    return [
        TradeRecord(
            trade_id="trade-1",
            market_id="market-1",
            source="kalshi",
            price=0.35,
            size=100.0,
            side="yes",
            timestamp=fixed_now.replace(hour=11, minute=30),
            taker=True,
        ),
        TradeRecord(
            trade_id="trade-2",
            market_id="market-1",
            source="kalshi",
            price=0.45,
            size=50.0,
            side="no",
            timestamp=fixed_now.replace(hour=11, minute=45),
            taker=False,
        ),
    ]


@pytest.fixture
def sample_features(fixed_now: datetime) -> list[FeatureRecord]:
    return [
        FeatureRecord(
            market_id="market-1",
            timestamp=fixed_now,
            feature_name="spread_tight",
            value=0.98,
            metadata={"source": "kalshi"},
        ),
        FeatureRecord(
            market_id="market-1",
            timestamp=fixed_now,
            feature_name="volume_spike_24h",
            value=2.5,
            metadata={"source": "kalshi"},
        ),
        FeatureRecord(
            market_id="market-1",
            timestamp=fixed_now,
            feature_name="price_deviation_vwap_1h",
            value=0.04,
            metadata={"source": "kalshi"},
        ),
    ]
