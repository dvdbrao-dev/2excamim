from __future__ import annotations

import math
from datetime import datetime, timezone

import pandas as pd

from research_prediction_markets.export_signals_json import normalize_value
from research_prediction_markets.main import OUTPUT_COLUMNS, assemble_signals_dataframe


def test_assemble_signals_dataframe_builds_expected_rows(
    sample_market,
    sample_trades,
    sample_features,
    fixed_now,
    monkeypatch,
) -> None:
    monkeypatch.setattr(
        "research_prediction_markets.main.compute_mvp_features",
        lambda markets, trades: sample_features,
    )

    dataframe = assemble_signals_dataframe([sample_market], sample_trades, now=fixed_now)

    assert list(dataframe.columns) == OUTPUT_COLUMNS
    assert len(dataframe) == 1
    assert set(dataframe["signal_name"]) == {"tight_spread_momentum"}
    assert set(dataframe["source"]) == {"kalshi"}
    assert all(value == sample_market.probability for value in dataframe["probability"])
    assert all(isinstance(value, str) for value in dataframe["metadata"])


def test_assemble_signals_dataframe_ignores_orphan_signals(sample_market, sample_trades, fixed_now, monkeypatch) -> None:
    def fake_generate(_markets, _features, now=None):
        from research_prediction_markets.schemas.records import SignalCandidateRecord

        return [
            SignalCandidateRecord(
                market_id="missing-market",
                timestamp=now,
                signal_name="tight_spread_momentum",
                strength=0.9,
                direction="long_yes",
                metadata={"source": "kalshi"},
            )
        ]

    monkeypatch.setattr("research_prediction_markets.main.generate_mvp_signals", fake_generate)

    dataframe = assemble_signals_dataframe([sample_market], sample_trades, now=fixed_now)

    assert dataframe.empty
    assert list(dataframe.columns) == OUTPUT_COLUMNS


def test_normalize_value_handles_nullish_datetime_and_non_finite_values() -> None:
    timestamp = datetime(2026, 4, 14, 12, 0, tzinfo=timezone.utc)

    assert normalize_value(timestamp) == "2026-04-14T12:00:00Z"
    assert normalize_value(pd.NA) is None
    assert normalize_value(float("nan")) is None
    assert normalize_value(float("inf")) is None
    assert normalize_value(math.pi) == math.pi
