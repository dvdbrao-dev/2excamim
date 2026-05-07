#!/usr/bin/env python3
"""Research-only cross-venue market matching skeleton (Polymarket/Kalshi)."""

from __future__ import annotations

import argparse
import json
import re
from dataclasses import dataclass
from difflib import SequenceMatcher
from pathlib import Path
from typing import Any

try:
    from core.event_envelope import append_event_jsonl, build_event, build_provenance
    from core.logging import emit_json
except ModuleNotFoundError:  # pragma: no cover
    from agents.core.event_envelope import append_event_jsonl, build_event, build_provenance
    from agents.core.logging import emit_json


AGENT_ID = "cross-venue-matcher-candidate-v1"
SOURCE = "cross_venue_matcher_candidate"
DEFAULT_OUTPUT = Path("./var/events/cross_venue_matches.jsonl")
ASSETS = ("BTC", "ETH", "SOL")


@dataclass(frozen=True)
class MarketFeatures:
    market_id: str
    title: str
    normalized_title: str
    asset: str | None
    window: str | None
    strike: float | None
    settlement_hint: str | None


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Research-only cross-venue market matching skeleton.")
    parser.add_argument("--polymarket-markets-json", required=True)
    parser.add_argument("--kalshi-markets-json", required=True)
    parser.add_argument("--output-jsonl", default=str(DEFAULT_OUTPUT))
    parser.add_argument("--min-confidence", type=float, default=0.90)
    parser.add_argument("--dry-run", action="store_true")
    return parser.parse_args()


def normalize_title(title: str) -> str:
    base = title.lower()
    base = re.sub(r"[^a-z0-9\s\-:\./]", " ", base)
    base = re.sub(r"\s+", " ", base).strip()
    return base


def extract_asset_symbol(text: str) -> str | None:
    upper = text.upper()
    for asset in ASSETS:
        if re.search(rf"\b{asset}\b", upper):
            return asset
    return None


def extract_window(text: str) -> str | None:
    normalized = text.lower()
    if "15m" in normalized or "15 min" in normalized or "15 minute" in normalized:
        return "15m"
    if "5m" in normalized or "5 min" in normalized or "5 minute" in normalized:
        return "5m"
    if "hour" in normalized or "hourly" in normalized or "1h" in normalized:
        return "1h"
    if "daily" in normalized or "day" in normalized or "24h" in normalized:
        return "1d"
    return None


def extract_strike(text: str) -> float | None:
    lowered = text.lower()
    patterns = [
        r"(?:above|over|below|under|at least|at most)\s*\$?([0-9]+(?:\.[0-9]+)?)",
        r"\$([0-9]+(?:\.[0-9]+)?)",
    ]
    for pat in patterns:
        match = re.search(pat, lowered)
        if match:
            try:
                return float(match.group(1))
            except ValueError:
                return None
    return None


def extract_settlement_hint(text: str) -> str | None:
    normalized = text.lower()
    date_like = re.search(r"\b\d{4}-\d{2}-\d{2}\b", normalized)
    if date_like:
        return date_like.group(0)
    month_like = re.search(
        r"\b(jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec)[a-z]*\s+\d{1,2}\b",
        normalized,
    )
    if month_like:
        return month_like.group(0)
    return None


def compute_text_similarity(a: str, b: str) -> float:
    return SequenceMatcher(None, a, b).ratio()


def market_features(raw: dict[str, Any]) -> MarketFeatures:
    market_id = str(raw.get("id") or raw.get("market_id") or raw.get("ticker") or raw.get("slug") or "unknown")
    title = str(raw.get("title") or raw.get("question") or raw.get("name") or raw.get("slug") or "")
    normalized = normalize_title(title)
    return MarketFeatures(
        market_id=market_id,
        title=title,
        normalized_title=normalized,
        asset=extract_asset_symbol(title),
        window=extract_window(title),
        strike=extract_strike(title),
        settlement_hint=extract_settlement_hint(title),
    )


def _load_markets(path: Path) -> list[dict[str, Any]]:
    data = json.loads(path.read_text(encoding="utf-8"))
    if isinstance(data, list):
        return [row for row in data if isinstance(row, dict)]
    if isinstance(data, dict):
        for key in ("markets", "data", "results"):
            value = data.get(key)
            if isinstance(value, list):
                return [row for row in value if isinstance(row, dict)]
    return []


