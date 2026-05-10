#!/usr/bin/env python3
"""Market slot discovery candidate (shadow/research only).

Discovers deterministic UTC crypto market slots and emits candidate
`market_slot.discovered` events to a dedicated JSONL stream.
"""

from __future__ import annotations

import argparse
import json
from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any

try:
    from core.event_envelope import append_event_jsonl, build_aggregate_key, build_event, build_provenance
    from core.logging import emit_json
except ModuleNotFoundError:  # pragma: no cover - fallback for package-style imports in tests
    from agents.core.event_envelope import (
        append_event_jsonl,
        build_aggregate_key,
        build_event,
        build_provenance,
    )
    from agents.core.logging import emit_json


AGENT_ID = "slot-discovery-candidate-v1"
SOURCE = "slot_discovery_candidate"
DEFAULT_OUTPUT = Path("./var/events/external_candidates.jsonl")
SUPPORTED_WINDOWS = {"5m": 5, "15m": 15}
SUPPORTED_ASSETS = {"BTC", "ETH", "SOL"}
CANONICAL_FAMILY = "canonical_updown_unix_v1"
LEGACY_FAMILY = "deterministic_slug_heuristic_v1"


@dataclass(frozen=True)
class SlotWindow:
    asset: str
    window: str
    slot_start: datetime
    slot_end: datetime


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Discover deterministic crypto market slots (candidate only).")
    parser.add_argument("--assets", default="BTC,ETH,SOL", help="Comma-separated assets.")
    parser.add_argument("--windows", default="5m,15m", help="Comma-separated windows.")
    parser.add_argument("--now", default="", help="Optional UTC ISO timestamp for deterministic runs.")
    parser.add_argument("--lookahead-slots", type=int, default=2, help="How many future slots to include.")
    parser.add_argument("--output-jsonl", default=str(DEFAULT_OUTPUT), help="Output JSONL file.")
    parser.add_argument("--dry-run", action="store_true", help="Validate/build events without persisting.")
    parser.add_argument(
        "--confirm-slugs",
        action="store_true",
        help="Optional best-effort confirmation mode. Never fails the process.",
    )
    parser.add_argument(
        "--slug-mode",
        choices=["canonical", "legacy"],
        default="canonical",
        help="Slug generation mode. canonical is default and recommended for read-only mode.",
    )
    return parser.parse_args()


def parse_now(value: str) -> datetime:
    if value.strip():
        parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
        return parsed.astimezone(timezone.utc)
    return datetime.now(timezone.utc)


def parse_csv(raw: str) -> list[str]:
    return [part.strip() for part in raw.split(",") if part.strip()]


