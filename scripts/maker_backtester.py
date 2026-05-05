#!/usr/bin/env python3
"""Maker strategy backtester for Polymarket NO-side liquidity provision.

Uses historical market data (from polymarket_orderbook_history) and simulates
conservative fill rates and PnL for maker quotes at NO >= 0.85.

Pre-committed validation rule (must be satisfied for PASS verdict):
    expected_edge_per_fill - fees - adverse_selection > 0
    in >= 60% of analyzed months, with aggregate t-stat >= 1.5

Usage:
    python scripts/maker_backtester.py [--data-dir PATH] [--quote-price 0.85]
                                       [--quote-size 50] [--output-json PATH]
"""
from __future__ import annotations

import argparse
import json
import math
import sys
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from agents.services.polymarket_orderbook_history import load_markets_from_dir
from scripts.maker_fill_simulator import build_synthetic_snapshot, simulate_fill
from agents.core.bootstrap import bootstrap_mean_t_stat

# Pre-committed validation thresholds — must be satisfied for PASS
VALIDATION_MONTHLY_PASS_RATE = 0.60  # >= 60% of months must be profitable net of fees
VALIDATION_MIN_T_STAT = 1.5
VALIDATION_FEE_BPS = 0.0            # maker fee on V2
VALIDATION_ADVERSE_PENALTY_BPS = 50.0  # worst-case adverse selection assumption

DEFAULT_QUOTE_PRICE_NO = 0.85
DEFAULT_QUOTE_SIZE_USD = 50.0
DEFAULT_MIN_NO_MIDPOINT = 0.85
DEFAULT_MIN_VOLUME = 50_000.0
DEFAULT_SPREAD = 0.02
DEFAULT_SIGMA_1H = 0.01
DEFAULT_VOLUME_1H = 500.0


def simulate_market(
    market: dict[str, Any],
    quote_price_no: float = DEFAULT_QUOTE_PRICE_NO,
    quote_size_usd: float = DEFAULT_QUOTE_SIZE_USD,
    spread: float = DEFAULT_SPREAD,
    sigma_1h: float = DEFAULT_SIGMA_1H,
    volume_1h_usd: float = DEFAULT_VOLUME_1H,
    n_snapshots: int = 24,    # simulate 24 hourly opportunities per market
) -> dict[str, Any]:
    """Simulate maker activity on a single resolved market.

    For each synthetic snapshot, attempt to post at quote_price_no.
    Track fills and compute PnL at resolution.
    """
    resolution = market.get("resolution")
    no_midpoint = float(market.get("no_midpoint", 0.0))
    market_id = market.get("market_id", "unknown")

    if resolution is None:
        return {"market_id": market_id, "skipped": True, "reason": "unresolved"}

    if no_midpoint < quote_price_no:
        return {"market_id": market_id, "skipped": True, "reason": "midpoint_below_quote"}

    fills: list[dict[str, Any]] = []
    total_pnl = 0.0
    total_notional = 0.0

    # Build synthetic snapshots (one per simulated hour)
    # Slight downward drift for NO price is conservative (mimics worst case)
    drift_per_step = -0.001  # 0.1% per hour drift against us
    base_no_mid = no_midpoint

    for step in range(n_snapshots):
        # Degrade no_midpoint slightly over time (conservative)
        current_no_mid = max(quote_price_no, base_no_mid + drift_per_step * step)
        snapshot = build_synthetic_snapshot(
            no_midpoint=current_no_mid,
            spread=spread,
            volume_1h_usd=volume_1h_usd,
            sigma_1h=sigma_1h,
        )

        # Build future snapshots for adverse selection (next 3 steps)
        future = []
        for future_step in range(step + 1, min(step + 4, n_snapshots)):
            future_no_mid = max(quote_price_no, base_no_mid + drift_per_step * future_step)
            future.append(build_synthetic_snapshot(
                no_midpoint=future_no_mid,
                spread=spread,
                volume_1h_usd=volume_1h_usd,
                sigma_1h=sigma_1h,
            ))

        result = simulate_fill(
            snapshot,
            quote_price_no=quote_price_no,
            quote_size_usd=quote_size_usd,
            future_snapshots=future,
            fee_bps=VALIDATION_FEE_BPS,
        )

        if result["filled"]:
            fill_price = result["fill_price"]
            adverse_bps = result["adverse_selection_bps"]
            adverse_cost = (adverse_bps / 10_000.0) * quote_size_usd

            # PnL at resolution
            if resolution == "NO":
                gross_pnl = (1.0 - fill_price) * (quote_size_usd / fill_price)
            else:  # YES: we bought NO tokens that are now worthless
                gross_pnl = -fill_price * (quote_size_usd / fill_price)

            # Subtract fees and adverse selection
            net_pnl = gross_pnl - adverse_cost
            fills.append({
                "step": step,
                "fill_price": fill_price,
                "gross_pnl": round(gross_pnl, 6),
                "adverse_cost": round(adverse_cost, 6),
                "net_pnl": round(net_pnl, 6),
            })
            total_pnl += net_pnl
            total_notional += quote_size_usd

    if not fills:
        return {
            "market_id": market_id,
            "skipped": False,
            "fill_count": 0,
            "total_pnl": 0.0,
            "total_notional": 0.0,
            "fill_rate": 0.0,
            "expected_edge_per_fill": None,
            "resolution": resolution,
            "no_midpoint": no_midpoint,
        }

    n_fills = len(fills)
    expected_edge = total_pnl / n_fills
    fill_rate = n_fills / n_snapshots

    return {
        "market_id": market_id,
        "skipped": False,
        "fill_count": n_fills,
        "total_pnl": round(total_pnl, 6),
        "total_notional": round(total_notional, 2),
        "fill_rate": round(fill_rate, 4),
        "expected_edge_per_fill": round(expected_edge, 6),
        "resolution": resolution,
        "no_midpoint": no_midpoint,
        "fills": fills,
    }


