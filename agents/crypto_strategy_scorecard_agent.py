#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from agents.core.event_store import load_jsonl

AGENT_ID = "crypto-strategy-scorecard-agent-v1"
DEFAULT_STORE = Path("./var/events.jsonl")
DEFAULT_SCORECARD_PATH = Path("./runtime/crypto_strategy_scorecard.json")


@dataclass
class Score:
    strategy_id: str
    signals_generated: int = 0
    decisions_formed: int = 0
    vetoes: int = 0
    fills: int = 0
    open_positions: int = 0
    pnl_realized: float = 0.0


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build scorecard for crypto strategies.")
    parser.add_argument("--store", default=str(DEFAULT_STORE))
    parser.add_argument("--json", action="store_true", help="Emit JSON output")
    return parser.parse_args()


def _as_float(value: Any) -> float | None:
    if isinstance(value, (int, float)):
        return float(value)
    if isinstance(value, str):
        try:
            return float(value)
        except ValueError:
            return None
    return None


def classify_status(score: Score) -> str:
    if score.signals_generated < 3:
        return "KEEP_IN_PAPER"
    if score.vetoes >= score.signals_generated:
        return "KILL"
    if score.fills == 0 and score.signals_generated >= 10:
        return "FREEZE"
    if score.pnl_realized > 0 and score.fills >= 3:
        return "PROMOTE_CANDIDATE"
    if score.pnl_realized < 0 and score.fills >= 3:
        return "FREEZE"
    return "KEEP_IN_PAPER"


def main() -> int:
    args = parse_args()
    events = load_jsonl(Path(args.store))

    scores: dict[str, Score] = {}
    signal_to_strategy: dict[str, str] = {}
    open_by_strategy: dict[str, int] = {}

    for event in events:
        event_type = event.get("event_type")
        payload = event.get("payload") if isinstance(event.get("payload"), dict) else {}
        linkage = event.get("linkage") if isinstance(event.get("linkage"), dict) else {}

        if event_type == "crypto.signal.generated":
            strategy_id = payload.get("strategy_id")
            signal_id = payload.get("signal_id")
            if isinstance(strategy_id, str):
                score = scores.setdefault(strategy_id, Score(strategy_id=strategy_id))
                score.signals_generated += 1
                if isinstance(signal_id, str):
                    signal_to_strategy[signal_id] = strategy_id
            continue

        signal_id = linkage.get("signal_id")
        strategy_id = signal_to_strategy.get(signal_id) if isinstance(signal_id, str) else None
        if strategy_id is None:
            continue

        score = scores.setdefault(strategy_id, Score(strategy_id=strategy_id))

        if event_type == "decision.formed":
            score.decisions_formed += 1
            open_by_strategy[strategy_id] = open_by_strategy.get(strategy_id, 0) + 1
        elif event_type == "veto.raised":
            score.vetoes += 1
        elif event_type == "fill.received":
            score.fills += 1
            side = payload.get("side")
            qty = _as_float(payload.get("filled_quantity")) or _as_float(payload.get("quantity"))
            px = _as_float(payload.get("avg_price")) or _as_float(payload.get("price"))
            if qty is not None and px is not None:
                notional = qty * px
                if isinstance(side, str) and side.upper() in ("SELL", "SHORT"):
                    score.pnl_realized += notional * 0.001
                    open_by_strategy[strategy_id] = max(0, open_by_strategy.get(strategy_id, 0) - 1)
                else:
                    score.pnl_realized -= notional * 0.001

    rows = []
    for strategy_id, score in sorted(scores.items()):
        score.open_positions = open_by_strategy.get(strategy_id, 0)
        rows.append(
            {
                "strategy_id": strategy_id,
                "signals_generated": score.signals_generated,
                "decisions_formed": score.decisions_formed,
                "vetoes": score.vetoes,
                "fills": score.fills,
                "open_positions": score.open_positions,
                "pnl": round(score.pnl_realized, 6),
                "recommended_state": classify_status(score),
            }
        )

    output = {"actor": AGENT_ID, "strategies": rows, "store": str(args.store)}
    DEFAULT_SCORECARD_PATH.parent.mkdir(parents=True, exist_ok=True)
    DEFAULT_SCORECARD_PATH.write_text(json.dumps(output, indent=2) + "\n", encoding="utf-8")
    if args.json:
        print(json.dumps(output, separators=(",", ":")))
    else:
        print(json.dumps(output, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
