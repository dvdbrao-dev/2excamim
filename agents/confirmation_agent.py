#!/usr/bin/env python3
"""Confirmation Agent v1.

Reads the local JSONL store, evaluates generated signals against independent
market-quality checks, and persists confirmations directly as idempotent
signal.confirmed events in the JSONL store (no external cargo subprocess).
"""

from __future__ import annotations

import argparse
import json
import sys
import uuid
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from core.event_store import (
    append_event_idempotent,
    default_checkpoint_path,
    load_checkpoint,
    read_jsonl_since,
    save_checkpoint,
)
from core.logging import emit_json
from core.time import execution_run_id, utc_now_rfc3339


AGENT_ID = "confirmation-agent-v1"
PRODUCED_BY = "runtime.agent.confirmation"
DEFAULT_STORE = Path("./var/events.jsonl")
DEFAULT_THRESHOLD = 0.6
DEFAULT_MIN_MARKET_SCORE = 0.2
DEFAULT_MIN_VOLUME_USDC = 1_000.0
DEFAULT_MIN_HOURS_TO_RESOLUTION = 1.0
DEFAULT_MAX_HOURS_TO_RESOLUTION = 720.0
DEFAULT_REQUIRED_POSITIVE_CHECKS = 2


@dataclass
class SignalCandidate:
    signal_id: str
    strength: float
    signal_kind: str
    aggregate_key: str | None
    market_id: str | None
    hypothesis_id: str | None
    correlation_id: str | None
    parent_event_id: str | None


@dataclass
class MarketScoreInfo:
    market_id: str
    score: float | None
    volume_usdc: float | None
    hours_to_resolution: float | None


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Confirm eligible signals from the JSONL store.")
    parser.add_argument("--store", default=str(DEFAULT_STORE), help="Path to the JSONL store.")
    parser.add_argument(
        "--threshold",
        type=float,
        default=DEFAULT_THRESHOLD,
        help="Minimum signal strength check.",
    )
    parser.add_argument(
        "--min-market-score",
        type=float,
        default=DEFAULT_MIN_MARKET_SCORE,
        help="Minimum market.scored score to count as a positive check.",
    )
    parser.add_argument(
        "--min-volume-usdc",
        type=float,
        default=DEFAULT_MIN_VOLUME_USDC,
        help="Minimum market.scored volume_usdc to count as a positive check.",
    )
    parser.add_argument(
        "--min-hours-to-resolution",
        type=float,
        default=DEFAULT_MIN_HOURS_TO_RESOLUTION,
        help="Minimum accepted hours_to_resolution for confirmation checks.",
    )
    parser.add_argument(
        "--max-hours-to-resolution",
        type=float,
        default=DEFAULT_MAX_HOURS_TO_RESOLUTION,
        help="Maximum accepted hours_to_resolution for confirmation checks.",
    )
    parser.add_argument(
        "--required-positive-checks",
        type=int,
        default=DEFAULT_REQUIRED_POSITIVE_CHECKS,
        help="Minimum number of independent positive checks required to confirm.",
    )
    parser.add_argument(
        "--checkpoint",
        default="",
        help="Optional checkpoint path (default: <store-dir>/.checkpoints/<agent>.json).",
    )
    parser.add_argument(
        "--full-replay",
        action="store_true",
        help="Ignore checkpoint and replay full store.",
    )
    return parser.parse_args()


def normalized_market_key(value: Any) -> str | None:
    if not isinstance(value, str):
        return None
    trimmed = value.strip()
    if not trimmed:
        return None
    if ":" in trimmed:
        return trimmed.split(":", 1)[1]
    return trimmed


