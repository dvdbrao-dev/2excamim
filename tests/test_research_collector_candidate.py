from __future__ import annotations

import json
from argparse import Namespace
from collections import deque
from pathlib import Path

from agents.core.event_store import load_jsonl
from agents.research_collector_candidate import oracle_spot_delta_bps, run, spot_delta_bps


def _write_slots(path: Path) -> None:
    event = {
        "event_type": "market_slot.discovered",
        "event_id": "00000000-0000-0000-0000-000000000001",
        "timestamp": "2026-05-07T18:00:00Z",
        "idempotency_key": "market_slot.discovered:v1:test",
        "aggregate_key": "polymarket:btc:5m",
        "provenance": {"agent_id": "test", "source": "test", "generated_at": "2026-05-07T18:00:00Z"},
        "payload": {
            "asset": "BTC",
            "window": "5m",
            "slot_start": "2026-05-07T18:00:00Z",
            "slot_end": "2026-05-07T18:05:00Z",
            "candidate_slug": "btc-updown-5m-1778176800",
        },
    }
    path.write_text(json.dumps(event) + "\n", encoding="utf-8")


def test_feature_helpers_calculate_correctly() -> None:
    assert round((spot_delta_bps(deque([100.0, 101.0])) or 0.0), 2) == 100.0
    assert round(oracle_spot_delta_bps(101.0, 100.0) or 0.0, 2) == 100.0


def test_mock_collector_emits_valid_jsonl(tmp_path: Path) -> None:
    slots = tmp_path / "slots.jsonl"
    out = tmp_path / "external_candidates.jsonl"
    _write_slots(slots)

    args = Namespace(
        input_slots_jsonl=str(slots),
        output_jsonl=str(out),
        assets="BTC",
        windows="5m",
        sample_count=2,
        sample_interval_ms=1000,
        dry_run=False,
        mock=False,
        data_mode="mock",
        network_timeout_sec=5.0,
        max_retries=2,
        fail_soft=False,
        polymarket_metadata_enabled=False,
        polymarket_orderbook_enabled=False,
    )
    summary = run(args)
    assert summary["events_persisted"] >= 2

    rows = load_jsonl(out)
    snapshots = [row for row in rows if row.get("event_type") == "market_snapshot.observed"]
    health = [row for row in rows if row.get("event_type") == "feed_health.checked"][-1]["payload"]
    assert snapshots
    assert snapshots[0]["payload"]["source_quality"] == "mock"
    assert health["ok"] is True
    assert health["partial"] is False


def test_missing_data_emits_data_gap_detected(tmp_path: Path) -> None:
    out = tmp_path / "external_candidates.jsonl"
    args = Namespace(
        input_slots_jsonl=str(tmp_path / "missing-slots.jsonl"),
        output_jsonl=str(out),
        assets="BTC",
        windows="5m",
        sample_count=1,
        sample_interval_ms=1000,
        dry_run=False,
        mock=False,
        data_mode="read_only",
        network_timeout_sec=0.01,
        max_retries=0,
        fail_soft=True,
        polymarket_metadata_enabled=True,
        polymarket_orderbook_enabled=False,
    )
    run(args)
    rows = load_jsonl(out)
    assert any(row.get("event_type") == "data_gap.detected" for row in rows)


def test_event_idempotency_stable(tmp_path: Path) -> None:
    slots = tmp_path / "slots.jsonl"
    out = tmp_path / "external_candidates.jsonl"
    _write_slots(slots)

    args = Namespace(
        input_slots_jsonl=str(slots),
        output_jsonl=str(out),
        assets="BTC",
        windows="5m",
        sample_count=1,
        sample_interval_ms=1000,
        dry_run=False,
        mock=False,
        data_mode="mock",
        network_timeout_sec=5.0,
        max_retries=2,
        fail_soft=False,
        polymarket_metadata_enabled=False,
        polymarket_orderbook_enabled=False,
    )
    run(args)
    first = load_jsonl(out)
    run(args)
    second = load_jsonl(out)
    assert len(first) == len(second)


def test_dry_run_behavior(tmp_path: Path) -> None:
    slots = tmp_path / "slots.jsonl"
    out = tmp_path / "external_candidates.jsonl"
    _write_slots(slots)

    args = Namespace(
        input_slots_jsonl=str(slots),
        output_jsonl=str(out),
        assets="BTC",
        windows="5m",
        sample_count=1,
        sample_interval_ms=1000,
        dry_run=True,
        mock=False,
        data_mode="mock",
        network_timeout_sec=5.0,
        max_retries=2,
        fail_soft=False,
        polymarket_metadata_enabled=False,
        polymarket_orderbook_enabled=False,
    )
    summary = run(args)
    assert summary["events_generated"] >= 1
    assert not out.exists()


