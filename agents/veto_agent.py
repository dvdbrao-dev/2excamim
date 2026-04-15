#!/usr/bin/env python3
"""Veto Agent v1.

Reads the local JSONL store, inspects confirmed signals, and writes `veto.raised`
events directly to the store when a small, explicit veto rule is met.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import uuid
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


AGENT_ID = "veto-agent-v1"
DEFAULT_STORE = Path("./var/events.jsonl")
DEFAULT_PROBABILITY_FLOOR = 0.10
PRODUCED_BY = "runtime.agent.veto"
REASON_CODE = "research_probability_below_floor"
RULE_NAME = "confirmed_signal_probability_floor"

PROBABILITY_PATTERN = re.compile(r"\bprobability=([0-9]+(?:\.[0-9]+)?)\b")


@dataclass
class SignalContext:
    signal_id: str
    aggregate_key: str | None
    hypothesis_id: str | None
    correlation_id: str | None
    parent_event_id: str | None
    generated_event_id: str | None
    confirmation_event_id: str | None
    probability: float | None = None


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Raise vetoes for weak confirmed signals.")
    parser.add_argument("--store", default=str(DEFAULT_STORE), help="Path to the JSONL store.")
    parser.add_argument(
        "--probability-floor",
        type=float,
        default=DEFAULT_PROBABILITY_FLOOR,
        help="Minimum research probability required to avoid veto.",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Inspect candidates without appending veto events.",
    )
    return parser.parse_args()


def utc_now_rfc3339() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def execution_run_id() -> str:
    timestamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S%fZ")
    return f"{AGENT_ID}-{timestamp}"


def load_events(store_path: Path) -> list[dict[str, Any]]:
    if not store_path.exists():
        return []

    events: list[dict[str, Any]] = []
    for line_number, line in enumerate(store_path.read_text(encoding="utf-8").splitlines(), start=1):
        stripped = line.strip()
        if not stripped:
            continue
        try:
            events.append(json.loads(stripped))
        except json.JSONDecodeError as error:
            raise SystemExit(
                f"invalid JSONL in {store_path} at line {line_number}: {error}"
            ) from error
    return events


def parse_probability_from_notes(notes: Any) -> float | None:
    if not isinstance(notes, str) or not notes.strip():
        return None
    try:
        parsed = json.loads(notes)
    except json.JSONDecodeError:
        return None
    return extract_probability(parsed)


def extract_probability(value: Any) -> float | None:
    if isinstance(value, dict):
        direct = value.get("probability")
        if isinstance(direct, (int, float)):
            probability = float(direct)
            if 0.0 <= probability <= 1.0:
                return probability
        metadata = value.get("metadata")
        if isinstance(metadata, dict):
            nested = metadata.get("probability")
            if isinstance(nested, (int, float)):
                probability = float(nested)
                if 0.0 <= probability <= 1.0:
                    return probability
    return None


def parse_probability_from_rationale(rationale: Any) -> float | None:
    if not isinstance(rationale, str):
        return None
    match = PROBABILITY_PATTERN.search(rationale)
    if not match:
        return None
    probability = float(match.group(1))
    if 0.0 <= probability <= 1.0:
        return probability
    return None


def deterministic_veto_id(signal_id: str) -> str:
    return f"veto-signal-{signal_id}-{REASON_CODE}"


def collect_signal_contexts(
    events: list[dict[str, Any]],
) -> tuple[dict[str, SignalContext], set[str], dict[str, str], set[str]]:
    contexts: dict[str, SignalContext] = {}
    confirmed_signals: set[str] = set()
    vetoed_signal_ids_by_veto_id: dict[str, str] = {}
    vetoed_signal_ids_any: set[str] = set()

    for event in events:
        event_type = event.get("event_type")
        payload = event.get("payload") or {}
        linkage = event.get("linkage") or {}
        provenance = event.get("provenance") or {}

        if event_type == "signal.generated":
            signal_id = payload.get("signal_id")
            if not isinstance(signal_id, str):
                continue

            context = contexts.get(signal_id)
            if context is None:
                context = SignalContext(
                    signal_id=signal_id,
                    aggregate_key=event.get("aggregate_key"),
                    hypothesis_id=linkage.get("hypothesis_id"),
                    correlation_id=linkage.get("correlation_id"),
                    parent_event_id=linkage.get("parent_event_id"),
                    generated_event_id=event.get("event_id"),
                    confirmation_event_id=None,
                )
                contexts[signal_id] = context

            context.aggregate_key = context.aggregate_key or event.get("aggregate_key")
            context.hypothesis_id = context.hypothesis_id or linkage.get("hypothesis_id")
            context.correlation_id = context.correlation_id or linkage.get("correlation_id")
            context.parent_event_id = context.parent_event_id or linkage.get("parent_event_id")
            context.generated_event_id = context.generated_event_id or event.get("event_id")

            probability = parse_probability_from_notes(provenance.get("notes"))
            if probability is None:
                probability = parse_probability_from_rationale(payload.get("rationale"))
            if probability is not None:
                context.probability = probability

        elif event_type == "signal.confirmed":
            signal_id = payload.get("signal_id")
            if not isinstance(signal_id, str):
                continue

            confirmed_signals.add(signal_id)
            context = contexts.get(signal_id)
            if context is None:
                context = SignalContext(
                    signal_id=signal_id,
                    aggregate_key=event.get("aggregate_key"),
                    hypothesis_id=linkage.get("hypothesis_id"),
                    correlation_id=linkage.get("correlation_id"),
                    parent_event_id=linkage.get("parent_event_id"),
                    generated_event_id=None,
                    confirmation_event_id=event.get("event_id"),
                )
                contexts[signal_id] = context

            context.aggregate_key = context.aggregate_key or event.get("aggregate_key")
            context.hypothesis_id = context.hypothesis_id or linkage.get("hypothesis_id")
            context.correlation_id = context.correlation_id or linkage.get("correlation_id")
            context.confirmation_event_id = context.confirmation_event_id or event.get("event_id")

        elif event_type == "veto.raised":
            scope = payload.get("scope")
            target_id = payload.get("target_id")
            veto_id = payload.get("veto_id")
            if scope != "Signal" or not isinstance(target_id, str):
                continue

            vetoed_signal_ids_any.add(target_id)
            if isinstance(veto_id, str):
                vetoed_signal_ids_by_veto_id[veto_id] = target_id

    return contexts, confirmed_signals, vetoed_signal_ids_by_veto_id, vetoed_signal_ids_any


def build_veto_event(
    context: SignalContext, run_id: str, probability_floor: float
) -> dict[str, Any]:
    probability = context.probability
    assert probability is not None

    reason_text = (
        f"confirmed signal vetoed because probability {probability:.6f} "
        f"is below floor {probability_floor:.6f}"
    )
    return {
        "event_id": str(uuid.uuid4()),
        "event_type": "veto.raised",
        "schema_version": "v1",
        "occurred_at": utc_now_rfc3339(),
        "produced_by": PRODUCED_BY,
        "idempotency_key": f"veto.raised:v1:{deterministic_veto_id(context.signal_id)}",
        "aggregate_key": context.aggregate_key,
        "linkage": {
            "hypothesis_id": context.hypothesis_id,
            "signal_id": context.signal_id,
            "decision_id": None,
            "order_id": None,
            "position_id": None,
            "parent_event_id": context.confirmation_event_id or context.generated_event_id,
            "correlation_id": context.correlation_id,
        },
        "provenance": {
            "source_kind": "Runtime",
            "source_ref": None,
            "producer_run_id": run_id,
            "actor": AGENT_ID,
            "trace_id": f"{run_id}:{context.signal_id}",
            "notes": json.dumps(
                {
                    "rule": RULE_NAME,
                    "reason_code": REASON_CODE,
                    "probability": probability,
                    "probability_floor": probability_floor,
                },
                separators=(",", ":"),
                sort_keys=True,
            ),
        },
        "payload": {
            "veto_id": deterministic_veto_id(context.signal_id),
            "scope": "Signal",
            "target_id": context.signal_id,
            "reason_code": REASON_CODE,
            "reason_text": reason_text,
            "raised_by": AGENT_ID,
        },
    }


def append_event(store_path: Path, event: dict[str, Any]) -> None:
    store_path.parent.mkdir(parents=True, exist_ok=True)
    with store_path.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(event, separators=(",", ":")) + "\n")


def main() -> int:
    args = parse_args()
    if not 0.0 <= args.probability_floor <= 1.0:
        raise SystemExit("--probability-floor must be within [0,1]")

    store_path = Path(args.store)
    run_id = execution_run_id()
    events = load_events(store_path)
    contexts, confirmed_signals, vetoed_by_veto_id, vetoed_any = collect_signal_contexts(events)

    inspected = 0
    vetoed = 0
    skipped = 0
    duplicates_detected = 0
    already_vetoed = 0
    missing_probability = 0
    above_floor = 0

    for signal_id in sorted(confirmed_signals):
        inspected += 1
        context = contexts.get(signal_id)
        if context is None:
            skipped += 1
            continue

        veto_id = deterministic_veto_id(signal_id)
        if veto_id in vetoed_by_veto_id:
            duplicates_detected += 1
            skipped += 1
            continue

        if signal_id in vetoed_any:
            already_vetoed += 1
            skipped += 1
            continue

        probability = context.probability
        if probability is None:
            missing_probability += 1
            skipped += 1
            continue

        if probability >= args.probability_floor:
            above_floor += 1
            skipped += 1
            continue

        if not args.dry_run:
            append_event(store_path, build_veto_event(context, run_id, args.probability_floor))
        vetoed += 1

    print(
        json.dumps(
            {
                "actor": AGENT_ID,
                "producer_run_id": run_id,
                "store": str(store_path),
                "dry_run": args.dry_run,
                "probability_floor": args.probability_floor,
                "inspected_signals": inspected,
                "vetoed_signals": vetoed,
                "skipped_signals": skipped,
                "duplicates_detected": duplicates_detected,
                "already_vetoed_elsewhere": already_vetoed,
                "missing_probability": missing_probability,
                "above_floor": above_floor,
            },
            separators=(",", ":"),
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