def floor_to_window(ts: datetime, window_minutes: int) -> datetime:
    floored_minute = (ts.minute // window_minutes) * window_minutes
    return ts.replace(minute=floored_minute, second=0, microsecond=0)


def make_slots(now: datetime, asset: str, window: str, lookahead_slots: int) -> list[SlotWindow]:
    minutes = SUPPORTED_WINDOWS[window]
    base = floor_to_window(now, minutes)
    slots: list[SlotWindow] = []
    for offset in range(lookahead_slots + 1):
        start = base + timedelta(minutes=offset * minutes)
        end = start + timedelta(minutes=minutes)
        slots.append(SlotWindow(asset=asset, window=window, slot_start=start, slot_end=end))
    return slots


def month_token(ts: datetime) -> str:
    return ts.strftime("%b").lower()


def _legacy_candidate_slug(slot: SlotWindow) -> tuple[str, float, str]:
    start = slot.slot_start
    # Heuristic only: intentionally conservative confidence.
    slug = (
        f"{slot.asset.lower()}-up-or-down-"
        f"{month_token(start)}-{start.day:02d}-{start:%H%M}-utc-{slot.window}"
    )
    return slug, 0.35, LEGACY_FAMILY


def canonical_candidate_slug(slot: SlotWindow) -> tuple[str, int, float, str]:
    unix_slot_start = int(slot.slot_start.timestamp())
    slug = f"{slot.asset.lower()}-updown-{slot.window}-{unix_slot_start}"
    return slug, unix_slot_start, 0.9, CANONICAL_FAMILY


def build_slot_event(slot: SlotWindow, now_ts: datetime, slug_mode: str = "canonical") -> dict[str, Any]:
    legacy_slug, _, _ = _legacy_candidate_slug(slot)
    if slug_mode == "legacy":
        slug = legacy_slug
        confidence = 0.35
        method = LEGACY_FAMILY
        slug_family = LEGACY_FAMILY
        slug_timestamp = None
        legacy_candidate_slug = None
    else:
        slug, slug_timestamp, confidence, method = canonical_candidate_slug(slot)
        slug_family = CANONICAL_FAMILY
        legacy_candidate_slug = legacy_slug
    aggregate = build_aggregate_key("polymarket", slot.asset.lower(), slot.window)
    payload = {
        "asset": slot.asset,
        "window": slot.window,
        "slot_start": slot.slot_start.isoformat().replace("+00:00", "Z"),
        "slot_end": slot.slot_end.isoformat().replace("+00:00", "Z"),
        "candidate_slug": slug,
        "legacy_candidate_slug": legacy_candidate_slug,
        "slug_family": slug_family,
        "slug_timestamp": slug_timestamp,
        "confidence": confidence,
        "discovery_method": method,
        "confirmed": False,
        "source": SOURCE,
        "slot_anchor_now": now_ts.isoformat().replace("+00:00", "Z"),
    }
    return build_event(
        event_type="market_slot.discovered",
        aggregate_key=aggregate,
        payload=payload,
        provenance=build_provenance(AGENT_ID, "shadow-research", notes="candidate only; no trading"),
        unique_components=[slot.asset, slot.window, payload["slot_start"], payload["candidate_slug"], method],
        timestamp=payload["slot_start"],
    )


def build_confirmation_failure_events(reason: str, now_ts: datetime) -> list[dict[str, Any]]:
    ts = now_ts.isoformat().replace("+00:00", "Z")
    health = build_event(
        event_type="feed_health.checked",
        aggregate_key="feed:polymarket:slug-confirmation",
        payload={
            "ok": False,
            "confirmed": False,
            "source": SOURCE,
            "reason": reason,
        },
        provenance=build_provenance(AGENT_ID, "shadow-research"),
        unique_components=["polymarket", "slug-confirmation", reason, ts],
        timestamp=ts,
    )
    gap = build_event(
        event_type="data_gap.detected",
        aggregate_key="feed:polymarket:slug-confirmation",
        payload={
            "gap_type": "slug_confirmation_unavailable",
            "source": SOURCE,
            "reason": reason,
            "from": ts,
            "to": ts,
        },
        provenance=build_provenance(AGENT_ID, "shadow-research"),
        unique_components=["polymarket", "slug-confirmation-unavailable", reason, ts],
        timestamp=ts,
    )
    return [health, gap]


def run(args: argparse.Namespace) -> dict[str, Any]:
    assets = [a.upper() for a in parse_csv(args.assets)]
    windows = [w.lower() for w in parse_csv(args.windows)]
    now_ts = parse_now(args.now)

    invalid_assets = [a for a in assets if a not in SUPPORTED_ASSETS]
    invalid_windows = [w for w in windows if w not in SUPPORTED_WINDOWS]
    if invalid_assets:
        raise SystemExit(f"unsupported assets: {','.join(invalid_assets)}")
    if invalid_windows:
        raise SystemExit(f"unsupported windows: {','.join(invalid_windows)}")
    if args.lookahead_slots < 0:
        raise SystemExit("--lookahead-slots must be >= 0")

    events: list[dict[str, Any]] = []
    for asset in assets:
        for window in windows:
            for slot in make_slots(now_ts, asset, window, args.lookahead_slots):
                events.append(build_slot_event(slot, now_ts, slug_mode=args.slug_mode))

    if args.confirm_slugs:
        events.extend(
            build_confirmation_failure_events(
                "confirmation_mode_requested_but_no_safe_network_lookup_configured",
                now_ts,
            )
        )

    output_path = Path(args.output_jsonl)
    persisted = 0
    for event in events:
        if append_event_jsonl(output_path, event, dry_run=args.dry_run):
            persisted += 1

    return {
        "actor": AGENT_ID,
        "source": SOURCE,
        "assets": assets,
        "windows": windows,
        "lookahead_slots": args.lookahead_slots,
        "generated_events": len(events),
        "persisted_events": persisted,
        "dry_run": bool(args.dry_run),
        "output_jsonl": str(output_path),
        "confirmation_mode": bool(args.confirm_slugs),
    }


def main() -> int:
    args = parse_args()
    summary = run(args)
    emit_json(summary)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
