from __future__ import annotations

from datetime import datetime, timedelta, timezone

from research_prediction_markets.schemas.records import (
    FeatureRecord,
    MarketRecord,
    SignalCandidateRecord,
)


def _clamp_unit(value: float) -> float:
    return max(0.0, min(1.0, value))


def generate_mvp_signals(
    markets: list[MarketRecord],
    features: list[FeatureRecord],
    now: datetime | None = None,
) -> list[SignalCandidateRecord]:
    timestamp = now or datetime.now(timezone.utc)
    feature_map: dict[str, dict[str, FeatureRecord]] = {}
    for feature in features:
        feature_map.setdefault(feature.market_id, {})[feature.feature_name] = feature

    signals: list[SignalCandidateRecord] = []
    for market in markets:
        market_features = feature_map.get(market.market_id, {})
        spread_tight = market_features.get("spread_tight")
        volume_spike = market_features.get("volume_spike_24h")
        deviation = market_features.get("price_deviation_vwap_1h")
        if spread_tight is None or volume_spike is None or deviation is None:
            continue

        if (
            spread_tight.value > 0.95
            and volume_spike.value > 2.0
            and _has_enough_time_to_resolution(market, timestamp)
        ):
            direction = "long_yes" if market.probability < 0.5 else "long_no"
            signals.append(
                SignalCandidateRecord(
                    market_id=market.market_id,
                    timestamp=timestamp,
                    signal_name="tight_spread_momentum",
                    strength=_clamp_unit(volume_spike.value * spread_tight.value),
                    direction=direction,
                    metadata={
                        "source": market.source,
                        "probability": market.probability,
                        "spread_tight": spread_tight.value,
                        "volume_spike_24h": volume_spike.value,
                    },
                )
            )

        if abs(deviation.value) > 0.03 and volume_spike.value < 1.5:
            direction = "short_yes" if deviation.value > 0.0 else "long_yes"
            signals.append(
                SignalCandidateRecord(
                    market_id=market.market_id,
                    timestamp=timestamp,
                    signal_name="vwap_reversion",
                    strength=_clamp_unit(abs(deviation.value)),
                    direction=direction,
                    metadata={
                        "source": market.source,
                        "probability": market.probability,
                        "price_deviation_vwap_1h": deviation.value,
                        "volume_spike_24h": volume_spike.value,
                    },
                )
            )

    return signals


def _has_enough_time_to_resolution(market: MarketRecord, now: datetime) -> bool:
    if market.resolution_time is None:
        return True
    return market.resolution_time - now > timedelta(hours=4)
