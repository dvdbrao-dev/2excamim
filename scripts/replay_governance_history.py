#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

DEFAULT_HISTORY = Path("./runtime/strategy_governance_history.jsonl")


def load_history(history_path: Path) -> list[dict[str, Any]]:
    records: list[dict[str, Any]] = []
    if not history_path.exists():
        return records
    with history_path.open(encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            try:
                records.append(json.loads(line))
            except json.JSONDecodeError:
                pass
    return records


def reconstruct_states(history_path: Path) -> dict[str, str]:
    """Return {strategy_id: current_state} by replaying the full history."""
    states: dict[str, str] = {}
    for record in load_history(history_path):
        strategy_id = record.get("strategy_id")
        to_state = record.get("to_state")
        if isinstance(strategy_id, str) and isinstance(to_state, str):
            states[strategy_id] = to_state
    return states


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Replay strategy governance history.")
    parser.add_argument("--history", default=str(DEFAULT_HISTORY), help="Path to governance JSONL history")
    parser.add_argument("--registry", help="Path to registry JSON for state comparison")
    parser.add_argument("--json", action="store_true", help="Compact JSON output")
    return parser.parse_args()


def main() -> int:
    args = _parse_args()
    history_path = Path(args.history)
    records = load_history(history_path)
    states = reconstruct_states(history_path)

    discrepancies: list[dict[str, Any]] = []
    registry_states: dict[str, str] = {}

    if args.registry:
        registry_path = Path(args.registry)
        if registry_path.exists():
            try:
                payload = json.loads(registry_path.read_text(encoding="utf-8"))
            except json.JSONDecodeError:
                payload = {}
            for row in payload.get("strategies", []):
                if isinstance(row, dict) and isinstance(row.get("strategy_id"), str):
                    registry_states[row["strategy_id"]] = row.get("status", "unknown")

            for sid, history_state in states.items():
                reg_state = registry_states.get(sid)
                if reg_state and reg_state != history_state:
                    discrepancies.append(
                        {
                            "strategy_id": sid,
                            "history_state": history_state,
                            "registry_state": reg_state,
                        }
                    )

    output = {
        "total_events": len(records),
        "strategies": len(states),
        "current_states": states,
        "discrepancies": discrepancies,
        "history_path": str(history_path),
    }

    if args.json:
        print(json.dumps(output, separators=(",", ":")))
    else:
        print(json.dumps(output, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
