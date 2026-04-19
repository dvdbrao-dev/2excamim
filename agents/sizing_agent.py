#!/usr/bin/env python3
"""Sizing Agent v1.

Reads eligible signals from the append-only JSONL store, applies Kelly sizing
against the most recent snapshot midpoint for the corresponding market, and
persists either `decision.formed` or `veto.raised`.
"""

from __future__ import annotations

import argparse
import json
import math
import sys
import uuid
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from core.event_store import (
    append_event_idempotent,
    default_checkpoint_path,
    load_checkpoint,
    load_jsonl,
    parse_timestamp,
    read_jsonl_since,
    save_checkpoint,
)
from core.logging import emit_json
from core.time import execution_run_id, utc_now_rfc3339


AGENT_ID = "sizing-agent-v1"
DEFAULT_STORE = Path("./var/events.jsonl")
DEFAULT_WATCH_DIR = Path("./var/market-watch")
DEFAULT_BANKROLL = 1000.0


@dataclass
class SignalCandidate:
    signal_id: str
    market_id: str
    instrument: str
    hypothesis_id: str | None
    correlation_id: str | None
    parent_event_id: str | None
    heuristic_score: float | None
    estimated_probability: float | None
    side: str | None
    market_price: float = 0.0
    bankroll: float = 0.0
    signal_kind: str = "market"


