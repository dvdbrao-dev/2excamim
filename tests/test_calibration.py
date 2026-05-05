"""Tests for agents/core/calibration.py."""
from __future__ import annotations

import math
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from agents.core.calibration import (
    brier_score,
    expected_calibration_error,
    log_loss,
    reliability_curve,
)


class TestBrierScore:
    def test_brier_score_known_values(self) -> None:
        # Perfect predictions → 0.0
        assert brier_score([1.0, 0.0], [1.0, 0.0]) == 0.0

    def test_brier_score_worst_case(self) -> None:
        # Perfectly wrong predictions → 1.0
        assert brier_score([1.0, 0.0], [0.0, 1.0]) == 1.0

    def test_brier_score_midpoint(self) -> None:
        # 0.5 always → 0.25
        assert brier_score([0.5, 0.5], [1.0, 0.0]) == pytest_approx(0.25)

    def test_brier_score_manual(self) -> None:
        # [(0.8-1)^2 + (0.3-0)^2] / 2 = (0.04 + 0.09) / 2 = 0.065
        result = brier_score([0.8, 0.3], [1.0, 0.0])
        assert abs(result - 0.065) < 1e-9

    def test_brier_score_single_element(self) -> None:
        assert abs(brier_score([0.7], [1.0]) - 0.09) < 1e-9

    def test_brier_score_length_mismatch_raises(self) -> None:
        try:
            brier_score([0.5, 0.5], [1.0])
            assert False, "expected ValueError"
        except ValueError:
            pass

    def test_brier_score_empty_raises(self) -> None:
        try:
            brier_score([], [])
            assert False, "expected ValueError"
        except ValueError:
            pass


class TestLogLoss:
    def test_log_loss_clamps_extreme_probabilities(self) -> None:
        # probability 0 or 1 should NOT raise and should return finite value
        result = log_loss([0.0, 1.0], [1.0, 0.0])
        assert math.isfinite(result)
        assert result > 0

    def test_log_loss_perfect_predictions(self) -> None:
        result = log_loss([1.0, 0.0], [1.0, 0.0], eps=1e-15)
        assert result < 1e-10  # near-zero

    def test_log_loss_manual(self) -> None:
        eps = 1e-9
        p, o = 0.8, 1.0
        expected = -(o * math.log(p) + (1 - o) * math.log(1 - p))
        result = log_loss([p], [o], eps=eps)
        assert abs(result - expected) < 1e-9

    def test_log_loss_empty_raises(self) -> None:
        try:
            log_loss([], [])
            assert False, "expected ValueError"
        except ValueError:
            pass

    def test_log_loss_length_mismatch_raises(self) -> None:
        try:
            log_loss([0.5], [1.0, 0.0])
            assert False, "expected ValueError"
        except ValueError:
            pass


class TestECE:
    def test_ece_perfect_calibration_zero(self) -> None:
        # Each bin: predicted == actual → ECE = 0
        # Build perfectly calibrated data: bin 0.05 → 5% outcomes, bin 0.95 → 95% outcomes
        probs = [0.05] * 100 + [0.95] * 100
        outcomes = [0.0] * 95 + [1.0] * 5 + [0.0] * 5 + [1.0] * 95
        ece = expected_calibration_error(probs, outcomes, n_bins=10)
        # Not exactly 0 due to rounding but should be very small
        assert ece < 0.02

    def test_ece_worst_case(self) -> None:
        # Always predict 1.0 but always outcome 0 → large ECE
        probs = [1.0] * 50
        outcomes = [0.0] * 50
        ece = expected_calibration_error(probs, outcomes, n_bins=10)
        assert ece > 0.5

    def test_ece_nonnegative(self) -> None:
        probs = [0.3, 0.6, 0.2, 0.9]
        outcomes = [0.0, 1.0, 0.0, 1.0]
        assert expected_calibration_error(probs, outcomes) >= 0.0

    def test_ece_empty_raises(self) -> None:
        try:
            expected_calibration_error([], [])
            assert False, "expected ValueError"
        except ValueError:
            pass

    def test_ece_length_mismatch_raises(self) -> None:
        try:
            expected_calibration_error([0.5], [1.0, 0.0])
            assert False, "expected ValueError"
        except ValueError:
            pass


class TestReliabilityCurve:
    def test_reliability_curve_returns_tuples(self) -> None:
        probs = [0.1, 0.5, 0.9, 0.5]
        outcomes = [0.0, 1.0, 1.0, 0.0]
        curve = reliability_curve(probs, outcomes, n_bins=5)
        assert isinstance(curve, list)
        for item in curve:
            assert len(item) == 3
            mean_pred, frac_pos, count = item
            assert 0.0 <= mean_pred <= 1.0
            assert 0.0 <= frac_pos <= 1.0
            assert count > 0

    def test_reliability_curve_length_mismatch_raises(self) -> None:
        try:
            reliability_curve([0.5], [1.0, 0.0])
            assert False, "expected ValueError"
        except ValueError:
            pass


def pytest_approx(val: float, rel: float = 1e-6) -> float:
    """Simple approx helper — returns val; comparison done with abs check in callers."""
    return val