def run_maker_backtest(
    markets: list[dict[str, Any]],
    quote_price_no: float = DEFAULT_QUOTE_PRICE_NO,
    quote_size_usd: float = DEFAULT_QUOTE_SIZE_USD,
) -> dict[str, Any]:
    """Run maker backtest across all markets; aggregate monthly."""
    market_results: list[dict[str, Any]] = []
    monthly: dict[str, list[float]] = defaultdict(list)  # month → list of per-market net_pnl

    for market in markets:
        result = simulate_market(market, quote_price_no, quote_size_usd)
        market_results.append(result)

        if not result.get("skipped") and result.get("fill_count", 0) > 0:
            ingested_at = market.get("ingested_at", "")
            month = ingested_at[:7] if len(ingested_at) >= 7 else "unknown"
            monthly[month].append(result["total_pnl"])

    # Monthly aggregation
    monthly_summary: list[dict[str, Any]] = []
    profitable_months = 0
    all_pnls: list[float] = []

    for month, pnls in sorted(monthly.items()):
        total = sum(pnls)
        avg = total / len(pnls) if pnls else 0.0
        net_after_costs = avg - (VALIDATION_ADVERSE_PENALTY_BPS / 10_000.0 * quote_size_usd)
        profitable = net_after_costs > 0
        if profitable:
            profitable_months += 1
        all_pnls.extend(pnls)
        monthly_summary.append({
            "month": month,
            "markets_with_fills": len(pnls),
            "total_pnl": round(total, 6),
            "avg_pnl_per_market": round(avg, 6),
            "net_after_adverse_penalty": round(net_after_costs, 6),
            "profitable": profitable,
        })

    n_months = len(monthly_summary)
    monthly_pass_rate = profitable_months / n_months if n_months > 0 else 0.0

    # Bootstrap t-stat on per-market PnL
    t_stat = 0.0
    ci_low = ci_high = 0.0
    mean_pnl = 0.0
    if all_pnls and len(all_pnls) >= 3:
        mean_pnl, t_stat, ci_low, ci_high = bootstrap_mean_t_stat(all_pnls, n_iterations=1000, seed=42)

    # Apply pre-committed validation rule
    verdict = _apply_validation_rule(monthly_pass_rate, t_stat)

    total_fills = sum(r.get("fill_count", 0) for r in market_results if not r.get("skipped"))
    total_markets = sum(1 for r in market_results if not r.get("skipped"))
    total_pnl_all = sum(r.get("total_pnl", 0.0) for r in market_results if not r.get("skipped"))

    return {
        "verdict": verdict,
        "strategy": "maker_no_side",
        "strategy_id": "maker_no_side_v1",
        "quote_price_no": quote_price_no,
        "quote_size_usd": quote_size_usd,
        "markets_analyzed": total_markets,
        "markets_skipped": sum(1 for r in market_results if r.get("skipped")),
        "total_fills": total_fills,
        "total_pnl": round(total_pnl_all, 6),
        "mean_pnl_per_market": round(mean_pnl, 6) if all_pnls else None,
        "t_stat": round(t_stat, 4),
        "ci_low_90": round(ci_low, 6) if all_pnls else None,
        "ci_high_90": round(ci_high, 6) if all_pnls else None,
        "monthly_pass_rate": round(monthly_pass_rate, 4),
        "profitable_months": profitable_months,
        "total_months": n_months,
        "validation_threshold_monthly_pass_rate": VALIDATION_MONTHLY_PASS_RATE,
        "validation_threshold_t_stat": VALIDATION_MIN_T_STAT,
        "monthly_summary": monthly_summary,
        # Scorecard-compatible fields
        "signals_generated": total_markets,
        "fills": total_fills,
        "pnl_net": round(total_pnl_all, 6),
        "profit_factor": _profit_factor(market_results),
        "expectancy": round(mean_pnl, 6) if all_pnls else None,
        "max_drawdown": _max_drawdown(monthly_summary),
        "confidence": round(monthly_pass_rate, 6),
        "negative_windows": n_months - profitable_months,
        "failed_runs": 0,
        "status": "candidate" if verdict == "PASS" else "rejected",
    }


