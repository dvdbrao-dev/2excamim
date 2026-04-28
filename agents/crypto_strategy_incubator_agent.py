#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from agents.core.event_store import load_jsonl

DEFAULT_CONFIG = Path("./config/crypto_strategy_incubator.yaml")
DEFAULT_STORE = Path("./var/events.jsonl")
DEFAULT_REGISTRY = Path("./runtime/crypto_strategy_registry.json")
DEFAULT_SCORECARD = Path("./runtime/crypto_strategy_scorecard.json")


@dataclass(frozen=True)
class IncubatorConfig:
    min_signals_for_shadow: int = 20
    min_signals_for_promotion: int = 100
    min_expectancy_for_promotion: float = 0.0
    min_profit_factor_for_promotion: float = 1.2
    max_drawdown_allowed: float = 0.15
    min_confidence_for_promotion: float = 0.60
    freeze_after_negative_windows: int = 3
    reject_after_failed_runs: int = 5


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Evaluate crypto strategies with a paper/shadow incubator.")
    parser.add_argument("--config", default=str(DEFAULT_CONFIG))
    parser.add_argument("--store", default=str(DEFAULT_STORE))
    parser.add_argument("--registry", default=str(DEFAULT_REGISTRY))
    parser.add_argument("--scorecard", default=str(DEFAULT_SCORECARD))
    parser.add_argument("--json", action="store_true")
    return parser.parse_args()


def utc_now_rfc3339() -> str:
    return datetime.now(tz=timezone.utc).isoformat().replace("+00:00", "Z")


def _as_int(value: Any) -> int | None:
    if isinstance(value, bool):
        return None
    if isinstance(value, int):
        return value
    if isinstance(value, float):
        return int(value)
    if isinstance(value, str):
        try:
            return int(float(value.strip()))
        except ValueError:
            return None
    return None


def _as_float(value: Any) -> float | None:
    if isinstance(value, bool):
        return None
    if isinstance(value, (int, float)):
        return float(value)
    if isinstance(value, str):
        try:
            return float(value.strip())
        except ValueError:
            return None
    return None


def load_incubator_config(path: Path) -> IncubatorConfig:
    if not path.exists():
        return IncubatorConfig()

    raw: dict[str, Any] = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        stripped = line.strip()
        if not stripped or stripped.startswith("#") or ":" not in stripped:
            continue
        key, value = stripped.split(":", 1)
        raw[key.strip()] = value.strip()

    return IncubatorConfig(
        min_signals_for_shadow=_as_int(raw.get("min_signals_for_shadow")) or 20,
        min_signals_for_promotion=_as_int(raw.get("min_signals_for_promotion")) or 100,
        min_expectancy_for_promotion=_as_float(raw.get("min_expectancy_for_promotion")) or 0.0,
        min_profit_factor_for_promotion=_as_float(raw.get("min_profit_factor_for_promotion")) or 1.2,
        max_drawdown_allowed=_as_float(raw.get("max_drawdown_allowed")) or 0.15,
        min_confidence_for_promotion=_as_float(raw.get("min_confidence_for_promotion")) or 0.60,
        freeze_after_negative_windows=_as_int(raw.get("freeze_after_negative_windows")) or 3,
        reject_after_failed_runs=_as_int(raw.get("reject_after_failed_runs")) or 5,
    )


def load_json_object(path: Path) -> tuple[dict[str, Any], str | None]:
    if not path.exists():
        return {}, f"missing_file:{path}"
    try:
        parsed = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return {}, f"invalid_json:{path}"
    if not isinstance(parsed, dict):
        return {}, f"invalid_payload:{path}"
    return parsed, None


def load_registry(path: Path) -> tuple[list[dict[str, Any]], list[str]]:
    warnings: list[str] = []
    payload, warning = load_json_object(path)
    if warning:
        warnings.append(warning)
        return [], warnings

    strategies = payload.get("strategies")
    if not isinstance(strategies, list):
        warnings.append(f"missing_registry_strategies:{path}")
        return [], warnings

    rows = [item for item in strategies if isinstance(item, dict)]
    if len(rows) != len(strategies):
        warnings.append("registry_contains_non_object_rows")
    return rows, warnings


