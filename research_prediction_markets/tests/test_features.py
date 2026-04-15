from __future__ import annotations

import pytest

from research_prediction_markets.features.mvp_features import (
    compute_mvp_features,
    compute_price_deviation_vwap_1h,
    compute_spread_tight,
    compute_volume_spike_24h,
)


def test_compute_spread_tight_clamps_to_unit_interval(sample_market, fixed_now) -> None:
    market = sample_market
    market.yes_bid = 0.10
    market.yes_ask = 1.40

    feature = compute_spread_tight(market, fixed_now)

    assert feature.feature_name == "spread_tight"
    assert feature.value == 0.0


def test_compute_volume_spike_uses_provided_baseline(sample_market, fixed_now) -> None:
    feature = compute_volume_spike_24h(
        sample_market,
        fixed_now,
        baselines={"market-1": 120.0},
    )

    assert feature.feature_name == "volume_spike_24h"
    assert feature.value == 2.5
    assert feature.metadata["baseline"] == 120.0
    assert feature.metadata["baseline_source"] == "provided_baseline"


def test_compute_volume_spike_falls_back_to_current_volume(sample_market, fixed_now) -> None:
    feature = compute_volume_spike_24h(sample_market, fixed_now, baselines={"market-1": 0.0})

    assert feature.value == 1.0
    assert feature.metadata["baseline"] == sample_market.volume_24h
    assert feature.metadata["baseline_source"] == "fallback_current_volume"


def test_compute_price_deviation_vwap_1h_uses_recent_trades_only(sample_market, sample_trades, fixed_now) -> None:
    old_trade = sample_trades[0]
    old_trade.timestamp = fixed_now.replace(hour=9)

    feature = compute_price_deviation_vwap_1h(sample_market, sample_trades, fixed_now)

    assert feature.feature_name == "price_deviation_vwap_1h"
    assert feature.metadata["trade_count"] == 1
    assert feature.metadata["vwap_1h"] == 0.45
    assert feature.value == pytest.approx((0.40 - 0.45) / 0.40)


def test_compute_mvp_features_returns_all_expected_features(sample_market, sample_trades, fixed_now) -> None:
    features = compute_mvp_features(
        [sample_market],
        sample_trades,
        now=fixed_now,
        baselines={"market-1": 120.0},
    )

    assert [feature.feature_name for feature in features] == [
        "spread_tight",
        "volume_spike_24h",
        "price_deviation_vwap_1h",
    ]
    assert all(feature.timestamp == fixed_now for feature in features)
