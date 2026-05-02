from __future__ import annotations

import dataclasses
import json
from pathlib import Path

import pytest

from agents.core.governance import (
    GovernanceConfig,
    StrategyMetrics,
    append_transition,
    evaluate_transition,
)
from scripts.replay_governance_history import reconstruct_states

# ---------------------------------------------------------------------------
# Fixture: a metrics snapshot that satisfies every promotion threshold
# ---------------------------------------------------------------------------

_FULL_METRICS = StrategyMetrics(
    signals_generated=100,
    fills=30,
    profit_factor=1.5,
    max_drawdown=0.10,
    confidence=0.70,
    expectancy=5.0,
    negative_windows=0,
    failed_runs=0,
)


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------


def test_promotion_requires_all_thresholds() -> None:
    config = GovernanceConfig()

    # Baseline: all thresholds met → promoted
    state, reason = evaluate_transition("shadow", _FULL_METRICS, config)
    assert state == "promoted"
    assert reason == "all_thresholds_met"

    # Insufficient fills → stays in shadow
    m = dataclasses.replace(_FULL_METRICS, fills=10)
    state, _ = evaluate_transition("shadow", m, config)
    assert state == "shadow"

    # profit_factor below threshold → stays in shadow
    m = dataclasses.replace(_FULL_METRICS, profit_factor=1.0)
    state, _ = evaluate_transition("shadow", m, config)
    assert state == "shadow"

    # Not enough signals → stays in shadow
    m = dataclasses.replace(_FULL_METRICS, signals_generated=50)
    state, _ = evaluate_transition("shadow", m, config)
    assert state == "shadow"

    # confidence below threshold → stays in shadow
    m = dataclasses.replace(_FULL_METRICS, confidence=0.50)
    state, _ = evaluate_transition("shadow", m, config)
    assert state == "shadow"

    # expectancy at or below zero → stays in shadow
    m = dataclasses.replace(_FULL_METRICS, expectancy=0.0)
    state, _ = evaluate_transition("shadow", m, config)
    assert state == "shadow"


def test_freeze_after_three_negative_windows() -> None:
    config = GovernanceConfig()

    metrics = StrategyMetrics(negative_windows=3)
    state, reason = evaluate_transition("shadow", metrics, config)
    assert state == "frozen"
    assert reason == "negative_windows_threshold"

    # Also fires from candidate and promoted
    state2, _ = evaluate_transition("candidate", metrics, config)
    assert state2 == "frozen"

    state3, _ = evaluate_transition("promoted", metrics, config)
    assert state3 == "frozen"

    # Two negative windows is not enough
    metrics_ok = StrategyMetrics(negative_windows=2)
    state_ok, _ = evaluate_transition("shadow", metrics_ok, config)
    assert state_ok != "frozen"


def test_freeze_after_max_drawdown_breach() -> None:
    config = GovernanceConfig()

    # Strict breach → frozen
    metrics = StrategyMetrics(max_drawdown=0.20)
    state, reason = evaluate_transition("shadow", metrics, config)
    assert state == "frozen"
    assert reason == "max_drawdown_breach"

    # Exactly at threshold is NOT a breach (strict greater-than)
    metrics_at = StrategyMetrics(max_drawdown=0.15)
    state_at, _ = evaluate_transition("shadow", metrics_at, config)
    assert state_at != "frozen"

    # Rejection takes priority over freeze when both apply
    both = StrategyMetrics(max_drawdown=0.20, failed_runs=5)
    state_both, reason_both = evaluate_transition("shadow", both, config)
    assert state_both == "rejected"
    assert reason_both == "failed_runs_threshold"


def test_rejection_after_five_failed_runs() -> None:
    config = GovernanceConfig()
    metrics = StrategyMetrics(failed_runs=5)

    for current in ("candidate", "shadow", "promoted", "frozen"):
        state, reason = evaluate_transition(current, metrics, config)
        assert state == "rejected", f"expected rejected from {current}"
        assert reason == "failed_runs_threshold"

    # Four failed runs is not enough
    metrics_4 = StrategyMetrics(failed_runs=4)
    state_4, _ = evaluate_transition("shadow", metrics_4, config)
    assert state_4 != "rejected"

    # Already rejected stays rejected regardless
    state_r, reason_r = evaluate_transition("rejected", StrategyMetrics(), config)
    assert state_r == "rejected"
    assert reason_r == "terminal_state"


def test_transition_history_persisted(tmp_path: Path) -> None:
    history = tmp_path / "gov.jsonl"
    snapshot = dataclasses.asdict(_FULL_METRICS)

    append_transition(history, "strat_x", "shadow", "promoted", "all_thresholds_met", snapshot)
    append_transition(history, "strat_x", "promoted", "frozen", "max_drawdown_breach", {})

    assert history.exists()
    lines = [ln for ln in history.read_text(encoding="utf-8").splitlines() if ln.strip()]
    assert len(lines) == 2

    r0 = json.loads(lines[0])
    assert r0["strategy_id"] == "strat_x"
    assert r0["from_state"] == "shadow"
    assert r0["to_state"] == "promoted"
    assert r0["reason"] == "all_thresholds_met"
    assert r0["evaluator_version"] == "v1"
    assert "timestamp" in r0
    assert "metrics_snapshot" in r0

    r1 = json.loads(lines[1])
    assert r1["from_state"] == "promoted"
    assert r1["to_state"] == "frozen"


def test_no_promotion_when_confidence_none() -> None:
    config = GovernanceConfig()

    # confidence=None must block promotion unconditionally
    metrics = dataclasses.replace(_FULL_METRICS, confidence=None)

    state_shadow, _ = evaluate_transition("shadow", metrics, config)
    assert state_shadow != "promoted"

    state_candidate, _ = evaluate_transition("candidate", metrics, config)
    assert state_candidate != "promoted"


def test_history_replay_reconstructs_current_state(tmp_path: Path) -> None:
    history = tmp_path / "gov_history.jsonl"

    # strat_a: candidate → shadow → promoted
    append_transition(history, "strat_a", "candidate", "shadow", "signals_threshold_met", {})
    append_transition(history, "strat_a", "shadow", "promoted", "all_thresholds_met", {})

    # strat_b: candidate → frozen → rejected
    append_transition(history, "strat_b", "candidate", "frozen", "max_drawdown_breach", {})
    append_transition(history, "strat_b", "frozen", "rejected", "failed_runs_threshold", {})

    states = reconstruct_states(history)

    assert states["strat_a"] == "promoted"
    assert states["strat_b"] == "rejected"
    assert len(states) == 2
