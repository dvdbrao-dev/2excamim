#!/usr/bin/env python3
"""Crypto strategy scorecard agent.

Classifies each strategy by real PnL metrics derived from fill.received events
using agents/core/pnl.py. No fake notional-based PnL formulas.

Thresholds used in classify_status:
  MIN_FILLS_FOR_STATUS     = 3     — minimum closed trades before promotion/freeze
  PROMOTE_MIN_PROFIT_FACTOR = 1.2  — profit_factor required for promotion
  PROMOTE_MIN_EXPECTANCY   = 0.0   — expectancy must be non-negative
  PROMOTE_MIN_CONFIDENCE   = 0.60  — confidence score threshold
  FREEZE_MAX_DRAWDOWN      = 0.15  — drawdown above which strategy is frozen
  FREEZE_NEG_WINDOWS       = 3     — consecutive negative windows before freeze
  KILL_VETO_RATIO          = 1.0   — vetoes/signals >= this with zero fills => KILL
  NEGATIVE_WINDOW_SIZE     = 5     — number of consecutive trades per window
"""
from __future__ import annotations

import argparse
import json
import math
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from agents.core.event_store import load_jsonl
from agents.core.pnl import FillInput, PnlSummary, calculate_realized_pnl_from_fills

AGENT_ID = "crypto-strategy-scorecard-agent-v1"
DEFAULT_STORE = Path("./var/events.jsonl")
DEFAULT_SCORECARD_PATH = Path("./runtime/crypto_strategy_scorecard.json")

MIN_FILLS_FOR_STATUS = 3
PROMOTE_MIN_PROFIT_FACTOR = 1.2
PROMOTE_MIN_EXPECTANCY = 0.0
PROMOTE_MIN_CONFIDENCE = 0.60
FREEZE_MAX_DRAWDOWN = 0.15
FREEZE_NEG_WINDOWS = 3
KILL_VETO_RATIO = 1.0
NEGATIVE_WINDOW_SIZE = 5


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build scorecard for crypto strategies.")
    parser.add_argument("--store", default=str(DEFAULT_STORE))
    parser.add_argument("--fee-bps", type=float, default=15.0)
    parser.add_argument("--slippage-bps", type=float, default=10.0)
    parser.add_argument("--scorecard-output", default=str(DEFAULT_SCORECARD_PATH))
    parser.add_argument("--json", action="store_true", help="Emit JSON output")
    return parser.parse_args()


def _as_float(value: Any) -> float | None:
    if isinstance(value, (int, float)) and math.isfinite(float(value)):
        return float(value)
    if isinstance(value, str):
        try:
            v = float(value)
            return v if math.isfinite(v) else None
        except ValueError:
            return None
    return None


@dataclass
class StrategyAcc:
    strategy_id: str
    signals_generated: int = 0
    decisions_formed: int = 0
    vetoes: int = 0
    fills_count: int = 0
    fill_inputs: list[FillInput] = field(default_factory=list)
    symbol: str | None = None
    timeframe: str | None = None


def _negative_windows(trade_pnls: list[float], window_size: int = NEGATIVE_WINDOW_SIZE) -> int:
    """Count non-overlapping windows of `window_size` consecutive all-negative trades."""
    if len(trade_pnls) < window_size:
        return 0
    count = 0
    i = 0
    while i + window_size <= len(trade_pnls):
        if all(p < 0 for p in trade_pnls[i : i + window_size]):
            count += 1
            i += window_size
        else:
            i += 1
    return count


def _sharpe_like(trade_pnls: list[float]) -> float | None:
    if len(trade_pnls) < 2:
        return None
    mean = sum(trade_pnls) / len(trade_pnls)
    variance = sum((p - mean) ** 2 for p in trade_pnls) / len(trade_pnls)
    std = variance ** 0.5
    if std == 0.0:
        return None
    return mean / std


def _confidence(winrate: float | None, max_dd: float) -> float:
    """Confidence in [0, 1] derived from winrate penalised by drawdown."""
    if winrate is None:
        return 0.0
    denominator = max(FREEZE_MAX_DRAWDOWN, 1e-9)
    dd_penalty = max(0.0, 1.0 - max_dd / denominator)
    return round(min(1.0, winrate * dd_penalty), 6)


def classify_status(acc: StrategyAcc, pnl: PnlSummary, neg_windows: int) -> str:
    """Map strategy to a status label using real PnL metrics.

    See module docstring for threshold values.
    """
    if acc.signals_generated == 0:
        return "KEEP_IN_PAPER"

    veto_ratio = acc.vetoes / acc.signals_generated
    if veto_ratio >= KILL_VETO_RATIO and acc.fills_count == 0:
        return "KILL"

    if pnl.max_drawdown > FREEZE_MAX_DRAWDOWN:
        return "FREEZE"
    if neg_windows >= FREEZE_NEG_WINDOWS:
        return "FREEZE"

    if pnl.trades < MIN_FILLS_FOR_STATUS:
        return "KEEP_IN_PAPER"

    pf = pnl.profit_factor
    pf_ok = pf == float("inf") or pf >= PROMOTE_MIN_PROFIT_FACTOR
    exp = pnl.expectancy or 0.0
    conf = _confidence(pnl.winrate, pnl.max_drawdown)

    if pf_ok and exp >= PROMOTE_MIN_EXPECTANCY and conf >= PROMOTE_MIN_CONFIDENCE:
        return "PROMOTE_CANDIDATE"

    if pnl.pnl_net < 0 and pnl.trades >= MIN_FILLS_FOR_STATUS:
        return "FREEZE"

    return "KEEP_IN_PAPER"


