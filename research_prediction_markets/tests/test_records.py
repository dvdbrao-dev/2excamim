from __future__ import annotations

from datetime import datetime, timezone

from research_prediction_markets.schemas.records import (
    normalize_amount,
    normalize_probability,
    parse_datetime,
)


def test_normalize_probability_handles_percent_strings_and_bounds() -> None:
    assert normalize_probability("75%") == 0.75
    assert normalize_probability(" 150 ") == 1.0
    assert normalize_probability(-5) == 0.0
    assert normalize_probability(0.42) == 0.42


def test_normalize_probability_falls_back_for_missing_or_invalid_values() -> None:
    assert normalize_probability(None, default=0.25) == 0.25
    assert normalize_probability("", default=0.25) == 0.25
    assert normalize_probability("not-a-number", default=0.25) == 0.25


def test_normalize_amount_handles_invalid_values() -> None:
    assert normalize_amount("12.5") == 12.5
    assert normalize_amount(None, default=7.0) == 7.0
    assert normalize_amount("bad", default=7.0) == 7.0


def test_parse_datetime_normalizes_multiple_input_formats_to_utc() -> None:
    expected = datetime(2026, 4, 14, 12, 0, tzinfo=timezone.utc)

    assert parse_datetime("2026-04-14T12:00:00Z") == expected
    assert parse_datetime("2026-04-14T14:00:00+02:00") == expected
    assert parse_datetime("2026-04-14T12:00:00") == expected
    assert parse_datetime(1_776_168_000) == expected
    assert parse_datetime(1_776_168_000_000) == expected


def test_parse_datetime_returns_none_for_blank_and_invalid_values() -> None:
    assert parse_datetime(None) is None
    assert parse_datetime("") is None
    assert parse_datetime("not-a-date") is None
