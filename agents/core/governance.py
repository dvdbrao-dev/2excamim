from __future__ import annotations

import dataclasses
import json
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

EVALUATOR_VERSION = "v1"
TERMINAL_STATES = frozenset({"rejected"})


@dataclass(frozen=True)
class GovernanceConfig:
    min_signals_for_shadow: int = 20
    min_signals_for_promotion: int = 100
    min_fills_for_promotion: int = 30
    min_profit_factor_for_promotion: float = 1.2
    max_drawdown_allowed: float = 0.15
    min_confidence_for_promotion: float = 0.60
    min_expectancy_for_promotion: float = 0.0
    freeze_after_negative_windows: int = 3
    reject_after_failed_runs: int = 5
    reject_after_freeze_count: int = 3
    recovery_days_for_unfreeze: int = 14


@dataclass(frozen=True)
class StrategyMetrics:
    signals_generated: int | None = None
    fills: int | None = None
    expectancy: float | None = None
    profit_factor: float | None = None
    max_drawdown: float | None = None
    confidence: float | None = None
    negative_windows: int | None = None
    failed_runs: int | None = None
    # Time-series context set by callers, not by the scorecard
    consecutive_positive_expectancy_days: int | None = None
    freeze_count_30d: int | None = None


def _promotion_eligible(metrics: StrategyMetrics, config: GovernanceConfig) -> bool:
    if metrics.signals_generated is None or metrics.signals_generated < config.min_signals_for_promotion:
        return False
    if metrics.fills is None or metrics.fills < config.min_fills_for_promotion:
        return False
    # profit_factor=None means incomplete data (not infinite); block promotion
    if metrics.profit_factor is None:
        return False
    if metrics.profit_factor < config.min_profit_factor_for_promotion:
        return False
    if metrics.max_drawdown is not None and metrics.max_drawdown > config.max_drawdown_allowed:
        return False
    # confidence=None means incomplete data; block promotion
    if metrics.confidence is None:
        return False
    if metrics.confidence < config.min_confidence_for_promotion:
        return False
    if metrics.expectancy is None or metrics.expectancy <= config.min_expectancy_for_promotion:
        return False
    return True


def evaluate_transition(
    current_state: str,
    metrics: StrategyMetrics,
    config: GovernanceConfig,
) -> tuple[str, str]:
    """Pure function: returns (new_state, reason).

    Priority order:
      1. Terminal state (rejected stays rejected)
      2. Rejection triggers (failed_runs / repeated_freezes)
      3. Freeze triggers (negative_windows / max_drawdown_breach)
      4. Recovery from frozen state
      5. Promotion eligibility
      6. Candidate → shadow advancement
    """
    # 1. Terminal
    if current_state in TERMINAL_STATES:
        return "rejected", "terminal_state"

    # 2. Rejection
    if metrics.failed_runs is not None and metrics.failed_runs >= config.reject_after_failed_runs:
        return "rejected", "failed_runs_threshold"
    if (
        metrics.freeze_count_30d is not None
        and metrics.freeze_count_30d >= config.reject_after_freeze_count
    ):
        return "rejected", "repeated_freezes"

    # 3. Freeze
    if (
        metrics.negative_windows is not None
        and metrics.negative_windows >= config.freeze_after_negative_windows
    ):
        return "frozen", "negative_windows_threshold"
    if metrics.max_drawdown is not None and metrics.max_drawdown > config.max_drawdown_allowed:
        return "frozen", "max_drawdown_breach"

    # 4. Recovery from frozen
    if current_state == "frozen":
        if (
            metrics.expectancy is not None
            and metrics.expectancy > 0
            and metrics.consecutive_positive_expectancy_days is not None
            and metrics.consecutive_positive_expectancy_days >= config.recovery_days_for_unfreeze
        ):
            return "shadow", "recovery_from_frozen"
        return "frozen", "insufficient_recovery"

    # 5. Promotion eligibility (from any active state)
    if _promotion_eligible(metrics, config):
        return "promoted", "all_thresholds_met"

    # 6. Candidate → shadow
    if current_state == "candidate":
        if (
            metrics.signals_generated is not None
            and metrics.signals_generated >= config.min_signals_for_shadow
        ):
            return "shadow", "signals_threshold_met"
        return "candidate", "insufficient_signals"

    return current_state, "no_transition"


def append_transition(
    history_path: Path,
    strategy_id: str,
    from_state: str,
    to_state: str,
    reason: str,
    metrics_snapshot: dict[str, Any],
) -> None:
    record = {
        "timestamp": datetime.now(tz=timezone.utc).isoformat().replace("+00:00", "Z"),
        "strategy_id": strategy_id,
        "from_state": from_state,
        "to_state": to_state,
        "reason": reason,
        "metrics_snapshot": metrics_snapshot,
        "evaluator_version": EVALUATOR_VERSION,
    }
    history_path.parent.mkdir(parents=True, exist_ok=True)
    with history_path.open("a", encoding="utf-8") as fh:
        fh.write(json.dumps(record, separators=(",", ":")) + "\n")