def reduce_events_into_state(
    state: dict[str, Any],
    events: list[dict[str, Any]],
) -> None:
    generated = state["generated"]
    confirmed = state["confirmed"]
    scored_markets = state["scored_markets"]

    for event in events:
        event_type = event.get("event_type")
        payload = event.get("payload") or {}
        linkage = event.get("linkage") or {}

        if event_type == "market.scored":
            market_id = normalized_market_key(payload.get("market_id")) or normalized_market_key(
                event.get("aggregate_key")
            )
            if isinstance(market_id, str):
                scored_markets[market_id] = {
                    "market_id": market_id,
                    "score": payload.get("score")
                    if isinstance(payload.get("score"), (int, float))
                    else None,
                    "volume_usdc": payload.get("volume_usdc")
                    if isinstance(payload.get("volume_usdc"), (int, float))
                    else None,
                    "hours_to_resolution": payload.get("hours_to_resolution")
                    if isinstance(payload.get("hours_to_resolution"), (int, float))
                    else None,
                }

        if event_type == "signal.generated":
            signal_id = payload.get("signal_id")
            strength = payload.get("strength")
            if not isinstance(signal_id, str) or not isinstance(strength, (int, float)):
                continue
            market_id = normalized_market_key(event.get("aggregate_key")) or normalized_market_key(
                payload.get("market_id")
            )
            generated[signal_id] = {
                "strength": float(strength),
                "signal_kind": "market",
                "aggregate_key": event.get("aggregate_key"),
                "market_id": market_id,
                "hypothesis_id": linkage.get("hypothesis_id"),
                "correlation_id": linkage.get("correlation_id"),
                "parent_event_id": linkage.get("parent_event_id"),
            }
        elif event_type == "crypto.signal.generated":
            signal_id = payload.get("signal_id")
            signal_strength = payload.get("signal_strength")
            if not isinstance(signal_id, str) or not isinstance(signal_strength, (int, float)):
                continue
            if float(signal_strength) < 0.4:
                continue

            generated[signal_id] = {
                "strength": float(signal_strength),
                "signal_kind": "crypto",
                "aggregate_key": event.get("aggregate_key"),
                "market_id": None,
                "hypothesis_id": linkage.get("hypothesis_id"),
                "correlation_id": linkage.get("correlation_id"),
                "parent_event_id": linkage.get("parent_event_id"),
            }
        elif event_type == "signal.confirmed":
            signal_id = payload.get("signal_id")
            confirmed_by = payload.get("confirmed_by")
            if isinstance(signal_id, str) and isinstance(confirmed_by, str):
                confirmed[f"{signal_id}|{confirmed_by}"] = True


def state_to_runtime(
    state: dict[str, Any]
) -> tuple[dict[str, SignalCandidate], set[tuple[str, str]], dict[str, MarketScoreInfo]]:
    generated: dict[str, SignalCandidate] = {}
    for signal_id, payload in state["generated"].items():
        if not isinstance(payload, dict):
            continue
        try:
            generated[signal_id] = SignalCandidate(
                signal_id=signal_id,
                strength=float(payload["strength"]),
                signal_kind=str(payload["signal_kind"]),
                aggregate_key=payload.get("aggregate_key"),
                market_id=payload.get("market_id"),
                hypothesis_id=payload.get("hypothesis_id"),
                correlation_id=payload.get("correlation_id"),
                parent_event_id=payload.get("parent_event_id"),
            )
        except (KeyError, TypeError, ValueError):
            continue

    confirmed = {
        tuple(key.split("|", 1))
        for key in state["confirmed"].keys()
        if isinstance(key, str) and "|" in key
    }
    scored_markets: dict[str, MarketScoreInfo] = {}
    raw_scored_markets = state.get("scored_markets")
    if isinstance(raw_scored_markets, dict):
        for market_id, raw in raw_scored_markets.items():
            if not isinstance(market_id, str) or not market_id:
                continue
            if isinstance(raw, dict):
                score = raw.get("score")
                volume_usdc = raw.get("volume_usdc")
                hours_to_resolution = raw.get("hours_to_resolution")
            else:
                score = None
                volume_usdc = None
                hours_to_resolution = None
            scored_markets[market_id] = MarketScoreInfo(
                market_id=market_id,
                score=float(score) if isinstance(score, (int, float)) else None,
                volume_usdc=float(volume_usdc) if isinstance(volume_usdc, (int, float)) else None,
                hours_to_resolution=(
                    float(hours_to_resolution)
                    if isinstance(hours_to_resolution, (int, float))
                    else None
                ),
            )
    return generated, confirmed, scored_markets


def runtime_to_state(
    generated: dict[str, SignalCandidate],
    confirmed: set[tuple[str, str]],
    scored_markets: dict[str, MarketScoreInfo],
) -> dict[str, Any]:
    return {
        "generated": {
            signal_id: {
                "strength": candidate.strength,
                "signal_kind": candidate.signal_kind,
                "aggregate_key": candidate.aggregate_key,
                "market_id": candidate.market_id,
                "hypothesis_id": candidate.hypothesis_id,
                "correlation_id": candidate.correlation_id,
                "parent_event_id": candidate.parent_event_id,
            }
            for signal_id, candidate in generated.items()
        },
        "confirmed": {f"{signal_id}|{confirmed_by}": True for signal_id, confirmed_by in confirmed},
        "scored_markets": {
            market_id: {
                "market_id": info.market_id,
                "score": info.score,
                "volume_usdc": info.volume_usdc,
                "hours_to_resolution": info.hours_to_resolution,
            }
            for market_id, info in scored_markets.items()
        },
    }


def checkpoint_path_from_args(args: argparse.Namespace, store_path: Path) -> Path:
    if isinstance(args.checkpoint, str) and args.checkpoint.strip():
        return Path(args.checkpoint)
    return default_checkpoint_path(store_path, AGENT_ID)


