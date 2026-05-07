#!/usr/bin/env python3
"""Generate weekly shadow vs paper PnL divergence report.

Usage:
    python scripts/shadow_vs_paper_report.py [--output-dir reports]
"""
from __future__ import annotations

import json
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))


def _load_equity_curve(path: Path) -> list[tuple[float, float]]:
    if not path.exists():
        return []
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
        return [(float(x["ts"]), float(x["equity"])) for x in data]
    except (json.JSONDecodeError, KeyError, TypeError, ValueError):
        return []


def compute_divergence(
    paper_curve: list[tuple[float, float]],
    shadow_curve: list[tuple[float, float]],
    initial_bankroll: float = 1000.0,
) -> dict[str, Any]:
    if not paper_curve or not shadow_curve:
        return {"divergence_pct": None, "paper_pnl": None, "shadow_pnl": None}

    paper_pnl = paper_curve[-1][1] - initial_bankroll
    shadow_pnl = shadow_curve[-1][1] - initial_bankroll
    divergence = paper_pnl - shadow_pnl

    divergence_pct: float | None = None
    if abs(paper_pnl) > 1e-6:
        divergence_pct = abs(divergence) / abs(paper_pnl)

    return {
        "paper_final_equity": round(paper_curve[-1][1], 4),
        "shadow_final_equity": round(shadow_curve[-1][1], 4),
        "paper_pnl": round(paper_pnl, 4),
        "shadow_pnl": round(shadow_pnl, 4),
        "divergence_abs": round(divergence, 4),
        "divergence_pct": round(divergence_pct, 4) if divergence_pct is not None else None,
        "paper_optimism_flag": (
            divergence_pct is not None and divergence_pct > 0.30 and paper_pnl > shadow_pnl
        ),
    }


def build_report(
    paper_curve: list[tuple[float, float]],
    shadow_curve: list[tuple[float, float]],
    week_str: str,
    initial_bankroll: float = 1000.0,
) -> str:
    div = compute_divergence(paper_curve, shadow_curve, initial_bankroll)

    lines = [
        f"# Shadow vs Paper Report — Week {week_str}",
        "",
        "## Equity Summary",
        f"- Paper final equity:  {div.get('paper_final_equity', 'N/A')}",
        f"- Shadow final equity: {div.get('shadow_final_equity', 'N/A')}",
        f"- Paper PnL:           {div.get('paper_pnl', 'N/A')}",
        f"- Shadow PnL:          {div.get('shadow_pnl', 'N/A')}",
        "",
        "## Divergence",
        f"- Absolute divergence: {div.get('divergence_abs', 'N/A')}",
        f"- Relative divergence: {div['divergence_pct']:.1%}" if div.get('divergence_pct') is not None else "- Relative divergence: N/A",
        "",
    ]

    if div.get("paper_optimism_flag"):
        lines += [
            "## ⚠️  Optimism Warning",
            "Paper PnL exceeds shadow PnL by >30%. The paper fill model is",
            "overly optimistic. Consider tightening slippage assumptions.",
            "",
        ]
    else:
        lines += ["## Status: OK — divergence within acceptable range.", ""]

    return "\n".join(lines)


def parse_args():
    import argparse
    parser = argparse.ArgumentParser()
    parser.add_argument("--output-dir", default="reports", dest="output_dir")
    parser.add_argument("--runtime-dir", default="runtime", dest="runtime_dir")
    parser.add_argument("--bankroll", type=float, default=1000.0)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    output_dir = ROOT / args.output_dir
    runtime_dir = ROOT / args.runtime_dir

    paper_curve = _load_equity_curve(runtime_dir / "equity_curve_paper.json")
    shadow_curve = _load_equity_curve(runtime_dir / "equity_curve_shadow.json")

    week_str = datetime.now(timezone.utc).strftime("%Y-W%W")
    report = build_report(paper_curve, shadow_curve, week_str, args.bankroll)

    output_dir.mkdir(parents=True, exist_ok=True)
    out_path = output_dir / f"shadow_vs_paper_{week_str}.md"
    out_path.write_text(report, encoding="utf-8")
    print(f"[shadow_vs_paper_report] Written: {out_path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