def scorecard_by_strategy(path: Path) -> tuple[dict[str, dict[str, Any]], list[str]]:
    warnings: list[str] = []
    payload, warning = load_json_object(path)
    if warning:
        warnings.append(warning)
        return {}, warnings

    rows = payload.get("strategies")
    if not isinstance(rows, list):
        warnings.append(f"missing_scorecard_strategies:{path}")
        return {}, warnings

    indexed: dict[str, dict[str, Any]] = {}
    for row in rows:
        if not isinstance(row, dict):
            continue
        strategy_id = row.get("strategy_id")
        if isinstance(strategy_id, str) and strategy_id:
            indexed[strategy_id] = row
    return indexed, warnings


def infer_signal_counts(store_path: Path) -> tuple[dict[str, int], str | None]:
    if not store_path.exists():
        return {}, f"missing_file:{store_path}"
    counts: dict[str, int] = {}
    for event in load_jsonl(store_path):
        if event.get("event_type") != "crypto.signal.generated":
            continue
        payload = event.get("payload") if isinstance(event.get("payload"), dict) else {}
        strategy_id = payload.get("strategy_id")
        if isinstance(strategy_id, str) and strategy_id:
            counts[strategy_id] = counts.get(strategy_id, 0) + 1
    return counts, None


def evaluate_status(
    strategy_id: str,
    registry_row: dict[str, Any],
    score: dict[str, Any],
    signal_count_fallback: int,
    config: IncubatorConfig,
) -> tuple[str, dict[str, Any], str | None]:
    signals = _as_int(score.get("signals_generated"))
    if signals is None:
        signals = signal_count_fallback

    expectancy = _as_float(score.get("expectancy"))
    profit_factor = _as_float(score.get("profit_factor"))
    max_drawdown = _as_float(score.get("max_drawdown"))
    confidence = _as_float(score.get("confidence"))
    negative_windows = _as_int(score.get("negative_windows"))
    failed_runs = _as_int(score.get("failed_runs"))

    freeze_count = _as_int(registry_row.get("freeze_count")) or 0
    promotion_count = _as_int(registry_row.get("promotion_count")) or 0

    warning: str | None = None

    if failed_runs is not None and failed_runs >= config.reject_after_failed_runs:
        status = "rejected"
    elif max_drawdown is not None and max_drawdown > config.max_drawdown_allowed:
        status = "frozen"
    elif negative_windows is not None and negative_windows >= config.freeze_after_negative_windows:
        status = "frozen"
    elif (
        signals >= config.min_signals_for_promotion
        and expectancy is not None
        and expectancy >= config.min_expectancy_for_promotion
        and profit_factor is not None
        and profit_factor >= config.min_profit_factor_for_promotion
        and confidence is not None
        and confidence >= config.min_confidence_for_promotion
        and (max_drawdown is None or max_drawdown <= config.max_drawdown_allowed)
    ):
        status = "promoted"
    elif signals >= config.min_signals_for_shadow:
        status = "shadow"
    else:
        status = "candidate"

    if (
        expectancy is None
        or profit_factor is None
        or confidence is None
        or max_drawdown is None
        or negative_windows is None
        or failed_runs is None
    ):
        warning = f"incomplete_metrics:{strategy_id}"

    updated = {
        "strategy_id": strategy_id,
        "agent_file": registry_row.get("agent_file"),
        "status": status,
        "created_at": registry_row.get("created_at"),
        "last_evaluated_at": utc_now_rfc3339(),
        "promotion_count": promotion_count + (1 if status == "promoted" else 0),
        "freeze_count": freeze_count + (1 if status == "frozen" else 0),
        "rejection_reason": (
            "failed_runs_threshold"
            if status == "rejected"
            else registry_row.get("rejection_reason")
        ),
        "notes": registry_row.get("notes"),
    }

    return status, updated, warning