@dataclass
class SnapshotCandidate:
    market_id: str
    observed_at: datetime
    midpoint: float


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Apply Kelly sizing to eligible signals.")
    parser.add_argument("--store", default=str(DEFAULT_STORE), help="Path to the JSONL store.")
    parser.add_argument(
        "--watch-dir",
        default=str(DEFAULT_WATCH_DIR),
        help="Path to the market-watch state directory.",
    )
    parser.add_argument(
        "--bankroll",
        type=float,
        default=DEFAULT_BANKROLL,
        help="Kelly bankroll in quote currency.",
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


def normalized_market_id(value: str | None) -> str | None:
    if not isinstance(value, str):
        return None
    trimmed = value.strip()
    if not trimmed:
        return None
    if ":" in trimmed:
        return trimmed.split(":", 1)[1]
    return trimmed


def market_midpoint(snapshot: dict[str, Any]) -> float | None:
    best_bid = snapshot.get("best_bid")
    best_ask = snapshot.get("best_ask")
    if isinstance(best_bid, (int, float)) and isinstance(best_ask, (int, float)):
        if math.isfinite(float(best_bid)) and math.isfinite(float(best_ask)):
            return (float(best_bid) + float(best_ask)) / 2.0
    return None


def latest_snapshots(watch_dir: Path) -> dict[str, SnapshotCandidate]:
    snapshots_path = watch_dir / "snapshots.jsonl"
    latest: dict[str, SnapshotCandidate] = {}

    for snapshot in load_jsonl(snapshots_path):
        market_id = snapshot.get("market_id")
        observed_at = parse_timestamp(snapshot.get("observed_at"))
        midpoint = market_midpoint(snapshot)
        if not isinstance(market_id, str) or observed_at is None or midpoint is None:
            continue

        current = latest.get(market_id)
        candidate = SnapshotCandidate(
            market_id=market_id,
            observed_at=observed_at,
            midpoint=midpoint,
        )
        if current is None or candidate.observed_at > current.observed_at:
            latest[market_id] = candidate

    return latest


def collect_candidates(
    events: list[dict[str, Any]]
) -> tuple[dict[str, SignalCandidate], dict[str, dict[str, float | None]], set[str]]:
    generated: dict[str, SignalCandidate] = {}
    confirmed: dict[str, dict[str, float | None]] = {}
    vetoed: set[str] = set()

    for event in events:
        event_type = event.get("event_type")
        payload = event.get("payload") or {}
        linkage = event.get("linkage") or {}

        if event_type == "signal.generated":
            signal_id = payload.get("signal_id")
            if not isinstance(signal_id, str):
                continue

            market_id = normalized_market_id(event.get("aggregate_key")) or normalized_market_id(
                payload.get("instrument")
            )
            if market_id is None:
                continue

            side = payload.get("side")
            if not isinstance(side, str):
                side = None

            generated[signal_id] = SignalCandidate(
                signal_id=signal_id,
                market_id=market_id,
                instrument=payload.get("instrument") if isinstance(payload.get("instrument"), str) else market_id,
                hypothesis_id=linkage.get("hypothesis_id"),
                correlation_id=linkage.get("correlation_id"),
                parent_event_id=linkage.get("parent_event_id"),
                heuristic_score=None,
                estimated_probability=None,
                side=normalize_side(side),
            )
        elif event_type == "signal.confirmed":
            signal_id = payload.get("signal_id")
            if not isinstance(signal_id, str):
                continue

            heuristic_score = payload.get("heuristic_score")
            confirmation_score = payload.get("confirmation_score")
            estimated_probability = payload.get("estimated_probability")

            normalized_heuristic_score = normalize_probability(heuristic_score)
            if normalized_heuristic_score is None:
                normalized_heuristic_score = normalize_probability(confirmation_score)
            normalized_estimated_probability = normalize_probability(estimated_probability)

            if (
                normalized_heuristic_score is None
                and normalized_estimated_probability is None
            ):
                continue

            existing = confirmed.get(signal_id)
            confirmed[signal_id] = {
                "heuristic_score": (
                    normalized_heuristic_score
                    if normalized_heuristic_score is not None
                    else (existing or {}).get("heuristic_score")
                ),
                "estimated_probability": (
                    normalized_estimated_probability
                    if normalized_estimated_probability is not None
                    else (existing or {}).get("estimated_probability")
                ),
            }
        elif event_type == "veto.raised":
            if payload.get("scope") != "Signal":
                continue
            target_id = payload.get("target_id")
            if isinstance(target_id, str):
                vetoed.add(target_id)

    return generated, confirmed, vetoed


def collect_crypto_signals(events: list[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    signals: dict[str, dict[str, Any]] = {}

    for event in events:
        if event.get("event_type") != "crypto.signal.generated":
            continue

        payload = event.get("payload") or {}
        linkage = event.get("linkage") or {}
        signal_id = payload.get("signal_id")
        symbol = payload.get("symbol")
        trend = payload.get("trend")
        signal_strength = payload.get("signal_strength")
        if (
            not isinstance(signal_id, str)
            or not signal_id.strip()
            or not isinstance(symbol, str)
            or not symbol.strip()
            or trend not in {"UP", "DOWN"}
            or not isinstance(signal_strength, (int, float))
        ):
            continue

        signals[signal_id] = {
            "signal_id": signal_id,
            "symbol": symbol,
            "trend": trend,
            "signal_strength": float(signal_strength),
            "aggregate_key": event.get("aggregate_key"),
            "hypothesis_id": linkage.get("hypothesis_id"),
            "correlation_id": linkage.get("correlation_id"),
            "parent_event_id": linkage.get("parent_event_id"),
        }

    return signals


def normalize_probability(value: Any) -> float | None:
    if isinstance(value, (int, float)) and math.isfinite(float(value)):
        probability = float(value)
        if 0.0 <= probability <= 1.0:
            return probability
    return None


def normalize_side(value: Any) -> str | None:
    if not isinstance(value, str):
        return None
    normalized = value.strip().lower()
    if not normalized:
        return None
    if normalized in {"long_yes", "short_no", "long", "yes"}:
        return "long_yes"
    if normalized in {"long_no", "short_yes", "short", "no"}:
        return "long_no"
    return None


def choose_crypto_match(matched_markets: Any) -> dict[str, Any] | None:
    if not isinstance(matched_markets, list):
        return None

    candidates: list[tuple[int, float, str, dict[str, Any]]] = []
    for market in matched_markets:
        if not isinstance(market, dict):
            continue
        market_id = market.get("market_id")
        title = market.get("title")
        midpoint = market.get("midpoint")
        if not isinstance(market_id, str) or not market_id.strip():
            continue
        if not isinstance(title, str) or not title.strip():
            continue
        if not isinstance(midpoint, (int, float)) or not math.isfinite(float(midpoint)):
            continue

        coherent = 1 if market.get("coherent") is True else 0
        candidates.append((coherent, float(midpoint), title.strip().lower(), market))

    if not candidates:
        return None

    candidates.sort(key=lambda item: (item[0], item[1], item[2]), reverse=True)
    return candidates[0][3]


def collect_crypto_matches(events: list[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    matches: dict[str, tuple[datetime, dict[str, Any]]] = {}

    for event in events:
        if event.get("event_type") != "crypto.market.matched":
            continue

        payload = event.get("payload") or {}
        signal_id = payload.get("signal_id")
        chosen = choose_crypto_match(payload.get("matched_markets"))
        if not isinstance(signal_id, str) or not signal_id.strip() or chosen is None:
            continue

        observed_at = parse_timestamp(event.get("occurred_at")) or datetime.min.replace(
            tzinfo=timezone.utc
        )
        current = matches.get(signal_id)
        if current is None or observed_at > current[0]:
            matches[signal_id] = (observed_at, chosen)

    return {signal_id: match for signal_id, (_, match) in matches.items()}


def build_crypto_candidate(
    signal: dict[str, Any],
    match: dict[str, Any],
    heuristic_score: float | None,
    estimated_probability: float | None,
) -> SignalCandidate | None:
    market_id = match.get("market_id")
    title = match.get("title")
    midpoint = match.get("midpoint")
    suggested_direction = match.get("suggested_direction")
    if (
        not isinstance(market_id, str)
        or not market_id.strip()
        or not isinstance(title, str)
        or not title.strip()
        or not isinstance(midpoint, (int, float))
        or not math.isfinite(float(midpoint))
        or not isinstance(suggested_direction, str)
        or not suggested_direction.strip()
    ):
        return None

    return SignalCandidate(
        signal_id=signal["signal_id"],
        market_id=market_id,
        instrument=f"polymarket:{market_id}",
        hypothesis_id=signal.get("hypothesis_id"),
        correlation_id=signal.get("correlation_id"),
        parent_event_id=signal.get("parent_event_id"),
        heuristic_score=heuristic_score,
        estimated_probability=estimated_probability,
        side=normalize_side(suggested_direction),
        market_price=float(midpoint),
        bankroll=0.0,
        signal_kind="crypto",
    )


def kelly_inputs(candidate: SignalCandidate) -> tuple[float, float] | tuple[None, None]:
    if candidate.estimated_probability is None:
        return None, None

    if candidate.side == "long_yes":
        return candidate.estimated_probability, candidate.market_price
    if candidate.side == "long_no":
        return 1.0 - candidate.estimated_probability, 1.0 - candidate.market_price
    return None, None


def kelly_size(p_win: float, market_price: float, bankroll: float, max_fraction: float = 0.25) -> float:
    if (
        not math.isfinite(p_win)
        or not math.isfinite(market_price)
        or not math.isfinite(bankroll)
        or not math.isfinite(max_fraction)
        or bankroll <= 0
        or market_price <= 0
        or market_price >= 1
        or max_fraction <= 0
    ):
        print(
            "kelly_size invalid "
            f"p_win={p_win:.6f} market_price={market_price:.6f} "
            f"bankroll={bankroll:.2f} cap={max_fraction:.4f} "
            "b=n/a f_star=n/a size_before_cap=0.00 size_after_cap=0.00",
            flush=True,
        )
        return 0.0

    b = (1 / market_price) - 1
    if b <= 0:
        print(
            "kelly_size invalid "
            f"p_win={p_win:.6f} market_price={market_price:.6f} "
            f"bankroll={bankroll:.2f} cap={max_fraction:.4f} "
            f"b={b:.6f} f_star=n/a size_before_cap=0.00 size_after_cap=0.00",
            flush=True,
        )
        return 0.0
    q = 1 - p_win
    f_star = (p_win * b - q) / b
    size_before_cap = bankroll * max(f_star, 0.0)
    size_after_cap = bankroll * min(max(f_star, 0.0), max_fraction)
    print(
        "kelly_size "
        f"p_win={p_win:.6f} market_price={market_price:.6f} "
        f"b={b:.6f} f_star={f_star:.6f} "
        f"size_before_cap={size_before_cap:.2f} size_after_cap={size_after_cap:.2f}",
        flush=True,
    )
    if f_star <= 0:
        return 0.0
    return round(size_after_cap, 2)


def deterministic_decision_id(signal_id: str) -> str:
    return f"decision-{signal_id}"


def deterministic_veto_id(signal_id: str) -> str:
    return f"sizing-veto-{signal_id}"


def existing_event_ids(events: list[dict[str, Any]]) -> tuple[set[str], set[str]]:
    decisions: set[str] = set()
    vetoes: set[str] = set()

    for event in events:
        event_type = event.get("event_type")
        payload = event.get("payload") or {}
        if event_type == "decision.formed":
            decision_id = payload.get("decision_id")
            if isinstance(decision_id, str):
                decisions.add(decision_id)
        elif event_type == "veto.raised":
            veto_id = payload.get("veto_id")
            if isinstance(veto_id, str):
                vetoes.add(veto_id)

    return decisions, vetoes


def active_decision_markets(events: list[dict[str, Any]]) -> set[str]:
    decision_indexes: dict[str, tuple[int, str]] = {}
    decision_veto_indexes: dict[str, int] = {}

    for index, event in enumerate(events):
        event_type = event.get("event_type")
        payload = event.get("payload") or {}

        if event_type == "decision.formed":
            decision_id = payload.get("decision_id")
            aggregate_key = event.get("aggregate_key")
            if isinstance(decision_id, str) and isinstance(aggregate_key, str):
                decision_indexes[decision_id] = (index, aggregate_key)
        elif event_type == "veto.raised" and payload.get("scope") == "Decision":
            target_id = payload.get("target_id")
            if isinstance(target_id, str):
                previous = decision_veto_indexes.get(target_id)
                if previous is None or index > previous:
                    decision_veto_indexes[target_id] = index

    active_markets: set[str] = set()
    for decision_id, (decision_index, aggregate_key) in decision_indexes.items():
        veto_index = decision_veto_indexes.get(decision_id)
        if veto_index is None or veto_index <= decision_index:
            active_markets.add(aggregate_key)

    return active_markets


def active_deployed_usd(events: list[dict[str, Any]]) -> float:
    decision_indexes: dict[str, int] = {}
    decision_sizes: dict[str, float] = {}
    decision_veto_indexes: dict[str, int] = {}

    for index, event in enumerate(events):
        event_type = event.get("event_type")
        payload = event.get("payload") or {}

        if event_type == "decision.formed":
            decision_id = payload.get("decision_id")
            size_hint = payload.get("size_hint")
            if isinstance(decision_id, str):
                decision_indexes[decision_id] = index
                decision_sizes[decision_id] = float(size_hint) if isinstance(size_hint, (int, float)) else 0.0
        elif event_type == "veto.raised" and payload.get("scope") == "Decision":
            target_id = payload.get("target_id")
            if isinstance(target_id, str):
                previous = decision_veto_indexes.get(target_id)
                if previous is None or index > previous:
                    decision_veto_indexes[target_id] = index

    total = 0.0
    for decision_id, decision_index in decision_indexes.items():
        veto_index = decision_veto_indexes.get(decision_id)
        if veto_index is None or veto_index <= decision_index:
            total += decision_sizes.get(decision_id, 0.0)

    return total


def _empty_sizing_state() -> dict[str, Any]:
    return {
        "generated": {},
        "confirmed": {},
        "vetoed": [],
        "crypto_signals": {},
        "crypto_matches": {},
        "existing_decisions": [],
        "existing_vetoes": [],
        "decision_status": {},
        "market_active_counts": {},
        "current_deployed": 0.0,
    }


def _apply_decision_state_delta(state: dict[str, Any], events: list[dict[str, Any]]) -> None:
    decision_status = state["decision_status"]
    market_active_counts = state["market_active_counts"]
    current_deployed = float(state.get("current_deployed", 0.0))

    def deactivate(decision_id: str) -> None:
        nonlocal current_deployed
        entry = decision_status.get(decision_id)
        if not isinstance(entry, dict) or not entry.get("active"):
            return
        market = entry.get("aggregate_key")
        size_hint = float(entry.get("size_hint", 0.0))
        if isinstance(market, str) and market:
            market_active_counts[market] = max(int(market_active_counts.get(market, 0)) - 1, 0)
            if market_active_counts[market] == 0:
                market_active_counts.pop(market, None)
        current_deployed = max(current_deployed - size_hint, 0.0)
        entry["active"] = False

    def activate(decision_id: str, market: str | None, size_hint: float) -> None:
        nonlocal current_deployed
        deactivate(decision_id)
        if not isinstance(market, str) or not market:
            return
        decision_status[decision_id] = {
            "aggregate_key": market,
            "size_hint": float(size_hint),
            "active": True,
        }
        market_active_counts[market] = int(market_active_counts.get(market, 0)) + 1
        current_deployed += float(size_hint)

    for event in events:
        event_type = event.get("event_type")
        payload = event.get("payload") or {}
        if event_type == "decision.formed":
            decision_id = payload.get("decision_id")
            aggregate_key = event.get("aggregate_key")
            size_hint = payload.get("size_hint")
            if isinstance(decision_id, str):
                state["existing_decisions"].append(decision_id)
                activate(
                    decision_id,
                    aggregate_key if isinstance(aggregate_key, str) else None,
                    float(size_hint) if isinstance(size_hint, (int, float)) else 0.0,
                )
        elif event_type == "veto.raised":
            veto_id = payload.get("veto_id")
            if isinstance(veto_id, str):
                state["existing_vetoes"].append(veto_id)
            if payload.get("scope") == "Decision":
                target_id = payload.get("target_id")
                if isinstance(target_id, str):
                    deactivate(target_id)

    state["current_deployed"] = round(current_deployed, 8)


def apply_events_to_sizing_state(state: dict[str, Any], events: list[dict[str, Any]]) -> None:
    generated, confirmed, vetoed = collect_candidates(events)
    state["generated"].update(
        {
            signal_id: {
                "signal_id": candidate.signal_id,
                "market_id": candidate.market_id,
                "instrument": candidate.instrument,
                "hypothesis_id": candidate.hypothesis_id,
                "correlation_id": candidate.correlation_id,
                "parent_event_id": candidate.parent_event_id,
                "heuristic_score": candidate.heuristic_score,
                "estimated_probability": candidate.estimated_probability,
                "side": candidate.side,
                "signal_kind": candidate.signal_kind,
            }
            for signal_id, candidate in generated.items()
        }
    )
    state["confirmed"].update(confirmed)
    state["vetoed"] = sorted(set(state.get("vetoed", [])) | set(vetoed))
    state["crypto_signals"].update(collect_crypto_signals(events))
    state["crypto_matches"].update(collect_crypto_matches(events))
    _apply_decision_state_delta(state, events)
    state["existing_decisions"] = sorted(set(state.get("existing_decisions", [])))
    state["existing_vetoes"] = sorted(set(state.get("existing_vetoes", [])))


def restore_sizing_state(payload: dict[str, Any]) -> dict[str, Any]:
    state = _empty_sizing_state()
    if not isinstance(payload, dict):
        return state
    for key in state.keys():
        if key in payload:
            state[key] = payload[key]
    if not isinstance(state["generated"], dict):
        state["generated"] = {}
    if not isinstance(state["confirmed"], dict):
        state["confirmed"] = {}
    if not isinstance(state["crypto_signals"], dict):
        state["crypto_signals"] = {}
    if not isinstance(state["crypto_matches"], dict):
        state["crypto_matches"] = {}
    if not isinstance(state["decision_status"], dict):
        state["decision_status"] = {}
    if not isinstance(state["market_active_counts"], dict):
        state["market_active_counts"] = {}
    state["vetoed"] = [item for item in state.get("vetoed", []) if isinstance(item, str)]
    state["existing_decisions"] = [
        item for item in state.get("existing_decisions", []) if isinstance(item, str)
    ]
    state["existing_vetoes"] = [item for item in state.get("existing_vetoes", []) if isinstance(item, str)]
    if not isinstance(state.get("current_deployed"), (int, float)):
        state["current_deployed"] = 0.0
    return state


def runtime_from_sizing_state(
    state: dict[str, Any],
) -> tuple[
    dict[str, SignalCandidate],
    dict[str, dict[str, float | None]],
    set[str],
    dict[str, dict[str, Any]],
    dict[str, dict[str, Any]],
    set[str],
    set[str],
    set[str],
    float,
]:
    generated: dict[str, SignalCandidate] = {}
    for signal_id, raw in state.get("generated", {}).items():
        if not isinstance(raw, dict):
            continue
        try:
            generated[signal_id] = SignalCandidate(
                signal_id=raw["signal_id"],
                market_id=raw["market_id"],
                instrument=raw["instrument"],
                hypothesis_id=raw.get("hypothesis_id"),
                correlation_id=raw.get("correlation_id"),
                parent_event_id=raw.get("parent_event_id"),
                heuristic_score=raw.get("heuristic_score")
                if isinstance(raw.get("heuristic_score"), (int, float))
                else None,
                estimated_probability=raw.get("estimated_probability")
                if isinstance(raw.get("estimated_probability"), (int, float))
                else None,
                side=normalize_side(raw.get("side")),
                signal_kind=raw.get("signal_kind") if isinstance(raw.get("signal_kind"), str) else "market",
            )
        except (KeyError, TypeError):
            continue

    confirmed: dict[str, dict[str, float | None]] = {}
    for signal_id, raw in state.get("confirmed", {}).items():
        if not isinstance(raw, dict):
            continue
        confirmed[signal_id] = {
            "heuristic_score": raw.get("heuristic_score")
            if isinstance(raw.get("heuristic_score"), (int, float))
            else None,
            "estimated_probability": raw.get("estimated_probability")
            if isinstance(raw.get("estimated_probability"), (int, float))
            else None,
        }

    vetoed = set(state.get("vetoed", []))
    crypto_signals = state.get("crypto_signals", {})
    crypto_matches = state.get("crypto_matches", {})
    existing_decisions = set(state.get("existing_decisions", []))
    existing_vetoes = set(state.get("existing_vetoes", []))
    active_markets = {
        market
        for market, count in state.get("market_active_counts", {}).items()
        if isinstance(market, str) and isinstance(count, int) and count > 0
    }
    current_deployed = float(state.get("current_deployed", 0.0))

    return (
        generated,
        confirmed,
        vetoed,
        crypto_signals,
        crypto_matches,
        existing_decisions,
        existing_vetoes,
        active_markets,
        current_deployed,
    )


def build_provenance(run_id: str, watch_dir: Path, market_id: str, notes: str) -> dict[str, Any]:
    return {
        "source_kind": "Runtime",
        "source_ref": str(watch_dir),
        "producer_run_id": run_id,
        "actor": AGENT_ID,
        "trace_id": f"{run_id}:{market_id}",
        "notes": notes,
    }


def build_decision_event(
    candidate: SignalCandidate,
    run_id: str,
    watch_dir: Path,
    size_hint: float,
    p_win: float,
    kelly_price: float,
) -> dict[str, Any]:
    decision_id = deterministic_decision_id(candidate.signal_id)
    return {
        "event_id": str(uuid.uuid4()),
        "event_type": "decision.formed",
        "schema_version": "v1",
        "occurred_at": utc_now_rfc3339(),
        "produced_by": "runtime.agent.sizing",
        "idempotency_key": f"decision.formed:v1:{decision_id}",
        "aggregate_key": candidate.instrument,
        "linkage": {
            "hypothesis_id": candidate.hypothesis_id,
            "signal_id": candidate.signal_id,
            "decision_id": decision_id,
            "order_id": None,
            "position_id": None,
            "parent_event_id": candidate.parent_event_id,
            "correlation_id": candidate.correlation_id,
        },
        "provenance": build_provenance(
            run_id,
            watch_dir,
            candidate.market_id,
            (
                f"kelly sizing approved side={candidate.side} "
                f"estimated_probability={candidate.estimated_probability:.6f} "
                f"p_win={p_win:.6f} kelly_price={kelly_price:.6f} size_hint={size_hint:.2f}"
            ),
        ),
        "payload": {
            "decision_id": decision_id,
            "instrument": candidate.instrument,
            "action": "Enter",
            "side": candidate.side,
            "size_hint": size_hint,
            "rationale": (
                f"kelly sizing approved side={candidate.side} "
                f"estimated_probability={candidate.estimated_probability:.6f} "
                f"p_win={p_win:.6f} kelly_price={kelly_price:.6f} "
                f"market_price={candidate.market_price:.6f} bankroll={candidate.bankroll:.2f}"
            ),
        },
    }


def build_veto_event(
    candidate: SignalCandidate, run_id: str, watch_dir: Path, reason_text: str
) -> dict[str, Any]:
    veto_id = deterministic_veto_id(candidate.signal_id)
    return {
        "event_id": str(uuid.uuid4()),
        "event_type": "veto.raised",
        "schema_version": "v1",
        "occurred_at": utc_now_rfc3339(),
        "produced_by": "runtime.agent.sizing",
        "idempotency_key": f"veto.raised:v1:{veto_id}",
        "aggregate_key": candidate.instrument,
        "linkage": {
            "hypothesis_id": candidate.hypothesis_id,
            "signal_id": candidate.signal_id,
            "decision_id": None,
            "order_id": None,
            "position_id": None,
            "parent_event_id": candidate.parent_event_id,
            "correlation_id": candidate.correlation_id,
        },
        "provenance": build_provenance(
            run_id,
            watch_dir,
            candidate.market_id,
            f"kelly sizing vetoed signal due to negative EV: {reason_text}",
        ),
        "payload": {
            "veto_id": veto_id,
            "scope": "Signal",
            "target_id": candidate.signal_id,
            "reason_code": "negative_ev",
            "reason_text": reason_text,
            "raised_by": AGENT_ID,
        },
    }


def main() -> int:
    args = parse_args()
    store_path = Path(args.store)
    watch_dir = Path(args.watch_dir)
    run_id = execution_run_id(AGENT_ID)
    checkpoint_path = (
        Path(args.checkpoint)
        if isinstance(args.checkpoint, str) and args.checkpoint.strip()
        else default_checkpoint_path(store_path, AGENT_ID)
    )
    checkpoint = {} if args.full_replay else load_checkpoint(checkpoint_path)
    offset = int(checkpoint.get("offset", 0)) if not args.full_replay else 0
    events, next_offset = read_jsonl_since(store_path, offset)
    state = restore_sizing_state(checkpoint.get("state") if isinstance(checkpoint.get("state"), dict) else {})
    if offset == 0:
        state = _empty_sizing_state()
    apply_events_to_sizing_state(state, events)

    (
        generated,
        confirmed,
        vetoed,
        crypto_signals,
        crypto_matches,
        existing_decisions,
        existing_vetoes,
        active_markets,
        current_deployed,
    ) = runtime_from_sizing_state(state)
    snapshots = latest_snapshots(watch_dir)
    bankroll_limit = float(args.bankroll) * 0.6

    eligible_candidates = [
        SignalCandidate(
            signal_id=candidate.signal_id,
            market_id=candidate.market_id,
            instrument=candidate.instrument,
            hypothesis_id=candidate.hypothesis_id,
            correlation_id=candidate.correlation_id,
            parent_event_id=candidate.parent_event_id,
            heuristic_score=confirmed[signal_id].get("heuristic_score"),
            estimated_probability=confirmed[signal_id].get("estimated_probability"),
            side=normalize_side(candidate.side),
            signal_kind="market",
        )
        for signal_id, candidate in generated.items()
        if signal_id in confirmed and signal_id not in vetoed
    ]

    crypto_eligible_candidates = []
    for signal_id, signal in crypto_signals.items():
        if signal_id not in confirmed or signal_id in vetoed:
            continue
        match = crypto_matches.get(signal_id)
        if match is None:
            continue
        candidate = build_crypto_candidate(
            signal,
            match,
            confirmed[signal_id].get("heuristic_score"),
            confirmed[signal_id].get("estimated_probability"),
        )
        if candidate is not None:
            crypto_eligible_candidates.append(candidate)

    decisions_written = 0
    vetoes_written = 0
    skipped_missing_snapshot = 0
    skipped_active_decision = 0
    skipped_already_written = 0
    skipped_missing_estimated_probability = 0
    skipped_invalid_side = 0
    skipped_not_eligible = len(generated) - len(eligible_candidates)
    skipped_crypto_not_eligible = len(crypto_signals) - len(crypto_eligible_candidates)

    for candidate in eligible_candidates + crypto_eligible_candidates:
        if candidate.signal_kind == "market":
            snapshot = snapshots.get(candidate.market_id)
            if snapshot is None:
                skipped_missing_snapshot += 1
                continue

            candidate.market_price = snapshot.midpoint
        elif candidate.market_price <= 0:
            continue

        candidate.bankroll = float(args.bankroll)

        if candidate.instrument in active_markets:
            skipped_active_decision += 1
            continue

        if candidate.estimated_probability is None:
            skipped_missing_estimated_probability += 1
            continue

        p_win, kelly_price = kelly_inputs(candidate)
        if p_win is None or kelly_price is None:
            skipped_invalid_side += 1
            continue

        max_fraction = 0.03 if candidate.signal_kind == "crypto" else 0.05
        size = kelly_size(
            p_win,
            kelly_price,
            float(args.bankroll),
            max_fraction=max_fraction,
        )

        if size <= 0:
            veto_id = deterministic_veto_id(candidate.signal_id)
            if veto_id in existing_vetoes:
                skipped_already_written += 1
                continue
            reason_text = (
                f"kelly size is zero for side={candidate.side} "
                f"market_price={candidate.market_price:.6f} "
                f"kelly_price={kelly_price:.6f} "
                f"estimated_probability={candidate.estimated_probability:.6f} "
                f"p_win={p_win:.6f} bankroll={float(args.bankroll):.2f}"
            )
            persisted = append_event_idempotent(
                store_path,
                build_veto_event(candidate, run_id, watch_dir, reason_text),
            )
            if persisted:
                vetoes_written += 1
            existing_vetoes.add(veto_id)
            state["existing_vetoes"] = sorted(existing_vetoes)
            vetoed.add(candidate.signal_id)
            state["vetoed"] = sorted(vetoed)
            continue

        if current_deployed + size > bankroll_limit:
            continue

        decision_id = deterministic_decision_id(candidate.signal_id)
        if decision_id in existing_decisions:
            skipped_already_written += 1
            continue

        persisted = append_event_idempotent(
            store_path,
            build_decision_event(candidate, run_id, watch_dir, size, p_win, kelly_price),
        )
        if persisted:
            decisions_written += 1
        existing_decisions.add(decision_id)
        state["existing_decisions"] = sorted(existing_decisions)
        active_markets.add(candidate.instrument)
        state["market_active_counts"][candidate.instrument] = int(
            state["market_active_counts"].get(candidate.instrument, 0)
        ) + 1
        status = state["decision_status"]
        status[decision_id] = {
            "aggregate_key": candidate.instrument,
            "size_hint": float(size),
            "active": True,
        }
        current_deployed += size
        state["current_deployed"] = round(current_deployed, 8)

    # Store checkpoint after appends so we do not re-read our own writes.
    safe_next_offset = next_offset
    if store_path.exists():
        safe_next_offset = store_path.stat().st_size
    save_checkpoint(
        checkpoint_path,
        {
            "offset": safe_next_offset,
            "state": state,
        },
    )

    emit_json(
        {
            "actor": AGENT_ID,
            "producer_run_id": run_id,
            "store": str(store_path),
            "watch_dir": str(watch_dir),
            "bankroll": float(args.bankroll),
            "generated_signals": len(generated),
            "eligible_signals": len(eligible_candidates),
            "crypto_signals": len(crypto_signals),
            "crypto_eligible_signals": len(crypto_eligible_candidates),
            "decisions_written": decisions_written,
            "vetoes_written": vetoes_written,
            "skipped_missing_snapshot": skipped_missing_snapshot,
            "skipped_active_decision": skipped_active_decision,
            "skipped_already_written": skipped_already_written,
            "skipped_missing_estimated_probability": skipped_missing_estimated_probability,
            "skipped_invalid_side": skipped_invalid_side,
            "skipped_not_eligible": skipped_not_eligible,
            "skipped_crypto_not_eligible": skipped_crypto_not_eligible,
            "events_read": len(events),
            "events_processed": len(events),
            "checkpoint_offset": offset,
            "next_checkpoint_offset": safe_next_offset,
            "full_replay": bool(args.full_replay),
        }
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
