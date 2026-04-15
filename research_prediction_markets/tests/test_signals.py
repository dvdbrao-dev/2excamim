from __future__ import annotations

from research_prediction_markets.schemas.records import FeatureRecord
from research_prediction_markets.signals.mvp_signals import generate_mvp_signals


def test_generate_mvp_signals_is_deterministic_for_fixed_inputs(
    sample_market,
    sample_features,
    fixed_now,
) -> None:
    first = generate_mvp_signals([sample_market], sample_features, now=fixed_now)
    second = generate_mvp_signals([sample_market], sample_features, now=fixed_now)

    assert first == second
    assert [signal.signal_name for signal in first] == ["tight_spread_momentum"]


def test_generate_mvp_signals_skips_markets_with_missing_features(sample_market, fixed_now) -> None:
    partial_features = [
        FeatureRecord(
            market_id="market-1",
            timestamp=fixed_now,
            feature_name="spread_tight",
            value=0.99,
            metadata={"source": "kalshi"},
        )
    ]

    assert generate_mvp_signals([sample_market], partial_features, now=fixed_now) == []


def test_generate_mvp_signals_respects_resolution_time_guardrail(
    sample_market,
    fixed_now,
) -> None:
    sample_market.resolution_time = fixed_now.replace(hour=15)
    features = [
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
            value=1.2,
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

    signals = generate_mvp_signals([sample_market], features, now=fixed_now)

    assert [signal.signal_name for signal in signals] == ["vwap_reversion"]


def test_generate_mvp_signals_uses_probability_and_deviation_for_direction(
    sample_market,
    fixed_now,
) -> None:
    sample_market.probability = 0.65
    features = [
        FeatureRecord(
            market_id="market-1",
            timestamp=fixed_now,
            feature_name="spread_tight",
            value=0.97,
            metadata={"source": "kalshi"},
        ),
        FeatureRecord(
            market_id="market-1",
            timestamp=fixed_now,
            feature_name="volume_spike_24h",
            value=1.2,
            metadata={"source": "kalshi"},
        ),
        FeatureRecord(
            market_id="market-1",
            timestamp=fixed_now,
            feature_name="price_deviation_vwap_1h",
            value=-0.08,
            metadata={"source": "kalshi"},
        ),
    ]

    signals = generate_mvp_signals([sample_market], features, now=fixed_now)

    assert len(signals) == 1
    assert signals[0].signal_name == "vwap_reversion"
    assert signals[0].direction == "long_yes"
