from __future__ import annotations

from collections import defaultdict
from datetime import datetime, timedelta, timezone

from research_prediction_markets.schemas.records import FeatureRecord, MarketRecord, TradeRecord


def compute_spread_tight(market: MarketRecord, timestamp: datetime) -> FeatureRecord:
    value = 1.0 - (market.yes_ask - market.yes_bid)
    value = max(0.0, min(1.0, value))
    return FeatureRecord(
        market_id=market.market_id,
        timestamp=timestamp,
        feature_name="spread_tight",
        value=value,
        metadata={"source": market.source},
    )


def compute_volume_spike_24h(
    market: MarketRecord,
    timestamp: datetime,
    baselines: dict[str, float] | None = None,
) -> FeatureRecord:
    baselines = baselines or {}
    baseline = baselines.get(market.market_id)
    if baseline is None or baseline <= 0.0:
        # No persisted 7d history yet, so fall back to current volume as a neutral baseline.
        baseline = market.volume_24h if market.volume_24h > 0.0 else 1.0
        baseline_source = "fallback_current_volume"
    else:
        baseline_source = "provided_baseline"

    value = market.volume_24h / baseline if baseline > 0.0 else 0.0
    return FeatureRecord(
        market_id=market.market_id,
        timestamp=timestamp,
        feature_name="volume_spike_24h",
        value=value,
        metadata={"source": market.source, "baseline": baseline, "baseline_source": baseline_source},
    )


def compute_price_deviation_vwap_1h(
    market: MarketRecord,
    trades: list[TradeRecord],
    timestamp: datetime,
) -> FeatureRecord:
    one_hour_ago = timestamp - timedelta(hours=1)
    recent_trades = [
        trade for trade in trades if trade.market_id == market.market_id and trade.timestamp >= one_hour_ago
    ]

    total_size = sum(trade.size for trade in recent_trades)
    if total_size > 0.0:
        vwap = sum(trade.price * trade.size for trade in recent_trades) / total_size
        value = (market.probability - vwap) / market.probability if market.probability > 0.0 else 0.0
        metadata = {"source": market.source, "vwap_1h": vwap, "trade_count": len(recent_trades)}
    else:
        value = 0.0
        metadata = {"source": market.source, "vwap_1h": market.probability, "trade_count": 0}

    return FeatureRecord(
        market_id=market.market_id,
        timestamp=timestamp,
        feature_name="price_deviation_vwap_1h",
        value=value,
        metadata=metadata,
    )


def compute_mvp_features(
    markets: list[MarketRecord],
    trades: list[TradeRecord],
    now: datetime | None = None,
    baselines: dict[str, float] | None = None,
) -> list[FeatureRecord]:
    timestamp = now or datetime.now(timezone.utc)
    trades_by_market: dict[str, list[TradeRecord]] = defaultdict(list)
    for trade in trades:
        trades_by_market[trade.market_id].append(trade)

    features: list[FeatureRecord] = []
    for market in markets:
        features.append(compute_spread_tight(market, timestamp))
        features.append(compute_volume_spike_24h(market, timestamp, baselines=baselines))
        features.append(
            compute_price_deviation_vwap_1h(
                market,
                trades_by_market.get(market.market_id, []),
                timestamp,
            )
        )
    return features