def test_read_only_with_orderbook_fields(monkeypatch, tmp_path: Path) -> None:
    slots = tmp_path / "slots.jsonl"
    out = tmp_path / "external_candidates.jsonl"
    _write_slots(slots)

    from agents.adapters.binance_spot_adapter import SpotPriceResult
    from agents.adapters.polymarket_metadata_adapter import MetadataResult
    from agents.adapters.polymarket_orderbook_adapter import OrderBookResult

    monkeypatch.setattr(
        "agents.adapters.binance_spot_adapter.BinanceSpotAdapter.observe",
        lambda self, asset: SpotPriceResult(100000.0, "BTCUSDT", 5, None),
    )
    monkeypatch.setattr(
        "agents.adapters.polymarket_metadata_adapter.PolymarketMetadataAdapter.observe",
        lambda self, slug: MetadataResult(
            market_id="m1",
            condition_id="c1",
            question="BTC up/down",
            active=True,
            closed=False,
            resolved=False,
            outcome_tokens=[{"outcome": "YES", "token_id": "10"}],
            end_date="2026-05-07T18:05:00Z",
            resolution_date=None,
            raw_source_summary="test",
            match_confidence=1.0,
            match_reason="exact_canonical_slug",
            latency_ms=5,
            error=None,
            adapter_errors={},
        ),
    )
    monkeypatch.setattr(
        "agents.adapters.polymarket_orderbook_adapter.PolymarketOrderBookAdapter.observe",
        lambda self, outcome_tokens: [
            OrderBookResult(
                token_id="10",
                outcome="YES",
                best_bid=0.48,
                best_ask=0.50,
                mid_price=0.49,
                spread_bps=408.16,
                depth_top_n={"n": 2.0, "bid": 1000.0, "ask": 900.0},
                imbalance_top_n=0.0526,
                raw_levels_summary={"bids": 1, "asks": 1},
                source_quality="read_only_orderbook",
                latency_ms=5,
                error=None,
            )
        ],
    )

    args = Namespace(
        input_slots_jsonl=str(slots),
        output_jsonl=str(out),
        assets="BTC",
        windows="5m",
        sample_count=1,
        sample_interval_ms=1,
        dry_run=False,
        mock=False,
        data_mode="read_only",
        network_timeout_sec=1.0,
        max_retries=0,
        fail_soft=True,
        polymarket_metadata_enabled=True,
        polymarket_orderbook_enabled=True,
    )
    run(args)
    rows = load_jsonl(out)
    snap = [r for r in rows if r.get("event_type") == "market_snapshot.observed"][-1]["payload"]
    assert snap["source_quality"] == "read_only_orderbook"
    assert snap["orderbook"]["best_bid"] == 0.48
    assert snap["orderbook"]["books"][0]["token_id"] == "10"


def test_read_only_orderbook_missing_emits_data_gap(monkeypatch, tmp_path: Path) -> None:
    slots = tmp_path / "slots.jsonl"
    out = tmp_path / "external_candidates.jsonl"
    _write_slots(slots)

    from agents.adapters.binance_spot_adapter import SpotPriceResult
    from agents.adapters.polymarket_metadata_adapter import MetadataResult
    from agents.adapters.polymarket_orderbook_adapter import OrderBookResult

    monkeypatch.setattr(
        "agents.adapters.binance_spot_adapter.BinanceSpotAdapter.observe",
        lambda self, asset: SpotPriceResult(100000.0, "BTCUSDT", 5, None),
    )
    monkeypatch.setattr(
        "agents.adapters.polymarket_metadata_adapter.PolymarketMetadataAdapter.observe",
        lambda self, slug: MetadataResult(
            market_id="m1",
            condition_id="c1",
            question="BTC up/down",
            active=True,
            closed=False,
            resolved=False,
            outcome_tokens=[{"outcome": "YES", "token_id": "10"}],
            end_date="2026-05-07T18:05:00Z",
            resolution_date=None,
            raw_source_summary="test",
            match_confidence=1.0,
            match_reason="exact_canonical_slug",
            latency_ms=5,
            error=None,
            adapter_errors={},
        ),
    )
    monkeypatch.setattr(
        "agents.adapters.polymarket_orderbook_adapter.PolymarketOrderBookAdapter.observe",
        lambda self, outcome_tokens: [
            OrderBookResult(
                token_id="10",
                outcome="YES",
                best_bid=None,
                best_ask=None,
                mid_price=None,
                spread_bps=None,
                depth_top_n=None,
                imbalance_top_n=None,
                raw_levels_summary={},
                source_quality="missing",
                latency_ms=5,
                error="http_404",
            )
        ],
    )

    args = Namespace(
        input_slots_jsonl=str(slots),
        output_jsonl=str(out),
        assets="BTC",
        windows="5m",
        sample_count=1,
        sample_interval_ms=1,
        dry_run=False,
        mock=False,
        data_mode="read_only",
        network_timeout_sec=1.0,
        max_retries=0,
        fail_soft=True,
        polymarket_metadata_enabled=True,
        polymarket_orderbook_enabled=True,
    )
    run(args)
    rows = load_jsonl(out)
    gaps = [r for r in rows if r.get("event_type") == "data_gap.detected"]
    assert any(g["payload"].get("gap_type") == "missing_polymarket_orderbook" for g in gaps)
