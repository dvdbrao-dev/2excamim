#!/usr/bin/env python3
"""Edge validation gauntlet — runs 4 validation stages against pre-committed thresholds.

Usage:
    python scripts/edge_validation_gauntlet.py [--store PATH] [--config PATH]
                                               [--candles-dir PATH] [--dry-run]

The YAML config is read ONCE at startup; no CLI override of individual thresholds.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import sys
import urllib.request
import urllib.error
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from agents.core.calibration import brier_score, reliability_curve
from agents.core.bootstrap import bootstrap_mean_t_stat

# ---------------------------------------------------------------------------
# Config loading — single read at startup, no override
# ---------------------------------------------------------------------------

def load_yaml_simple(path: Path) -> dict[str, Any]:
    """Minimal YAML parser (key: value, nested via indentation). No pyyaml needed."""
    result: dict[str, Any] = {}
    # Root is at virtual indent -1 so any real indent (≥0) stays above it
    stack: list[tuple[int, dict[str, Any]]] = [(-1, result)]

    for raw_line in path.read_text(encoding="utf-8").splitlines():
        # Strip comments
        if "#" in raw_line:
            raw_line = raw_line[: raw_line.index("#")]
        line = raw_line.rstrip()
        if not line.strip():
            continue

        indent = len(line) - len(line.lstrip())
        stripped = line.strip()

        if ":" not in stripped:
            continue

        key, _, val = stripped.partition(":")
        key = key.strip()
        val = val.strip()

        # Pop stack: remove frames whose indent >= current key's indent
        # (they are siblings or deeper than us — our parent must be shallower)
        while len(stack) > 1 and stack[-1][0] >= indent:
            stack.pop()

        parent = stack[-1][1]

        if not val or val.startswith("#"):
            # Nested dict — push at this key's indent level
            new_dict: dict[str, Any] = {}
            parent[key] = new_dict
            stack.append((indent, new_dict))
        else:
            # Scalar — coerce types
            if val.lower() in ("true", "yes"):
                parent[key] = True
            elif val.lower() in ("false", "no"):
                parent[key] = False
            else:
                try:
                    parent[key] = int(val)
                except ValueError:
                    try:
                        parent[key] = float(val)
                    except ValueError:
                        parent[key] = val.strip('"').strip("'")

    return result


# ---------------------------------------------------------------------------
# Data helpers
# ---------------------------------------------------------------------------

def load_events(store_path: Path) -> list[dict[str, Any]]:
    if not store_path.exists():
        return []
    events: list[dict[str, Any]] = []
    for line in store_path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            events.append(json.loads(line))
        except json.JSONDecodeError:
            pass
    return events


def fetch_market_resolution(market_id: str, cache_dir: Path) -> float | None:
    """Return 1.0 if YES resolved, 0.0 if NO resolved, None if unresolved/error."""
    cache_file = cache_dir / f"{market_id}.json"
    if cache_file.exists():
        try:
            data = json.loads(cache_file.read_text(encoding="utf-8"))
            return _parse_resolution(data)
        except (json.JSONDecodeError, KeyError):
            pass

    url = f"https://gamma-api.polymarket.com/markets/{market_id}"
    try:
        req = urllib.request.Request(url, headers={"User-Agent": "edge-gauntlet/1.0"})
        with urllib.request.urlopen(req, timeout=10) as resp:
            data = json.loads(resp.read().decode("utf-8"))
        cache_dir.mkdir(parents=True, exist_ok=True)
        cache_file.write_text(json.dumps(data), encoding="utf-8")
        return _parse_resolution(data)
    except (urllib.error.URLError, json.JSONDecodeError, OSError):
        return None


def _parse_resolution(data: dict[str, Any]) -> float | None:
    closed = data.get("closed") or data.get("active") is False
    if not closed:
        return None
    outcome = data.get("outcome") or data.get("winner") or data.get("resolution")
    if isinstance(outcome, str):
        if outcome.lower() in ("yes", "true", "1"):
            return 1.0
        if outcome.lower() in ("no", "false", "0"):
            return 0.0
    resolved_yes = data.get("outcomePrices") or []
    if isinstance(resolved_yes, list) and len(resolved_yes) >= 2:
        try:
            yes_price = float(resolved_yes[0])
            if yes_price >= 0.95:
                return 1.0
            if yes_price <= 0.05:
                return 0.0
        except (ValueError, TypeError):
            pass
    return None


def _market_id_from_event(event: dict[str, Any]) -> str | None:
    payload = event.get("payload") or {}
    mid = payload.get("market_id")
    if mid:
        return mid
    agg = event.get("aggregate_key", "")
    if isinstance(agg, str) and agg.startswith("polymarket:"):
        return agg[len("polymarket:"):]
    return None


# ---------------------------------------------------------------------------
# Stage A: LLM calibration
# ---------------------------------------------------------------------------

def stage_a_llm_calibration(
    events: list[dict[str, Any]],
    cfg: dict[str, Any],
    cache_dir: Path,
) -> dict[str, Any]:
    """Compare Brier(LLM) vs Brier(midpoint) on resolved markets."""
    llm_cfg = cfg["llm_calibration"]
    min_markets: int = llm_cfg["min_resolved_markets"]
    min_improvement: float = llm_cfg["min_relative_improvement_brier"]

    # Collect probability-agent confirmed events that have estimated_probability
    prob_events = [
        e for e in events
        if e.get("event_type") == "signal.confirmed"
        and e.get("produced_by") == "runtime.agent.probability"
    ]

    llm_probs: list[float] = []
    mid_probs: list[float] = []
    outcomes: list[float] = []
    skipped = 0

    for event in prob_events:
        payload = event.get("payload") or {}
        est_p = payload.get("estimated_probability")
        midpoint = payload.get("market_midpoint")
        if est_p is None or midpoint is None:
            skipped += 1
            continue

        market_id = _market_id_from_event(event)
        if not market_id:
            skipped += 1
            continue

        resolution = fetch_market_resolution(market_id, cache_dir)
        if resolution is None:
            skipped += 1
            continue

        llm_probs.append(float(est_p))
        mid_probs.append(float(midpoint))
        outcomes.append(resolution)

    n_resolved = len(outcomes)
    if n_resolved < min_markets:
        return {
            "stage": "llm_calibration",
            "verdict": "INCONCLUSIVE",
            "reason": f"only {n_resolved} resolved markets (need {min_markets})",
            "n_resolved": n_resolved,
            "skipped": skipped,
        }

    brier_llm = brier_score(llm_probs, outcomes)
    brier_mid = brier_score(mid_probs, outcomes)
    relative_improvement = (brier_mid - brier_llm) / brier_mid if brier_mid > 0 else 0.0
    curve = reliability_curve(llm_probs, outcomes)
    ascii_curve = _ascii_reliability(curve)

    verdict = "PASS" if relative_improvement >= min_improvement else "KILL"

    return {
        "stage": "llm_calibration",
        "verdict": verdict,
        "n_resolved": n_resolved,
        "skipped": skipped,
        "brier_llm": round(brier_llm, 6),
        "brier_midpoint": round(brier_mid, 6),
        "relative_improvement": round(relative_improvement, 6),
        "threshold_min_improvement": min_improvement,
        "reliability_curve": curve,
        "reliability_curve_ascii": ascii_curve,
    }


def _ascii_reliability(curve: list[tuple[float, float, int]]) -> str:
    """ASCII art reliability diagram (predicted vs actual)."""
    if not curve:
        return "(no data)"
    rows = ["  pred  | actual | count | bar"]
    rows.append("  -------+--------+-------+----")
    for pred, actual, count in curve:
        bar = "#" * min(int(actual * 20), 20)
        perfect = "·" * min(int(pred * 20), 20)
        rows.append(f"  {pred:.2f}  | {actual:.2f}  | {count:5d} | {bar} (perfect:{perfect})")
    return "\n".join(rows)


# ---------------------------------------------------------------------------
# Stage B: Strategy walk-forward
# ---------------------------------------------------------------------------

def stage_b_walkforward(
    strategy_ids: list[str],
    candles_dir: Path,
    cfg: dict[str, Any],
) -> list[dict[str, Any]]:
    """Run walk-forward per strategy; return verdict per strategy."""
    from scripts.ohlcv_backtester import run_walk_forward, load_candles_jsonl, STRATEGY_MODULES

    wf_cfg = cfg["strategy_walkforward"]
    min_pf = wf_cfg["min_pf_oos_window"]
    min_pass_rate = wf_cfg["min_pf_pass_rate"]
    min_sharpe = wf_cfg["min_sharpe_like"]
    max_dd_allowed = wf_cfg["max_dd_worst_window"]

    results = []
    for sid in strategy_ids:
        candle_files = list(candles_dir.glob(f"{sid}*.jsonl"))
        if not candle_files:
            results.append({
                "strategy_id": sid,
                "verdict": "INCONCLUSIVE",
                "reason": f"no candle file in {candles_dir} matching {sid}*.jsonl",
            })
            continue

        candle_path = sorted(candle_files)[-1]
        candles = load_candles_jsonl(candle_path)

        wf = run_walk_forward(sid, candles, train_months=6, test_months=1)

        windows = wf.get("windows", [])
        oos_total = wf.get("oos_windows_total", 0)

        if oos_total < 2:
            results.append({
                "strategy_id": sid,
                "verdict": "INCONCLUSIVE",
                "reason": f"only {oos_total} OOS windows",
                "walkforward": wf,
            })
            continue

        # PF pass rate (windows where PF >= min_pf_oos_window)
        pf_pass = sum(
            1 for w in windows
            if (w["oos"].get("profit_factor") or 0) >= min_pf
        )
        pass_rate = pf_pass / oos_total

        # Sharpe-like: mean(net_pnl_per_window) / std(net_pnl_per_window)
        oos_pnls = [w["oos"].get("pnl_net", 0.0) for w in windows]
        mean_pnl = sum(oos_pnls) / len(oos_pnls)
        variance_pnl = sum((x - mean_pnl) ** 2 for x in oos_pnls) / max(len(oos_pnls) - 1, 1)
        import math
        std_pnl = math.sqrt(variance_pnl) if variance_pnl > 0 else 0.0
        sharpe_like = mean_pnl / std_pnl if std_pnl > 0 else 0.0

        # Max DD worst window
        worst_dd = max((w["oos"].get("max_drawdown", 0.0) for w in windows), default=0.0)

        kills: list[str] = []
        if pass_rate < min_pass_rate:
            kills.append(f"pf_pass_rate={pass_rate:.2f}<{min_pass_rate}")
        if sharpe_like < min_sharpe:
            kills.append(f"sharpe_like={sharpe_like:.2f}<{min_sharpe}")
        if worst_dd > max_dd_allowed:
            kills.append(f"max_dd={worst_dd:.2f}>{max_dd_allowed}")

        verdict = "KILL" if kills else "PASS"

        results.append({
            "strategy_id": sid,
            "verdict": verdict,
            "kill_reasons": kills,
            "oos_windows_total": oos_total,
            "pf_pass_windows": pf_pass,
            "pf_pass_rate": round(pass_rate, 4),
            "sharpe_like": round(sharpe_like, 4),
            "worst_window_dd": round(worst_dd, 4),
            "threshold_pf_pass_rate": min_pass_rate,
            "threshold_sharpe_like": min_sharpe,
            "threshold_max_dd": max_dd_allowed,
            "walkforward": wf,
        })

    return results


# ---------------------------------------------------------------------------
# Stage C: Bootstrap significance
# ---------------------------------------------------------------------------

def stage_c_bootstrap(
    strategy_id: str,
    windows: list[dict[str, Any]],
    cfg: dict[str, Any],
) -> dict[str, Any]:
    bs_cfg = cfg["bootstrap_significance"]
    n_iter: int = bs_cfg["n_iterations"]
    min_t: float = bs_cfg["min_t_stat"]
    kill_t: float = bs_cfg["kill_t_stat"]

    oos_returns = [w["oos"].get("pnl_net", 0.0) for w in windows]
    if not oos_returns:
        return {"strategy_id": strategy_id, "verdict": "INCONCLUSIVE", "reason": "no OOS returns"}

    mean_r, t_stat, ci_low, ci_high = bootstrap_mean_t_stat(oos_returns, n_iterations=n_iter)

    if t_stat >= min_t:
        verdict = "PASS"
    elif t_stat <= kill_t:
        verdict = "KILL"
    else:
        verdict = "INCONCLUSIVE"

    return {
        "strategy_id": strategy_id,
        "verdict": verdict,
        "mean_return": round(mean_r, 6),
        "t_stat": round(t_stat, 4),
        "ci_low_90": round(ci_low, 6),
        "ci_high_90": round(ci_high, 6),
        "n_iterations": n_iter,
        "threshold_min_t_stat": min_t,
        "threshold_kill_t_stat": kill_t,
    }


# ---------------------------------------------------------------------------
# Stage D: Correlation
# ---------------------------------------------------------------------------

def stage_d_correlation(
    strategy_results: list[dict[str, Any]],
    cfg: dict[str, Any],
) -> dict[str, Any]:
    import math
    corr_cfg = cfg["correlation_cap"]
    max_rho: float = corr_cfg["max_pairwise_rho"]

    # Collect OOS return series per strategy
    series: dict[str, list[float]] = {}
    for sr in strategy_results:
        sid = sr["strategy_id"]
        wf = sr.get("walkforward")
        if wf and "windows" in wf:
            series[sid] = [w["oos"].get("pnl_net", 0.0) for w in wf["windows"]]

    sids = list(series.keys())
    flags: list[dict[str, Any]] = []
    matrix: dict[str, dict[str, float]] = {}

    for i, sid_a in enumerate(sids):
        matrix[sid_a] = {}
        for j, sid_b in enumerate(sids):
            if i == j:
                matrix[sid_a][sid_b] = 1.0
                continue
            a = series[sid_a]
            b = series[sid_b]
            n = min(len(a), len(b))
            if n < 2:
                matrix[sid_a][sid_b] = 0.0
                continue
            a, b = a[:n], b[:n]
            mean_a = sum(a) / n
            mean_b = sum(b) / n
            cov = sum((a[k] - mean_a) * (b[k] - mean_b) for k in range(n)) / (n - 1)
            std_a = math.sqrt(sum((x - mean_a) ** 2 for x in a) / (n - 1)) if n > 1 else 0.0
            std_b = math.sqrt(sum((x - mean_b) ** 2 for x in b) / (n - 1)) if n > 1 else 0.0
            rho = cov / (std_a * std_b) if std_a > 0 and std_b > 0 else 0.0
            matrix[sid_a][sid_b] = round(rho, 4)
            if i < j and abs(rho) > max_rho:
                flags.append({"strategy_a": sid_a, "strategy_b": sid_b, "rho": round(rho, 4)})

    return {
        "stage": "correlation",
        "verdict": "CONSOLIDATE" if flags else "OK",
        "max_pairwise_rho": max_rho,
        "consolidation_flags": flags,
        "pairwise_matrix": matrix,
    }


# ---------------------------------------------------------------------------
# Actions: kill LLM flag, reject strategy via governance
# ---------------------------------------------------------------------------

def apply_llm_kill(runtime_dir: Path) -> None:
    runtime_dir.mkdir(parents=True, exist_ok=True)
    flag = runtime_dir / "llm_killed.flag"
    flag.write_text(
        json.dumps({
            "killed_at": datetime.now(timezone.utc).isoformat(),
            "reason": "edge_validation: brier improvement below threshold",
        }),
        encoding="utf-8",
    )


def apply_strategy_kill(strategy_id: str, reason: str, registry_path: Path) -> str | None:
    """Force strategy to rejected in the registry JSON. Returns error string or None."""
    if not registry_path.exists():
        return f"registry not found: {registry_path}"
    try:
        data = json.loads(registry_path.read_text(encoding="utf-8"))
    except (json.JSONDecodeError, OSError) as exc:
        return str(exc)

    strategies = data.get("strategies", [])
    found = False
    for row in strategies:
        if row.get("strategy_id") == strategy_id:
            row["status"] = "rejected"
            row["rejection_reason"] = f"edge_validation_killed:{reason}"
            row["last_evaluated_at"] = datetime.now(timezone.utc).isoformat()
            found = True
            break

    if not found:
        return f"strategy {strategy_id!r} not found in registry"

    registry_path.write_text(json.dumps(data, indent=2), encoding="utf-8")
    return None


# ---------------------------------------------------------------------------
# Report generation
# ---------------------------------------------------------------------------

def _ascii_table(rows: list[dict[str, Any]], fields: list[str]) -> str:
    header = " | ".join(f"{f:>20}" for f in fields)
    sep = "-+-".join("-" * 20 for _ in fields)
    lines = [header, sep]
    for row in rows:
        lines.append(" | ".join(f"{str(row.get(f, ''))!s:>20}" for f in fields))
    return "\n".join(lines)


def build_report(
    stage_a: dict[str, Any],
    stage_b: list[dict[str, Any]],
    stage_c_results: list[dict[str, Any]],
    stage_d: dict[str, Any],
    run_ts: str,
) -> tuple[str, dict[str, Any]]:
    """Return (markdown_text, structured_json)."""

    overall_llm = stage_a.get("verdict", "INCONCLUSIVE")
    strategy_verdicts = {
        r["strategy_id"]: {
            "walkforward": r.get("verdict"),
            "bootstrap": next(
                (c.get("verdict") for c in stage_c_results if c["strategy_id"] == r["strategy_id"]),
                "INCONCLUSIVE",
            ),
        }
        for r in stage_b
    }

    # Final per-strategy verdict: KILL if either stage says KILL
    final: dict[str, str] = {}
    for sid, v in strategy_verdicts.items():
        if "KILL" in (v["walkforward"], v["bootstrap"]):
            final[sid] = "KILL"
        elif "PASS" in (v["walkforward"], v["bootstrap"]):
            final[sid] = "PASS"
        else:
            final[sid] = "INCONCLUSIVE"

    # Markdown
    md_lines = [
        f"# Edge Validation Gauntlet — {run_ts}",
        "",
        "## Stage A: LLM Calibration",
        f"**Verdict: {overall_llm}**",
        "",
        f"- Resolved markets: {stage_a.get('n_resolved', 'N/A')}",
        f"- Brier LLM: {stage_a.get('brier_llm', 'N/A')}",
        f"- Brier midpoint: {stage_a.get('brier_midpoint', 'N/A')}",
        f"- Relative improvement: {stage_a.get('relative_improvement', 'N/A')}",
        f"- Threshold: {stage_a.get('threshold_min_improvement', 'N/A')}",
        "",
    ]

    if "reliability_curve_ascii" in stage_a:
        md_lines += ["### Reliability Diagram", "", "```", stage_a["reliability_curve_ascii"], "```", ""]

    md_lines += [
        "## Stage B+C: Strategy Walk-Forward + Bootstrap",
        "",
    ]

    table_rows = []
    for sid, v in final.items():
        b_result = next((r for r in stage_b if r["strategy_id"] == sid), {})
        c_result = next((r for r in stage_c_results if r["strategy_id"] == sid), {})
        table_rows.append({
            "strategy_id": sid,
            "wf_verdict": b_result.get("verdict"),
            "bs_verdict": c_result.get("verdict"),
            "final": v,
            "pf_pass_rate": b_result.get("pf_pass_rate"),
            "sharpe_like": b_result.get("sharpe_like"),
            "t_stat": c_result.get("t_stat"),
        })

    fields = ["strategy_id", "wf_verdict", "bs_verdict", "final", "pf_pass_rate", "sharpe_like", "t_stat"]
    md_lines.append(_ascii_table(table_rows, fields))
    md_lines.append("")

    for r in stage_b:
        if r.get("kill_reasons"):
            md_lines.append(f"**{r['strategy_id']} kill reasons:** {', '.join(r['kill_reasons'])}")

    md_lines += [
        "",
        "## Stage D: Correlation",
        f"**Verdict: {stage_d.get('verdict')}**",
        "",
    ]
    for flag in stage_d.get("consolidation_flags", []):
        md_lines.append(f"- {flag['strategy_a']} ↔ {flag['strategy_b']}: ρ={flag['rho']}")

    md_lines += [
        "",
        "## Summary",
        f"- LLM: **{overall_llm}**",
    ]
    for sid, v in final.items():
        md_lines.append(f"- {sid}: **{v}**")

    md_text = "\n".join(md_lines)

    structured = {
        "run_ts": run_ts,
        "stage_a": stage_a,
        "stage_b": stage_b,
        "stage_c": stage_c_results,
        "stage_d": stage_d,
        "final_verdicts": {
            "llm": overall_llm,
            "strategies": final,
        },
    }

    return md_text, structured


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Edge validation gauntlet.")
    parser.add_argument("--store", default="var/events.jsonl", help="Path to JSONL event store")
    parser.add_argument("--config", default="config/edge_validation.yaml")
    parser.add_argument("--candles-dir", default="var/crypto_ohlcv", dest="candles_dir")
    parser.add_argument("--registry", default="var/crypto_strategy_registry.json")
    parser.add_argument("--reports-dir", default="reports", dest="reports_dir")
    parser.add_argument("--runtime-dir", default="runtime", dest="runtime_dir")
    parser.add_argument("--cache-dir", default="var/polymarket_cache", dest="cache_dir")
    parser.add_argument("--dry-run", action="store_true", help="Skip writes (kills, flag)")
    parser.add_argument(
        "--strategies",
        default="crypto_adx_ema_pullback_v1,crypto_volatility_breakout_v1",
        help="Comma-separated strategy IDs",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()

    # Load config ONCE — no per-run override
    config_path = ROOT / args.config
    cfg = load_yaml_simple(config_path)

    store_path = ROOT / args.store
    candles_dir = ROOT / args.candles_dir
    registry_path = ROOT / args.registry
    reports_dir = ROOT / args.reports_dir
    runtime_dir = ROOT / args.runtime_dir
    cache_dir = ROOT / args.cache_dir
    strategy_ids = [s.strip() for s in args.strategies.split(",") if s.strip()]

    run_ts = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")

    print(f"[gauntlet] config={config_path} store={store_path}", flush=True)
    print(f"[gauntlet] strategies={strategy_ids}", flush=True)

    # Stage A
    print("[gauntlet] Stage A: LLM calibration ...", flush=True)
    events = load_events(store_path)
    stage_a = stage_a_llm_calibration(events, cfg, cache_dir)
    print(f"[gauntlet] Stage A verdict: {stage_a['verdict']}", flush=True)

    # Stage B
    print("[gauntlet] Stage B: Walk-forward ...", flush=True)
    stage_b = stage_b_walkforward(strategy_ids, candles_dir, cfg)
    for r in stage_b:
        print(f"[gauntlet] Stage B {r['strategy_id']}: {r['verdict']}", flush=True)

    # Stage C (bootstrap per strategy, using walk-forward windows)
    print("[gauntlet] Stage C: Bootstrap ...", flush=True)
    stage_c: list[dict[str, Any]] = []
    # Build per-strategy walk-forward data for bootstrap
    for sr in stage_b:
        sid = sr["strategy_id"]
        windows: list[dict[str, Any]] = (sr.get("walkforward") or {}).get("windows", [])
        c_result = stage_c_bootstrap(sid, windows, cfg)
        stage_c.append(c_result)
        print(f"[gauntlet] Stage C {sid}: {c_result['verdict']}", flush=True)

    # Stage D
    print("[gauntlet] Stage D: Correlation ...", flush=True)
    # Attach windows to stage_b results for D
    stage_d = stage_d_correlation(
        [
            {**sr, "walkforward": sr.get("walkforward") or {"windows": []}}
            for sr in stage_b
        ],
        cfg,
    )
    print(f"[gauntlet] Stage D: {stage_d['verdict']}", flush=True)

    # Build report
    md_text, structured = build_report(stage_a, stage_b, stage_c, stage_d, run_ts)

    reports_dir.mkdir(parents=True, exist_ok=True)
    date_str = datetime.now(timezone.utc).strftime("%Y%m%d")
    md_path = reports_dir / f"edge_validation_{date_str}.md"
    json_path = reports_dir / f"edge_validation_{date_str}.json"

    if not args.dry_run:
        md_path.write_text(md_text, encoding="utf-8")
        json_path.write_text(json.dumps(structured, indent=2), encoding="utf-8")
        print(f"[gauntlet] report written: {md_path}", flush=True)

    # Compute reproducibility hash
    structured_canonical = json.dumps(structured, sort_keys=True)
    digest = hashlib.md5(structured_canonical.encode()).hexdigest()
    print(f"[gauntlet] structured_json_md5={digest}", flush=True)

    # Apply kills
    final_verdicts = structured["final_verdicts"]

    if not args.dry_run:
        if final_verdicts["llm"] == "KILL":
            apply_llm_kill(runtime_dir)
            print("[gauntlet] LLM kill flag written → runtime/llm_killed.flag", flush=True)

        for sid, verdict in final_verdicts["strategies"].items():
            if verdict == "KILL":
                kill_reasons = next(
                    (r.get("kill_reasons", []) for r in stage_b if r["strategy_id"] == sid), []
                )
                reason = ";".join(kill_reasons) if kill_reasons else "validation_failed"
                err = apply_strategy_kill(sid, reason, registry_path)
                if err:
                    print(f"[gauntlet] WARNING: could not reject {sid}: {err}", flush=True)
                else:
                    print(f"[gauntlet] Strategy {sid} → rejected in registry", flush=True)

    # Print summary
    print("\n=== GAUNTLET SUMMARY ===")
    print(f"LLM calibration: {final_verdicts['llm']}")
    for sid, v in final_verdicts["strategies"].items():
        print(f"  {sid}: {v}")
    print(f"Correlation: {stage_d['verdict']}")
    print(f"MD5: {digest}")

    any_kill = final_verdicts["llm"] == "KILL" or "KILL" in final_verdicts["strategies"].values()
    return 1 if any_kill else 0


if __name__ == "__main__":
    sys.exit(main())
