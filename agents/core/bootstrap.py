"""Bootstrap significance testing for return series. Stdlib only."""
from __future__ import annotations

import math
import random
from typing import Sequence


def bootstrap_mean_t_stat(
    returns: Sequence[float],
    n_iterations: int = 1000,
    seed: int = 42,
) -> tuple[float, float, float, float]:
    """Bootstrap the mean and t-stat of a return series.

    Returns (mean, t_stat, ci_low, ci_high) where the CI is the 5th/95th
    percentile of bootstrapped means (90% interval).
    """
    if not returns:
        raise ValueError("returns must be non-empty")
    n = len(returns)
    rng = random.Random(seed)
    returns_list = list(returns)

    mean_obs = sum(returns_list) / n
    variance = sum((r - mean_obs) ** 2 for r in returns_list) / max(n - 1, 1)
    std_err = math.sqrt(variance / n) if variance > 0 else 0.0
    t_stat = mean_obs / std_err if std_err > 0 else 0.0

    boot_means: list[float] = []
    for _ in range(n_iterations):
        sample = [rng.choice(returns_list) for _ in range(n)]
        boot_means.append(sum(sample) / n)

    boot_means.sort()
    ci_low = boot_means[int(0.05 * n_iterations)]
    ci_high = boot_means[int(0.95 * n_iterations)]

    return mean_obs, t_stat, ci_low, ci_high
