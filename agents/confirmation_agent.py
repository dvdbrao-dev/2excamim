#!/usr/bin/env python3
"""Confirmation Agent v1.

Reads the local JSONL store, finds unconfirmed signals with strength >= threshold,
and writes `signal.confirmed` events. Signals are only eligible when their
`aggregate_key` matches a `market.scored` event in the same store. It first tries
the contract CLI command and falls back to a direct JSONL append when the current
runtime does not expose that subcommand yet.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import uuid
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


AGENT_ID = "confirmation-agent-v1"
DEFAULT_STORE = Path("./var/events.jsonl")
DEFAULT_THRESHOLD = 0.6


@dataclass
class SignalCandidate:
    signal_id: str
    strength: float
    aggregate_key: str | None
    hypothesis_id: str | None
    correlation_id: str | None
    parent_event_id: str | None


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Confirm eligible signals from the JSONL store.")
    parser.add_argument("--store", default=str(DEFAULT_STORE), help="Path to the JSONL store.")
    parser.add_argument(
        "--threshold",
        type=float,
        default=DEFAULT_THRESHOLD,
        help="Minimum signal strength to confirm.",
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


def normalized_market_key(value: Any) -> str | None:
    if not isinstance(value, str):
        return None
    trimmed = value.strip()
    if not trimmed:
        return None
    if ":" in trimmed:
        return trimmed.split(":", 1)[1]
    return trimmed


def collect_candidates(
    events: list[dict[str, Any]], threshold: float
) -> tuple[dict[str, SignalCandidate], set[tuple[str, str]], set[str]]:
    generated: dict[str, SignalCandidate] = {}
    confirmed: set[tuple[str, str]] = set()
    scored_markets: set[str] = set()

    for event in events:
        event_type = event.get("event_type")
        payload = event.get("payload") or {}
        linkage = event.get("linkage") or {}

        if event_type == "market.scored":
            market_id = normalized_market_key(payload.get("market_id")) or normalized_market_key(
                event.get("aggregate_key")
            )
            if isinstance(market_id, str):
                scored_markets.add(market_id)

        if event_type == "signal.generated":
            signal_id = payload.get("signal_id")
            strength = payload.get("strength")
            if not isinstance(signal_id, str) or not isinstance(strength, (int, float)):
                continue
            if float(strength) < threshold:
                continue
            generated[signal_id] = SignalCandidate(
                signal_id=signal_id,
                strength=float(strength),
                aggregate_key=event.get("aggregate_key"),
                hypothesis_id=linkage.get("hypothesis_id"),
                correlation_id=linkage.get("correlation_id"),
                parent_event_id=linkage.get("parent_event_id"),
            )
        elif event_type == "signal.confirmed":
            signal_id = payload.get("signal_id")
            confirmed_by = payload.get("confirmed_by")
            if isinstance(signal_id, str) and isinstance(confirmed_by, str):
                confirmed.add((signal_id, confirmed_by))

    return generated, confirmed, scored_markets


def build_signal_confirmed_event(
    candidate: SignalCandidate, run_id: str, threshold: float
) -> dict[str, Any]:
    return {
        "event_id": str(uuid.uuid4()),
        "event_type": "signal.confirmed",
        "schema_version": "v1",
        "occurred_at": utc_now_rfc3339(),
        "produced_by": "runtime.agent.confirmation",
        "idempotency_key": f"signal.confirmed:v1:{candidate.signal_id}:{AGENT_ID}",
        "aggregate_key": candidate.aggregate_key,
        "linkage": {
            "hypothesis_id": candidate.hypothesis_id,
            "signal_id": candidate.signal_id,
            "decision_id": None,
            "order_id": None,
            "position_id": None,
            "parent_event_id": candidate.parent_event_id,
            "correlation_id": candidate.correlation_id,
        },
        "provenance": {
            "source_kind": "Runtime",
            "source_ref": None,
            "producer_run_id": run_id,
            "actor": AGENT_ID,
            "trace_id": f"{run_id}:{candidate.signal_id}",
            "notes": f"auto-confirmed strength={candidate.strength:.6f} threshold={threshold:.6f}",
        },
        "payload": {
            "signal_id": candidate.signal_id,
            "confirmed_by": AGENT_ID,
            "confirmation_reason": f"strength>={threshold}",
            "confirmation_score": candidate.strength,
        },
    }


def confirm_via_cli(store_path: Path, candidate: SignalCandidate) -> bool:
    command = [
        "cargo",
        "run",
        "--",
        "confirm",
        "signal",
        candidate.signal_id,
        "--confirmed-by",
        AGENT_ID,
        "--store",
        str(store_path),
    ]
    result = subprocess.run(
        command,
        cwd=Path(__file__).resolve().parents[1],
        capture_output=True,
        text=True,
    )
    return result.returncode == 0


def append_event(store_path: Path, event: dict[str, Any]) -> None:
    store_path.parent.mkdir(parents=True, exist_ok=True)
    with store_path.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(event, separators=(",", ":")) + "\n")


def main() -> int:
    args = parse_args()
    store_path = Path(args.store)
    run_id = execution_run_id()

    events = load_events(store_path)
    generated, confirmed, scored_markets = collect_candidates(events, args.threshold)

    eligible_candidates = [
        candidate
        for candidate in generated.values()
        if normalized_market_key(candidate.aggregate_key) in scored_markets
    ]
    to_confirm = [
        candidate
        for candidate in eligible_candidates
        if (candidate.signal_id, AGENT_ID) not in confirmed
    ]
    blocked_by_missing_market_score = len(generated) - len(eligible_candidates)

    cli_successes = 0
    appended = 0
    for candidate in to_confirm:
        if confirm_via_cli(store_path, candidate):
            cli_successes += 1
            continue

        append_event(store_path, build_signal_confirmed_event(candidate, run_id, args.threshold))
        appended += 1

    print(
        json.dumps(
                {
                    "actor": AGENT_ID,
                    "producer_run_id": run_id,
                    "store": str(store_path),
                    "threshold": args.threshold,
                    "generated_candidates": len(generated),
                    "market_scored_markets": len(scored_markets),
                    "eligible_candidates": len(eligible_candidates),
                    "blocked_by_missing_market_score": blocked_by_missing_market_score,
                    "already_confirmed": len(eligible_candidates) - len(to_confirm),
                    "confirmed_via_cli": cli_successes,
                    "confirmed_via_direct_append": appended,
                    "total_confirmed_this_run": cli_successes + appended,
                },
                separators=(",", ":"),
        )
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