def _apply_validation_rule(monthly_pass_rate: float, t_stat: float) -> str:
    """Pre-committed binary verdict."""
    if monthly_pass_rate >= VALIDATION_MONTHLY_PASS_RATE and t_stat >= VALIDATION_MIN_T_STAT:
        return "PASS"
    if monthly_pass_rate < VALIDATION_MONTHLY_PASS_RATE and t_stat < VALIDATION_MIN_T_STAT:
        return "FAIL"
    return "INCONCLUSIVE"


def _profit_factor(results: list[dict[str, Any]]) -> float | None:
    total_wins = sum(r.get("total_pnl", 0.0) for r in results if r.get("total_pnl", 0.0) > 0 and not r.get("skipped"))
    total_losses = abs(sum(r.get("total_pnl", 0.0) for r in results if r.get("total_pnl", 0.0) < 0 and not r.get("skipped")))
    if total_losses == 0:
        return None
    return round(total_wins / total_losses, 6)


def _max_drawdown(monthly_summary: list[dict[str, Any]]) -> float:
    pnls = [m.get("total_pnl", 0.0) for m in monthly_summary]
    if not pnls:
        return 0.0
    equity = 0.0
    peak = 0.0
    max_dd = 0.0
    for p in pnls:
        equity += p
        if equity > peak:
            peak = equity
        dd = (peak - equity) / (abs(peak) + 1e-9)
        if dd > max_dd:
            max_dd = dd
    return round(max_dd, 6)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Maker strategy backtester.")
    parser.add_argument("--data-dir", default="data/orderbook", dest="data_dir")
    parser.add_argument("--quote-price", type=float, default=DEFAULT_QUOTE_PRICE_NO, dest="quote_price")
    parser.add_argument("--quote-size", type=float, default=DEFAULT_QUOTE_SIZE_USD, dest="quote_size")
    parser.add_argument("--output-json", default=None, dest="output_json")
    parser.add_argument("--min-no-midpoint", type=float, default=DEFAULT_MIN_NO_MIDPOINT, dest="min_no_midpoint")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    data_dir = ROOT / args.data_dir

    print(f"[maker_backtester] Loading markets from {data_dir}")
    markets = load_markets_from_dir(data_dir, min_no_midpoint=args.min_no_midpoint)
    print(f"[maker_backtester] {len(markets)} markets loaded")

    if not markets:
        print("[maker_backtester] No markets found. Run polymarket_orderbook_history.py first.")
        return 1

    result = run_maker_backtest(markets, args.quote_price, args.quote_size)

    print(f"\n=== MAKER BACKTESTER RESULT ===")
    print(f"Verdict:              {result['verdict']}")
    print(f"Markets analyzed:     {result['markets_analyzed']}")
    print(f"Total fills:          {result['total_fills']}")
    print(f"Total PnL:            {result['total_pnl']}")
    print(f"Monthly pass rate:    {result['monthly_pass_rate']:.1%} (threshold: {VALIDATION_MONTHLY_PASS_RATE:.0%})")
    print(f"T-stat:               {result['t_stat']:.3f} (threshold: {VALIDATION_MIN_T_STAT})")

    if args.output_json:
        out_path = Path(args.output_json)
        out_path.parent.mkdir(parents=True, exist_ok=True)
        out_path.write_text(json.dumps(result, indent=2), encoding="utf-8")
        print(f"[maker_backtester] Output written: {out_path}")

    return 0 if result["verdict"] != "FAIL" else 1


if __name__ == "__main__":
    sys.exit(main())
