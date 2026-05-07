from __future__ import annotations

import json
from argparse import Namespace
from pathlib import Path

from agents.core.event_store import load_jsonl
from agents.research_collector_candidate import run


def _write_slots(path: Path) -> None:
    rows = []
    for idx, asset in enumerate(("BTC", "ETH", "SOL"), start=1):
        rows.append(
            {
                "event_type": "market_slot.discovered",
                "event_id": f"00000000-0000-0000-0000-00000000010{idx}",
                "timestamp": "2026-05-07T18:00:00Z",
                "idempotency_key": f"market_slot.discovered:v1:test2:{asset}",
                "aggregate_key": f"polymarket:{asset.lower()}:5m",
                "provenance": {"agent_id": "test", "source": "test", "generated_at": "2026-05-07T18:00:00Z"},
                "payload": {
                    "asset": asset,
                    "window": "5m",
                    "slot_start": "2026-05-07T18:00:00Z",
                    "slot_end": "2026-05-07T18:05:00Z",
                    "candidate_slug": f"{asset.lower()}-up-or-down-may-07-1800-utc-5m",
                },
            }
        )
    path.write_text("\n".join(json.dumps(r) for r in rows) + "\n", encoding="utf-8")


def _args(slots: Path, out: Path) -> Namespace:
    return Namespace(
        input_slots_jsonl=str(slots),
        output_jsonl=str(out),
        assets="BTC,ETH,SOL",
        windows="5m",
        sample_count=1,
        sample_interval_ms=1,
        dry_run=False,
        mock=False,
        data_mode="read_only",
        network_timeout_sec=0.001,
        max_retries=0,
        fail_soft=True,
        polymarket_metadata_enabled=False,
        polymarket_orderbook_enabled=False,
    )


def test_all_spot_failures_emit_health_errors(monkeypatch, tmp_path: Path) -> None:
    slots = tmp_path / "slots.jsonl"
    out = tmp_path / "events.jsonl"
    _write_slots(slots)

    def _fail(self, asset: str):
        from agents.adapters.binance_spot_adapter import SpotPriceResult

        return SpotPriceResult(None, f"{asset}USDT", 10, "timeout after 5s")

    monkeypatch.setattr("agents.adapters.binance_spot_adapter.BinanceSpotAdapter.observe", _fail)
    run(_args(slots, out))

    rows = load_jsonl(out)
    health = [r for r in rows if r.get("event_type") == "feed_health.checked"][-1]["payload"]
    assert health["ok"] is False
    assert health["partial"] is False
    assert health["failed_assets"] == ["BTC", "ETH", "SOL"]
    assert health["adapter_errors"]["binance_spot"]["BTC"] == "timeout after 5s"


def test_partial_spot_success_sets_partial(monkeypatch, tmp_path: Path) -> None:
    slots = tmp_path / "slots.jsonl"
    out = tmp_path / "events.jsonl"
    _write_slots(slots)

    def _mixed(self, asset: str):
        from agents.adapters.binance_spot_adapter import SpotPriceResult

        if asset == "BTC":
            return SpotPriceResult(100000.0, "BTCUSDT", 10, None)
        return SpotPriceResult(None, f"{asset}USDT", 10, f"http 451 restricted:{asset}")

    monkeypatch.setattr("agents.adapters.binance_spot_adapter.BinanceSpotAdapter.observe", _mixed)
    run(_args(slots, out))

    rows = load_jsonl(out)
    health = [r for r in rows if r.get("event_type") == "feed_health.checked"][-1]["payload"]
    assert health["ok"] is False
    assert health["partial"] is True
    assert health["successful_assets"] == ["BTC"]
    assert sorted(health["failed_assets"]) == ["ETH", "SOL"]


def test_spot_success_emits_market_snapshot(monkeypatch, tmp_path: Path) -> None:
    slots = tmp_path / "slots.jsonl"
    out = tmp_path / "events.jsonl"
    _write_slots(slots)

    def _ok(self, asset: str):
        from agents.adapters.binance_spot_adapter import SpotPriceResult

        base = {"BTC": 100000.0, "ETH": 2500.0, "SOL": 160.0}
        return SpotPriceResult(base[asset], f"{asset}USDT", 10, None)

    monkeypatch.setattr("agents.adapters.binance_spot_adapter.BinanceSpotAdapter.observe", _ok)
    run(_args(slots, out))

    rows = load_jsonl(out)
    assert any(r.get("event_type") == "market_snapshot.observed" for r in rows)


def test_spot_missing_emits_data_gap(monkeypatch, tmp_path: Path) -> None:
    slots = tmp_path / "slots.jsonl"
    out = tmp_path / "events.jsonl"
    _write_slots(slots)

    def _none(self, asset: str):
        from agents.adapters.binance_spot_adapter import SpotPriceResult

        return SpotPriceResult(None, f"{asset}USDT", 10, "dns resolution failed")

    monkeypatch.setattr("agents.adapters.binance_spot_adapter.BinanceSpotAdapter.observe", _none)
    run(_args(slots, out))

    rows = load_jsonl(out)
    gaps = [r for r in rows if r.get("event_type") == "data_gap.detected"]
    assert any(g["payload"].get("gap_type") == "missing_spot_price" for g in gaps)