def main() -> int:
    args = parse_args()
    config = load_incubator_config(Path(args.config))
    registry_rows, warnings = load_registry(Path(args.registry))
    scorecards, score_warnings = scorecard_by_strategy(Path(args.scorecard))
    warnings.extend(score_warnings)

    fallback_signals, store_warning = infer_signal_counts(Path(args.store))
    if store_warning:
        warnings.append(store_warning)

    if not registry_rows and scorecards:
        for strategy_id in sorted(scorecards):
            registry_rows.append(
                {
                    "strategy_id": strategy_id,
                    "agent_file": None,
                    "status": "candidate",
                    "created_at": utc_now_rfc3339(),
                    "last_evaluated_at": None,
                    "promotion_count": 0,
                    "freeze_count": 0,
                    "rejection_reason": None,
                    "notes": "autogenerated from scorecard",
                }
            )

    evaluated_strategies: list[dict[str, Any]] = []
    promoted: list[str] = []
    frozen: list[str] = []
    rejected: list[str] = []
    updated_registry: list[dict[str, Any]] = []

    for row in registry_rows:
        strategy_id = row.get("strategy_id")
        if not isinstance(strategy_id, str) or not strategy_id:
            warnings.append("invalid_registry_row_missing_strategy_id")
            continue

        score = scorecards.get(strategy_id, {})
        status, updated, warning = evaluate_status(
            strategy_id,
            row,
            score,
            fallback_signals.get(strategy_id, 0),
            config,
        )

        signal_count = _as_int(score.get("signals_generated")) or fallback_signals.get(strategy_id, 0)
        evaluated_strategies.append(
            {
                "strategy_id": strategy_id,
                "timestamp": utc_now_rfc3339(),
                "symbol": score.get("symbol"),
                "timeframe": score.get("timeframe"),
                "signal_count": signal_count,
                "signals_generated": signal_count,
                "winrate": _as_float(score.get("winrate")),
                "expectancy": _as_float(score.get("expectancy")),
                "avg_return": _as_float(score.get("avg_return")),
                "max_drawdown": _as_float(score.get("max_drawdown")),
                "profit_factor": _as_float(score.get("profit_factor")),
                "sharpe_like": _as_float(score.get("sharpe_like")),
                "recent_performance": _as_float(score.get("recent_performance")),
                "confidence": _as_float(score.get("confidence")),
                "negative_windows": _as_int(score.get("negative_windows")),
                "failed_runs": _as_int(score.get("failed_runs")),
                "status": status,
            }
        )

        if status == "promoted":
            promoted.append(strategy_id)
        elif status == "frozen":
            frozen.append(strategy_id)
        elif status == "rejected":
            rejected.append(strategy_id)

        if warning:
            warnings.append(warning)
        updated_registry.append(updated)

    runtime_registry_path = Path(args.registry)
    runtime_registry_path.parent.mkdir(parents=True, exist_ok=True)
    runtime_registry_path.write_text(
        json.dumps(
            {
                "agent": "crypto_strategy_incubator",
                "mode": "paper_shadow_only",
                "timestamp": utc_now_rfc3339(),
                "strategies": updated_registry,
            },
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )

    output = {
        "agent": "crypto_strategy_incubator",
        "mode": "paper_shadow_only",
        "timestamp": utc_now_rfc3339(),
        "evaluated_strategies": evaluated_strategies,
        "promoted": sorted(promoted),
        "frozen": sorted(frozen),
        "rejected": sorted(rejected),
        "warnings": sorted(set(warnings)),
        "summary": {
            "total": len(evaluated_strategies),
            "promoted": len(promoted),
            "frozen": len(frozen),
            "rejected": len(rejected),
        },
    }

    if args.json:
        print(json.dumps(output, separators=(",", ":")))
    else:
        print(json.dumps(output, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