def confirm_via_jsonl(
    store_path: Path,
    candidate: SignalCandidate,
    run_id: str,
    threshold: float,
    confirmation_reasons: list[str],
    rejection_reasons: list[str],
) -> tuple[bool, dict[str, Any] | None]:
    """Write a signal.confirmed event directly to the JSONL store."""
    signal_id = candidate.signal_id
    idempotency_key = f"signal.confirmed:v1:{signal_id}:{AGENT_ID}"

    event: dict[str, Any] = {
        "event_id": str(uuid.uuid4()),
        "event_type": "signal.confirmed",
        "schema_version": "v1",
        "occurred_at": utc_now_rfc3339(),
        "produced_by": PRODUCED_BY,
        "idempotency_key": idempotency_key,
        "aggregate_key": candidate.aggregate_key,
        "linkage": {
            "hypothesis_id": candidate.hypothesis_id,
            "signal_id": signal_id,
            "decision_id": None,
            "order_id": None,
            "position_id": None,
            "parent_event_id": candidate.parent_event_id,
            "correlation_id": candidate.correlation_id or signal_id,
        },
        "provenance": {
            "source_kind": "Runtime",
            "source_ref": None,
            "producer_run_id": run_id,
            "actor": AGENT_ID,
            "trace_id": f"{run_id}:{signal_id}",
            "notes": (
                f"auto-confirmed strength={candidate.strength:.6f} threshold={threshold:.6f}"
            ),
        },
        "payload": {
            "signal_id": signal_id,
            "confirmed_by": AGENT_ID,
            "confirmation_reasons": confirmation_reasons,
            "rejection_reasons": rejection_reasons,
            "confirmation_score": round(candidate.strength, 6),
            "estimated_probability": round(candidate.strength, 6),
        },
    }

    try:
        append_event_idempotent(store_path, event)
        return True, None
    except Exception as error:
        return False, {
            "signal_id": signal_id,
            "signal_kind": candidate.signal_kind,
            "error_type": error.__class__.__name__,
            "error_message": str(error),
        }


def evaluate_market_candidate(
    candidate: SignalCandidate,
    market_info: MarketScoreInfo | None,
    args: argparse.Namespace,
) -> tuple[bool, list[str], list[str]]:
    confirmation_reasons: list[str] = []
    rejection_reasons: list[str] = []

    if candidate.strength >= args.threshold:
        confirmation_reasons.append(
            f"strength={candidate.strength:.6f} >= threshold={args.threshold:.6f}"
        )
    else:
        rejection_reasons.append(
            f"strength={candidate.strength:.6f} < threshold={args.threshold:.6f}"
        )

    if market_info is None:
        rejection_reasons.append("missing market.scored context")
    else:
        if isinstance(market_info.score, float) and market_info.score >= args.min_market_score:
            confirmation_reasons.append(
                f"market_score={market_info.score:.6f} >= min_market_score={args.min_market_score:.6f}"
            )
        else:
            rejection_reasons.append(
                f"market_score={market_info.score} below min_market_score={args.min_market_score:.6f}"
            )

        if isinstance(market_info.volume_usdc, float) and market_info.volume_usdc >= args.min_volume_usdc:
            confirmation_reasons.append(
                f"volume_usdc={market_info.volume_usdc:.2f} >= min_volume_usdc={args.min_volume_usdc:.2f}"
            )
        else:
            rejection_reasons.append(
                f"volume_usdc={market_info.volume_usdc} below min_volume_usdc={args.min_volume_usdc:.2f}"
            )

        if isinstance(market_info.hours_to_resolution, float):
            if args.min_hours_to_resolution <= market_info.hours_to_resolution <= args.max_hours_to_resolution:
                confirmation_reasons.append(
                    "hours_to_resolution="
                    f"{market_info.hours_to_resolution:.2f} within "
                    f"[{args.min_hours_to_resolution:.2f}, {args.max_hours_to_resolution:.2f}]"
                )
            else:
                rejection_reasons.append(
                    "hours_to_resolution="
                    f"{market_info.hours_to_resolution:.2f} outside "
                    f"[{args.min_hours_to_resolution:.2f}, {args.max_hours_to_resolution:.2f}]"
                )
        else:
            rejection_reasons.append("hours_to_resolution missing")

    confirmed = len(confirmation_reasons) >= args.required_positive_checks
    hours_failed = any(
        "hours_to_resolution" in r and "outside" in r
        for r in rejection_reasons
    )
    if hours_failed:
        confirmed = False
    if not confirmed and len(confirmation_reasons) < args.required_positive_checks:
        rejection_reasons.append(
            f"positive_checks={len(confirmation_reasons)} < required_positive_checks={args.required_positive_checks}"
        )

    return confirmed, confirmation_reasons, rejection_reasons