def build_scorecard_row(
    acc: StrategyAcc,
    pnl: PnlSummary,
    extra_warnings: list[str] | None = None,
) -> dict[str, Any]:
    trade_pnls = pnl._trade_pnls_net
    neg_windows = _negative_windows(trade_pnls)
    sharpe = _sharpe_like(trade_pnls)
    conf = _confidence(pnl.winrate, pnl.max_drawdown)
    status = classify_status(acc, pnl, neg_windows)

    recent_performance: float | None = None
    if trade_pnls:
        last_n = trade_pnls[-5:]
        recent_performance = round(sum(last_n) / len(last_n), 8)

    pf_raw = pnl.profit_factor
    profit_factor_out: float | None = None if pf_raw == float("inf") else round(pf_raw, 6)

    warnings: list[str] = list(extra_warnings or [])
    if pnl.trades == 0:
        warnings.append("no_fills_for_pnl")

    return {
        "strategy_id": acc.strategy_id,
        "symbol": acc.symbol,
        "timeframe": acc.timeframe,
        "signals_generated": acc.signals_generated,
        "decisions_formed": acc.decisions_formed,
        "vetoes": acc.vetoes,
        "fills": acc.fills_count,
        "trades": pnl.trades,
        "wins": pnl.wins,
        "losses": pnl.losses,
        "pnl_gross": round(pnl.pnl_gross, 8),
        "costs": round(pnl.costs, 8),
        "pnl_net": round(pnl.pnl_net, 8),
        "profit_factor": profit_factor_out,
        "expectancy": None if pnl.expectancy is None else round(pnl.expectancy, 8),
        "winrate": None if pnl.winrate is None else round(pnl.winrate, 6),
        "avg_return": None if pnl.expectancy is None else round(pnl.expectancy, 8),
        "max_drawdown": round(pnl.max_drawdown, 8),
        "sharpe_like": None if sharpe is None else round(sharpe, 6),
        "recent_performance": recent_performance,
        "confidence": conf,
        "negative_windows": neg_windows,
        "failed_runs": 0,
        "status": status,
        "warnings": warnings,
    }


def compute_scorecard(
    events: list[dict[str, Any]],
    fee_bps: float,
    slippage_bps: float,
) -> list[dict[str, Any]]:
    accs: dict[str, StrategyAcc] = {}
    signal_to_strategy: dict[str, str] = {}
    fills_by_strategy: dict[str, list[FillInput]] = {}

    for event in events:
        event_type = event.get("event_type")
        payload = event.get("payload") if isinstance(event.get("payload"), dict) else {}
        linkage = event.get("linkage") if isinstance(event.get("linkage"), dict) else {}

        if event_type == "crypto.signal.generated":
            strategy_id = payload.get("strategy_id")
            signal_id = payload.get("signal_id")
            if not isinstance(strategy_id, str):
                continue
            acc = accs.setdefault(strategy_id, StrategyAcc(strategy_id=strategy_id))
            acc.signals_generated += 1
            if isinstance(signal_id, str):
                signal_to_strategy[signal_id] = strategy_id
            if acc.symbol is None and isinstance(payload.get("symbol"), str):
                acc.symbol = payload["symbol"]
            if acc.timeframe is None and isinstance(payload.get("timeframe"), str):
                acc.timeframe = payload["timeframe"]
            continue

        signal_id = linkage.get("signal_id")
        strategy_id = signal_to_strategy.get(signal_id) if isinstance(signal_id, str) else None
        if strategy_id is None:
            continue

        acc = accs.setdefault(strategy_id, StrategyAcc(strategy_id=strategy_id))

        if event_type == "decision.formed":
            acc.decisions_formed += 1
        elif event_type == "veto.raised":
            acc.vetoes += 1
        elif event_type == "fill.received":
            acc.fills_count += 1
            side = payload.get("side")
            qty = _as_float(payload.get("filled_quantity") or payload.get("quantity"))
            px = _as_float(payload.get("avg_price") or payload.get("price"))
            if isinstance(side, str) and qty is not None and px is not None and qty > 0 and px > 0:
                fills_by_strategy.setdefault(strategy_id, []).append(
                    FillInput(side=side, quantity=qty, price=px)
                )

    rows: list[dict[str, Any]] = []
    for strategy_id, acc in sorted(accs.items()):
        fills = fills_by_strategy.get(strategy_id, [])
        pnl = calculate_realized_pnl_from_fills(fills, fee_bps=fee_bps, slippage_bps=slippage_bps)
        rows.append(build_scorecard_row(acc, pnl))

    return rows


def main() -> int:
    args = parse_args()
    events = load_jsonl(Path(args.store))
    rows = compute_scorecard(events, fee_bps=args.fee_bps, slippage_bps=args.slippage_bps)

    output = {"actor": AGENT_ID, "strategies": rows, "store": str(args.store)}

    scorecard_path = Path(args.scorecard_output)
    scorecard_path.parent.mkdir(parents=True, exist_ok=True)
    scorecard_path.write_text(json.dumps(output, indent=2) + "\n", encoding="utf-8")

    if args.json:
        print(json.dumps(output, separators=(",", ":")))
    else:
        print(json.dumps(output, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
