from __future__ import annotations

import json
from argparse import Namespace
from datetime import datetime, timezone
from pathlib import Path

from agents.market_slot_discovery_candidate import (
    build_slot_event,
    canonical_candidate_slug,
    floor_to_window,
    make_slots,
    run,
)
from agents.core.event_store import load_jsonl


def test_floor_to_window_5m_rounding() -> None:
    ts = datetime(2026, 5, 7, 17, 13, 41, tzinfo=timezone.utc)
    floored = floor_to_window(ts, 5)
    assert floored.isoformat() == "2026-05-07T17:10:00+00:00"


def test_floor_to_window_15m_rounding() -> None:
    ts = datetime(2026, 5, 7, 17, 29, 10, tzinfo=timezone.utc)
    floored = floor_to_window(ts, 15)
    assert floored.isoformat() == "2026-05-07T17:15:00+00:00"


def test_idempotency_key_stability() -> None:
    now_ts = datetime(2026, 5, 7, 17, 13, 0, tzinfo=timezone.utc)
    slot = make_slots(now_ts, "BTC", "5m", 0)[0]
    event_a = build_slot_event(slot, now_ts)
    event_b = build_slot_event(slot, now_ts)
    assert event_a["idempotency_key"] == event_b["idempotency_key"]


def test_canonical_btc_5m_slug() -> None:
    now_ts = datetime(2026, 5, 10, 10, 15, 0, tzinfo=timezone.utc)
    slot = make_slots(now_ts, "BTC", "5m", 0)[0]
    slug, unix_ts, confidence, method = canonical_candidate_slug(slot)
    assert slug == "btc-updown-5m-1778408100"
    assert unix_ts == 1778408100
    assert confidence > 0.35
    assert method == "canonical_updown_unix_v1"


def test_canonical_eth_15m_slug() -> None:
    now_ts = datetime(2026, 5, 10, 10, 15, 0, tzinfo=timezone.utc)
    slot = make_slots(now_ts, "ETH", "15m", 0)[0]
    slug, unix_ts, _, _ = canonical_candidate_slug(slot)
    assert slug == "eth-updown-15m-1778408100"
    assert unix_ts == 1778408100


def test_canonical_sol_5m_slug() -> None:
    now_ts = datetime(2026, 5, 10, 10, 15, 0, tzinfo=timezone.utc)
    slot = make_slots(now_ts, "SOL", "5m", 0)[0]
    slug, unix_ts, _, _ = canonical_candidate_slug(slot)
    assert slug == "sol-updown-5m-1778408100"
    assert unix_ts == 1778408100


def test_jsonl_event_validity(tmp_path: Path) -> None:
    output = tmp_path / "external_candidates.jsonl"
    args = Namespace(
        assets="BTC",
        windows="5m",
        now="2026-05-07T17:13:00Z",
        lookahead_slots=0,
        output_jsonl=str(output),
        dry_run=False,
        confirm_slugs=False,
        slug_mode="canonical",
    )
    summary = run(args)
    assert summary["persisted_events"] == 1

    records = load_jsonl(output)
    assert len(records) == 1
    record = records[0]
    assert record["event_type"] == "market_slot.discovered"
    assert isinstance(record["payload"], dict)
    assert record["payload"]["asset"] == "BTC"
    assert record["payload"]["slug_family"] == "canonical_updown_unix_v1"
    assert record["payload"]["legacy_candidate_slug"] is not None
    assert "updown-5m-" in record["payload"]["candidate_slug"]

    raw = output.read_text(encoding="utf-8").strip()
    assert json.loads(raw)["event_type"] == "market_slot.discovered"


def test_dry_run_does_not_write_file(tmp_path: Path) -> None:
    output = tmp_path / "external_candidates.jsonl"
    args = Namespace(
        assets="BTC",
        windows="5m",
        now="2026-05-07T17:13:00Z",
        lookahead_slots=2,
        output_jsonl=str(output),
        dry_run=True,
        confirm_slugs=False,
        slug_mode="canonical",
    )
    summary = run(args)
    assert summary["generated_events"] == 3
    assert summary["persisted_events"] == 3
    assert not output.exists()


def test_legacy_slug_mode_supported() -> None:
    now_ts = datetime(2026, 5, 10, 10, 15, 0, tzinfo=timezone.utc)
    slot = make_slots(now_ts, "BTC", "5m", 0)[0]
    event = build_slot_event(slot, now_ts, slug_mode="legacy")
    payload = event["payload"]
    assert payload["candidate_slug"].startswith("btc-up-or-down-")
    assert payload["slug_family"] == "deterministic_slug_heuristic_v1"
    assert payload["legacy_candidate_slug"] is None
