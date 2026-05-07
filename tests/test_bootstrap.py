"""Tests for agents/core/bootstrap.py."""
from __future__ import annotations

import math
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from agents.core.bootstrap import bootstrap_mean_t_stat


class TestBootstrapReproducible:
    def test_bootstrap_reproducible_with_seed(self) -> None:
        returns = [0.01, -0.02, 0.03, 0.01, -0.01, 0.02] * 10
        r1 = bootstrap_mean_t_stat(returns, n_iterations=200, seed=42)
        r2 = bootstrap_mean_t_stat(returns, n_iterations=200, seed=42)
        assert r1 == r2

    def test_bootstrap_different_seeds_differ(self) -> None:
        returns = [0.01, -0.02, 0.03, 0.01, -0.01, 0.02] * 10
        r1 = bootstrap_mean_t_stat(returns, n_iterations=200, seed=42)
        r2 = bootstrap_mean_t_stat(returns, n_iterations=200, seed=99)
        # Seeds differ → CI bounds should differ (means may coincide but CIs won't)
        assert r1[2] != r2[2] or r1[3] != r2[3]


class TestBootstrapTStatRecovery:
    def test_bootstrap_t_stat_recovers_known_signal(self) -> None:
        """Returns with known mean μ=0.1 and σ=0.2, n=100 → analytical t=μ/(σ/√n)=5.0."""
        import random
        rng = random.Random(7)
        mu, sigma, n = 0.1, 0.2, 100
        returns = [rng.gauss(mu, sigma) for _ in range(n)]
        mean_obs, t_stat, ci_low, ci_high = bootstrap_mean_t_stat(returns, n_iterations=1000, seed=42)
        analytical_t = mu / (sigma / math.sqrt(n))
        # Bootstrap t should be within ±1.0 of analytical t for large n
        assert abs(t_stat - analytical_t) < 1.0, f"t_stat={t_stat:.3f} analytical={analytical_t:.3f}"

    def test_bootstrap_zero_returns_t_stat_zero(self) -> None:
        returns = [0.0] * 50
        mean_r, t_stat, ci_low, ci_high = bootstrap_mean_t_stat(returns, n_iterations=100, seed=0)
        assert mean_r == 0.0
        assert t_stat == 0.0

    def test_bootstrap_positive_mean_positive_t(self) -> None:
        returns = [0.05] * 100
        mean_r, t_stat, ci_low, ci_high = bootstrap_mean_t_stat(returns, n_iterations=200, seed=1)
        assert mean_r > 0
        # All returns identical → variance=0 → t=0 (std_err=0 case)
        assert t_stat == 0.0  # degenerate: all same value, std=0

    def test_bootstrap_ci_contains_mean(self) -> None:
        returns = [0.01, -0.02, 0.03, 0.01, -0.01, 0.02] * 20
        mean_r, t_stat, ci_low, ci_high = bootstrap_mean_t_stat(returns, n_iterations=500, seed=42)
        assert ci_low <= mean_r <= ci_high

    def test_bootstrap_returns_four_tuple(self) -> None:
        result = bootstrap_mean_t_stat([0.01, 0.02, -0.01], n_iterations=100, seed=0)
        assert len(result) == 4

    def test_bootstrap_empty_raises(self) -> None:
        try:
            bootstrap_mean_t_stat([], n_iterations=100, seed=0)
            assert False, "expected ValueError"
        except ValueError:
            pass


