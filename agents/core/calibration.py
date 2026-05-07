"""Calibration metrics for probability forecasts. Stdlib only."""
from __future__ import annotations

import math
from typing import Sequence


def brier_score(probabilities: Sequence[float], outcomes: Sequence[float]) -> float:
    """Mean squared error between probabilities and binary outcomes."""
    if len(probabilities) != len(outcomes):
        raise ValueError("probabilities and outcomes must have the same length")
    if not probabilities:
        raise ValueError("empty sequences")
    return sum((p - o) ** 2 for p, o in zip(probabilities, outcomes)) / len(probabilities)


def log_loss(
    probabilities: Sequence[float],
    outcomes: Sequence[float],
    eps: float = 1e-9,
) -> float:
    """Binary cross-entropy. Clamps probabilities to [eps, 1-eps] to avoid log(0)."""
    if len(probabilities) != len(outcomes):
        raise ValueError("probabilities and outcomes must have the same length")
    if not probabilities:
        raise ValueError("empty sequences")
    total = 0.0
    for p, o in zip(probabilities, outcomes):
        p_clamp = max(eps, min(1.0 - eps, p))
        total += o * math.log(p_clamp) + (1.0 - o) * math.log(1.0 - p_clamp)
    return -total / len(probabilities)


def expected_calibration_error(
    probabilities: Sequence[float],
    outcomes: Sequence[float],
    n_bins: int = 10,
) -> float:
    """ECE: weighted mean absolute calibration error across equal-width bins."""
    if len(probabilities) != len(outcomes):
        raise ValueError("probabilities and outcomes must have the same length")
    if not probabilities:
        raise ValueError("empty sequences")
    n = len(probabilities)
    bins: list[list[tuple[float, float]]] = [[] for _ in range(n_bins)]
    for p, o in zip(probabilities, outcomes):
        idx = min(int(p * n_bins), n_bins - 1)
        bins[idx].append((p, o))
    ece = 0.0
    for bucket in bins:
        if not bucket:
            continue
        mean_p = sum(x[0] for x in bucket) / len(bucket)
        mean_o = sum(x[1] for x in bucket) / len(bucket)
        ece += (len(bucket) / n) * abs(mean_p - mean_o)
    return ece


def reliability_curve(
    probabilities: Sequence[float],
    outcomes: Sequence[float],
    n_bins: int = 10,
) -> list[tuple[float, float, int]]:
    """Returns list of (mean_predicted, fraction_positive, count) per bin."""
    if len(probabilities) != len(outcomes):
        raise ValueError("probabilities and outcomes must have the same length")
    bins: list[list[tuple[float, float]]] = [[] for _ in range(n_bins)]
    for p, o in zip(probabilities, outcomes):
        idx = min(int(p * n_bins), n_bins - 1)
        bins[idx].append((p, o))
    result: list[tuple[float, float, int]] = []
    for bucket in bins:
        if not bucket:
            continue
        mean_p = sum(x[0] for x in bucket) / len(bucket)
        mean_o = sum(x[1] for x in bucket) / len(bucket)
        result.append((mean_p, mean_o, len(bucket)))
    return result
