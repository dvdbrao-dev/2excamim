#!/usr/bin/env python3
"""Read-only bridge that exports 2EXCAMIM state for OpenFang consumption.

Contract:
  - Only reads: registry, scorecard, kill-switch existence.
  - Only writes: runtime/openfang_state.json (and optionally POSTs to OPENFANG_INGEST_URL).
  - Never touches var/events.jsonl or any execution artefact.
  - Never imports live, sizing, confirmation, exit, or veto agents.
"""
from __future__ import annotations

import argparse
import json
import os
import sys
import urllib.error
import urllib.request
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

SCHEMA_VERSION = 1
SYSTEM_ID = "2excamim"
MODE = "paper"

DEFAULT_REGISTRY = Path("./runtime/crypto_strategy_registry.json")
DEFAULT_SCORECARD = Path("./runtime/crypto_strategy_scorecard.json")
DEFAULT_KILL_SWITCH = Path("./var/.kill_switch")
DEFAULT_OUTPUT = Path("./runtime/openfang_state.json")

# Fields to exclude from the metrics dict (they live at the strategy level)
_METRIC_EXCLUDE = frozenset({"strategy_id", "status", "warnings", "agent_file",
                              "created_at", "last_evaluated_at", "promotion_count",
                              "freeze_count", "rejection_reason", "notes"})


def _utc_now() -> str:
    return datetime.now(tz=timezone.utc).isoformat().replace("+00:00", "Z")


def _load_json_list(path: Path, key: str) -> list[dict[str, Any]]:
    if not path.exists():
        return []
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (json.JSONDecodeError, OSError):
        return []
    rows = data.get(key, [])
    return [r for r in rows if isinstance(r, dict)]


def build_state(
    registry_path: Path,
    scorecard_path: Path,
    kill_switch_path: Path,
) -> dict[str, Any]:
    timestamp = _utc_now()
    kill_switch_active = kill_switch_path.exists()

    registry_by_id: dict[str, dict[str, Any]] = {
        r["strategy_id"]: r
        for r in _load_json_list(registry_path, "strategies")
        if isinstance(r.get("strategy_id"), str)
    }
    scorecard_by_id: dict[str, dict[str, Any]] = {
        r["strategy_id"]: r
        for r in _load_json_list(scorecard_path, "strategies")
        if isinstance(r.get("strategy_id"), str)
    }

    all_ids = sorted(set(registry_by_id) | set(scorecard_by_id))
    strategies: list[dict[str, Any]] = []
    alerts: list[dict[str, Any]] = []

    for sid in all_ids:
        reg = registry_by_id.get(sid, {})
        score = scorecard_by_id.get(sid, {})

        status = reg.get("status") or score.get("status") or "candidate"
        last_evaluated = reg.get("last_evaluated_at")
        metrics = {k: v for k, v in score.items() if k not in _METRIC_EXCLUDE}

        strategies.append(
            {
                "id": sid,
                "status": status,
                "metrics": metrics,
                "last_evaluated_at": last_evaluated,
            }
        )

        if status == "frozen":
            alerts.append(
                {"level": "warning", "message": f"Strategy {sid} is frozen", "ts": timestamp}
            )
        elif status == "rejected":
            alerts.append(
                {"level": "critical", "message": f"Strategy {sid} has been rejected", "ts": timestamp}
            )

    if kill_switch_active:
        alerts.append(
            {
                "level": "critical",
                "message": "Kill switch is active — pipeline is paused",
                "ts": timestamp,
            }
        )

    return {
        "schema_version": SCHEMA_VERSION,
        "system_id": SYSTEM_ID,
        "timestamp": timestamp,
        "mode": MODE,
        "kill_switch_active": kill_switch_active,
        "strategies": strategies,
        "alerts": alerts,
    }


def write_state(state: dict[str, Any], output_path: Path) -> None:
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(json.dumps(state, indent=2) + "\n", encoding="utf-8")


def post_state(
    state: dict[str, Any],
    url: str,
    timeout: int = 5,
    max_attempts: int = 2,
) -> bool:
    payload = json.dumps(state, separators=(",", ":")).encode("utf-8")
    for _ in range(max_attempts):
        try:
            req = urllib.request.Request(
                url,
                data=payload,
                method="POST",
                headers={"Content-Type": "application/json"},
            )
            with urllib.request.urlopen(req, timeout=timeout) as resp:
                return resp.status < 400
        except Exception:
            pass
    return False


def run(
    registry_path: Path,
    scorecard_path: Path,
    kill_switch_path: Path,
    output_path: Path,
    ingest_url: str | None = None,
) -> int:
    state = build_state(registry_path, scorecard_path, kill_switch_path)
    write_state(state, output_path)

    if ingest_url:
        ok = post_state(state, ingest_url)
        if not ok:
            print("[openfang_bridge] WARN: POST to ingest URL failed after retries", file=sys.stderr)

    return 0


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Export 2EXCAMIM state to OpenFang (read-only)."
    )
    parser.add_argument("--registry", default=str(DEFAULT_REGISTRY))
    parser.add_argument("--scorecard", default=str(DEFAULT_SCORECARD))
    parser.add_argument("--kill-switch", default=str(DEFAULT_KILL_SWITCH))
    parser.add_argument("--output", default=str(DEFAULT_OUTPUT))
    return parser.parse_args()


def main() -> int:
    args = _parse_args()
    ingest_url = os.environ.get("OPENFANG_INGEST_URL") or None
    return run(
        registry_path=Path(args.registry),
        scorecard_path=Path(args.scorecard),
        kill_switch_path=Path(args.kill_switch),
        output_path=Path(args.output),
        ingest_url=ingest_url,
    )


if __name__ == "__main__":
    sys.exit(main())