def main() -> int:
    args = parse_args()
    if args.required_positive_checks < 1:
        raise SystemExit("--required-positive-checks must be >= 1")
    if args.min_hours_to_resolution > args.max_hours_to_resolution:
        raise SystemExit("--min-hours-to-resolution must be <= --max-hours-to-resolution")

    store_path = Path(args.store)
    run_id = execution_run_id(AGENT_ID)
    checkpoint_path = checkpoint_path_from_args(args, store_path)

    checkpoint = {} if args.full_replay else load_checkpoint(checkpoint_path)
    offset = int(checkpoint.get("offset", 0)) if not args.full_replay else 0
    state = checkpoint.get("state")
    if not isinstance(state, dict):
        state = {
            "generated": {},
            "confirmed": {},
            "scored_markets": {},
        }
    else:
        restored_generated, restored_confirmed, restored_scored_markets = state_to_runtime(state)
        state = runtime_to_state(restored_generated, restored_confirmed, restored_scored_markets)

    events_read, next_offset = read_jsonl_since(store_path, offset)
    reduce_events_into_state(state, events_read)
    generated, confirmed, scored_markets = state_to_runtime(state)

    market_candidates = [candidate for candidate in generated.values() if candidate.signal_kind == "market"]
    crypto_candidates = [candidate for candidate in generated.values() if candidate.signal_kind == "crypto"]

    to_evaluate = [
        candidate
        for candidate in market_candidates
        if (candidate.signal_id, AGENT_ID) not in confirmed
    ]
    already_confirmed = len(market_candidates) - len(to_evaluate)

    jsonl_successes = 0
    unsupported_kinds = len(crypto_candidates)
    rejected = 0
    blocked_by_missing_market_score = 0
    persistence_failures: list[dict[str, Any]] = []
    confirmation_samples: list[dict[str, Any]] = []
    rejection_samples: list[dict[str, Any]] = []

    for candidate in to_evaluate:
        market_id = candidate.market_id or normalized_market_key(candidate.aggregate_key)
        market_info = scored_markets.get(market_id) if isinstance(market_id, str) else None
        if market_info is None:
            blocked_by_missing_market_score += 1

        is_confirmed, confirmation_reasons, rejection_reasons = evaluate_market_candidate(
            candidate, market_info, args
        )
        if not is_confirmed:
            rejected += 1
            rejection_samples.append(
                {
                    "signal_id": candidate.signal_id,
                    "market_id": market_id,
                    "confirmation_reasons": confirmation_reasons,
                    "rejection_reasons": rejection_reasons,
                }
            )
            continue

        success, error = confirm_via_jsonl(
            store_path,
            candidate,
            run_id,
            args.threshold,
            confirmation_reasons,
            rejection_reasons,
        )
        if success:
            jsonl_successes += 1
            confirmation_samples.append(
                {
                    "signal_id": candidate.signal_id,
                    "market_id": market_id,
                    "confirmation_reasons": confirmation_reasons,
                    "rejection_reasons": rejection_reasons,
                }
            )
            continue
        if error is not None:
            persistence_failures.append(error)

    safe_next_offset = next_offset
    if store_path.exists():
        safe_next_offset = store_path.stat().st_size

    save_checkpoint(
        checkpoint_path,
        {
            "offset": safe_next_offset,
            "state": runtime_to_state(generated, confirmed, scored_markets),
        },
    )

    emit_json(
        {
            "actor": AGENT_ID,
            "producer_run_id": run_id,
            "store": str(store_path),
            "threshold": args.threshold,
            "min_market_score": args.min_market_score,
            "min_volume_usdc": args.min_volume_usdc,
            "min_hours_to_resolution": args.min_hours_to_resolution,
            "max_hours_to_resolution": args.max_hours_to_resolution,
            "required_positive_checks": args.required_positive_checks,
            "generated_candidates": len(generated),
            "market_scored_markets": len(scored_markets),
            "eligible_candidates": len(to_evaluate),
            "blocked_by_missing_market_score": blocked_by_missing_market_score,
            "crypto_candidates": len(crypto_candidates),
            "already_confirmed": already_confirmed,
            "rejected_candidates": rejected,
            "confirmed_via_jsonl": jsonl_successes,
            "unsupported_kind_skipped": unsupported_kinds,
            "persistence_failures": len(persistence_failures),
            "persistence_error_sample": persistence_failures[0] if persistence_failures else None,
            "confirmation_sample": confirmation_samples[0] if confirmation_samples else None,
            "rejection_sample": rejection_samples[0] if rejection_samples else None,
            "events_read": len(events_read),
            "events_processed": len(events_read),
            "checkpoint_offset": offset,
            "next_checkpoint_offset": safe_next_offset,
            "full_replay": bool(args.full_replay),
            "total_confirmed_this_run": jsonl_successes,
        }
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