def _score_pair(poly: MarketFeatures, kalshi: MarketFeatures, min_confidence: float) -> dict[str, Any]:
    reasons: list[str] = []
    reject_reason: str | None = None

    text_sim = compute_text_similarity(poly.normalized_title, kalshi.normalized_title)
    score = 0.55 * text_sim
    reasons.append(f"text_similarity={text_sim:.4f}")

    if poly.asset and kalshi.asset:
        if poly.asset == kalshi.asset:
            score += 0.20
            reasons.append("asset_match")
        else:
            reasons.append("asset_mismatch")
            reject_reason = "asset_mismatch"
    elif poly.asset or kalshi.asset:
        reasons.append("asset_partial")

    if poly.window and kalshi.window:
        if poly.window == kalshi.window:
            score += 0.15
            reasons.append("window_match")
        else:
            reasons.append("window_mismatch")
            reject_reason = reject_reason or "settlement_window_mismatch"
    elif poly.window or kalshi.window:
        reasons.append("window_partial")

    if poly.strike is not None and kalshi.strike is not None:
        if abs(poly.strike - kalshi.strike) <= 1e-9:
            score += 0.10
            reasons.append("strike_match")
        else:
            reasons.append(f"strike_mismatch:{poly.strike}!={kalshi.strike}")
            reject_reason = reject_reason or "strike_mismatch"
    elif poly.strike is None and kalshi.strike is None:
        score += 0.05
        reasons.append("strike_absent_both")

    if poly.settlement_hint and kalshi.settlement_hint:
        if poly.settlement_hint == kalshi.settlement_hint:
            score += 0.05
            reasons.append("settlement_hint_match")
        else:
            reasons.append("settlement_hint_mismatch")

    confidence = max(0.0, min(1.0, round(score, 6)))

    rejected = confidence < min_confidence or reject_reason is not None
    if confidence < min_confidence and reject_reason is None:
        reject_reason = "confidence_below_threshold"

    return {
        "confidence": confidence,
        "reasons": reasons,
        "rejected": rejected,
        "reject_reason": reject_reason,
    }


def run(args: argparse.Namespace) -> dict[str, Any]:
    polymarkets = [market_features(row) for row in _load_markets(Path(args.polymarket_markets_json))]
    kalshi_markets = [market_features(row) for row in _load_markets(Path(args.kalshi_markets_json))]

    generated = 0
    persisted = 0

    for poly in polymarkets:
        if not kalshi_markets:
            break
        scored_pairs = []
        for kalshi in kalshi_markets:
            pair = _score_pair(poly, kalshi, float(args.min_confidence))
            scored_pairs.append((pair["confidence"], kalshi, pair))

        scored_pairs.sort(key=lambda item: item[0], reverse=True)
        _, best_kalshi, best = scored_pairs[0]

        payload = {
            "polymarket_market_id": poly.market_id,
            "kalshi_market_id": best_kalshi.market_id,
            "polymarket_title": poly.title,
            "kalshi_title": best_kalshi.title,
            "asset": poly.asset or best_kalshi.asset,
            "window": poly.window or best_kalshi.window,
            "strike": poly.strike if poly.strike is not None else best_kalshi.strike,
            "confidence": best["confidence"],
            "reasons": best["reasons"],
            "rejected": best["rejected"],
            "reject_reason": best["reject_reason"],
            "source": SOURCE,
        }
        event = build_event(
            event_type="cross_venue.market_match_scored",
            aggregate_key=f"cross_venue:{(payload['asset'] or 'unknown').lower()}",
            payload=payload,
            provenance=build_provenance(AGENT_ID, "research-only"),
            unique_components=[poly.market_id, best_kalshi.market_id, str(args.min_confidence)],
            timestamp=None,
        )
        generated += 1
        if append_event_jsonl(Path(args.output_jsonl), event, dry_run=args.dry_run):
            persisted += 1

    return {
        "actor": AGENT_ID,
        "source": SOURCE,
        "polymarket_markets_seen": len(polymarkets),
        "kalshi_markets_seen": len(kalshi_markets),
        "events_generated": generated,
        "events_persisted": persisted,
        "min_confidence": float(args.min_confidence),
        "dry_run": bool(args.dry_run),
    }


def main() -> int:
    args = parse_args()
    emit_json(run(args))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