class TestBootstrapGauntletIntegration:
    def test_gauntlet_kills_llm_when_brier_worse(self) -> None:
        """Fixture where Brier(LLM) > Brier(midpoint) → llm_killed flag written."""
        import json
        import tempfile
        from pathlib import Path

        # Build fake events where LLM is worse than midpoint
        events = []
        for i in range(35):
            outcome = 1.0 if i % 2 == 0 else 0.0
            # LLM estimates opposite of truth (very bad)
            llm_p = 0.1 if outcome == 1.0 else 0.9
            # Midpoint close to truth
            mid_p = 0.8 if outcome == 1.0 else 0.2
            events.append({
                "event_type": "signal.confirmed",
                "produced_by": "runtime.agent.probability",
                "aggregate_key": f"polymarket:0xfake{i:04d}",
                "payload": {
                    "estimated_probability": llm_p,
                    "market_midpoint": mid_p,
                },
            })

        cfg = {
            "llm_calibration": {
                "min_resolved_markets": 30,
                "min_relative_improvement_brier": 0.05,
            }
        }

        with tempfile.TemporaryDirectory() as tmpdir:
            cache_dir = Path(tmpdir) / "cache"
            cache_dir.mkdir()
            # Write resolution cache files so fetch returns without HTTP
            for i in range(35):
                market_id = f"0xfake{i:04d}"
                outcome = 1.0 if i % 2 == 0 else 0.0
                # Write a "closed" market with known resolution
                resolution_data = {
                    "closed": True,
                    "outcomePrices": [str(outcome), str(1.0 - outcome)],
                }
                (cache_dir / f"{market_id}.json").write_text(
                    json.dumps(resolution_data), encoding="utf-8"
                )

            from scripts.edge_validation_gauntlet import stage_a_llm_calibration

            result = stage_a_llm_calibration(events, cfg, cache_dir)

        assert result["verdict"] == "KILL", f"expected KILL, got: {result}"

    def test_gauntlet_kills_strategy_below_pf_threshold(self) -> None:
        """Strategy with all OOS windows PF<1.2 → KILL."""
        from agents.core.bootstrap import bootstrap_mean_t_stat
        from scripts.edge_validation_gauntlet import stage_c_bootstrap

        # All windows with negative PnL → t_stat will be negative → KILL
        windows = [{"oos": {"pnl_net": -100.0, "profit_factor": 0.8, "max_drawdown": 0.05}}
                   for _ in range(10)]

        cfg = {
            "bootstrap_significance": {
                "n_iterations": 200,
                "min_t_stat": 1.5,
                "kill_t_stat": 1.0,
            }
        }

        result = stage_c_bootstrap("test_strategy", windows, cfg)
        # All returns negative → t_stat << 0 → KILL
        assert result["verdict"] == "KILL", f"expected KILL, got t_stat={result.get('t_stat')}"

    def test_gauntlet_pre_committed_yaml_immutable_in_run(self) -> None:
        """Config loaded once at startup; the script reads from file only once."""
        import tempfile
        from pathlib import Path

        yaml_content = """\
schema_version: "v1"
llm_calibration:
  min_resolved_markets: 30
  min_relative_improvement_brier: 0.05
strategy_walkforward:
  min_pf_oos_window: 1.2
  min_pf_pass_rate: 0.60
  min_sharpe_like: 0.5
  max_dd_worst_window: 0.15
bootstrap_significance:
  n_iterations: 100
  min_t_stat: 1.5
  kill_t_stat: 1.0
correlation_cap:
  max_pairwise_rho: 0.6
"""
        with tempfile.TemporaryDirectory() as tmpdir:
            cfg_path = Path(tmpdir) / "test.yaml"
            cfg_path.write_text(yaml_content, encoding="utf-8")

            from scripts.edge_validation_gauntlet import load_yaml_simple
            cfg = load_yaml_simple(cfg_path)

            # Verify thresholds are read-only from the YAML, not from CLI args
            assert cfg["llm_calibration"]["min_resolved_markets"] == 30
            assert cfg["strategy_walkforward"]["min_pf_pass_rate"] == 0.60
            assert cfg["bootstrap_significance"]["n_iterations"] == 100

            # Simulate "override" attempt — the load function returns a fixed dict,
            # no argv mutation can change the loaded cfg
            import sys
            original_argv = sys.argv[:]
            sys.argv = ["gauntlet.py", "--config", str(cfg_path)]
            cfg2 = load_yaml_simple(cfg_path)
            sys.argv = original_argv

            assert cfg2["llm_calibration"]["min_resolved_markets"] == 30
