#!/usr/bin/env python3

import json
from collections import Counter
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path


EUR_USD_RATE = 0.92
DATA_PATH = Path("var/events.jsonl")
TARGET_ACTOR = "probability-agent-v1"
TARGET_EVENTS = {"signal.confirmed", "veto.raised"}
PIPELINE_EVENTS = ("signal.generated", "signal.confirmed", "decision.formed", "veto.raised")


@dataclass
class Summary:
    total_usd: float = 0.0
    signals_processed: int = 0
    decisions_formed: int = 0
    pipeline_counts: Counter | None = None


def parse_notes_cost(notes: str | None) -> float:
    if not notes:
        return 0.0

    try:
        parsed = json.loads(notes)
    except json.JSONDecodeError:
        return 0.0

    value = parsed.get("estimated_cost_usd", 0.0)
    try:
        return float(value)
    except (TypeError, ValueError):
        return 0.0


def load_summary(path: Path) -> Summary:
    today = datetime.now(timezone.utc).date().isoformat()
    pipeline_counts = Counter()
    total_usd = 0.0
    signals_processed = 0
    decisions_formed = 0

    with path.open("r", encoding="utf-8") as handle:
        for raw_line in handle:
            raw_line = raw_line.strip()
            if not raw_line:
                continue

            event = json.loads(raw_line)
            occurred_at = event.get("occurred_at", "")
            if not occurred_at.startswith(today):
                continue

            event_type = event.get("event_type", "")
            if event_type in pipeline_counts:
                pipeline_counts[event_type] += 1
            elif event_type in PIPELINE_EVENTS:
                pipeline_counts[event_type] += 1

            provenance = event.get("provenance") or {}
            actor = provenance.get("actor")

            if event_type in TARGET_EVENTS and actor == TARGET_ACTOR:
                total_usd += parse_notes_cost(provenance.get("notes"))
                signals_processed += 1

            if event_type == "decision.formed":
                decisions_formed += 1

    return Summary(
        total_usd=total_usd,
        signals_processed=signals_processed,
        decisions_formed=decisions_formed,
        pipeline_counts=pipeline_counts,
    )


def format_summary(summary: Summary) -> str:
    total_eur = summary.total_usd * EUR_USD_RATE
    per_signal = summary.total_usd / summary.signals_processed if summary.signals_processed else 0.0
    per_decision = summary.total_usd / summary.decisions_formed if summary.decisions_formed else 0.0
    monthly = summary.total_usd * 30.0
    monthly_eur = monthly * EUR_USD_RATE
    counts = summary.pipeline_counts or Counter()

    return (
        "=== EXCAMIM PnL Summary ===\n"
        f"Fecha: {datetime.now(timezone.utc).date().isoformat()}\n\n"
        "--- Costes LLM ---\n"
        f"Total gastado:     ${summary.total_usd:.4f} (€{total_eur:.4f})\n"
        f"Señales procesadas: {summary.signals_processed}\n"
        f"Coste por señal:   ${per_signal:.5f}\n"
        f"Decisiones formadas: {summary.decisions_formed}\n"
        f"Coste por decisión: ${per_decision:.4f}\n"
        f"Proyección mensual: ${monthly:.2f}/mes (~€{monthly_eur:.2f})\n\n"
        "--- Pipeline hoy ---\n"
        f"signal.generated:  {counts.get('signal.generated', 0)}\n"
        f"signal.confirmed:  {counts.get('signal.confirmed', 0)}\n"
        f"decision.formed:   {counts.get('decision.formed', 0)}\n"
        f"veto.raised:       {counts.get('veto.raised', 0)}"
    )


def main() -> int:
    summary = load_summary(DATA_PATH)
    print(format_summary(summary))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
