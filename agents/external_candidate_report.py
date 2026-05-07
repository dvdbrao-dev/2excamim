#!/usr/bin/env python3
"""External candidate markdown report generator (shadow/research observability)."""

from __future__ import annotations

import argparse
import json
from collections import Counter, defaultdict
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

try:
    from core.event_store import load_jsonl
    from core.logging import emit_json
except ModuleNotFoundError:  # pragma: no cover
    from agents.core.event_store import load_jsonl
    from agents.core.logging import emit_json


DEFAULT_INPUT = Path("./var/events/external_candidates.jsonl")
DEFAULT_REPORT_DIR = Path("./reports/external_candidates")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Generate markdown reports for external candidate events.")
    parser.add_argument("--input-jsonl", default=str(DEFAULT_INPUT))
    parser.add_argument("--report-dir", default=str(DEFAULT_REPORT_DIR))
    parser.add_argument("--strategy-version", default="")
    parser.add_argument("--run-id", default="")
    parser.add_argument("--output-report", default="")
    return parser.parse_args()


def _utc_now_slug() -> str:
    return datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")


def _safe_num(v: Any) -> float | None:
    if isinstance(v, (int, float)):
        return float(v)
    return None


def _read_events(path: Path) -> list[dict[str, Any]]:
    if not path.exists():
        return []
    return load_jsonl(path)


def _build_report(events: list[dict[str, Any]], strategy_filter: str) -> dict[str, Any]:
    strategy_rows: dict[str, dict[str, Any]] = defaultdict(
        lambda: {
            "signals": 0,
            "rejected_signals": 0,
            "rejection_reasons": Counter(),
            "fills": 0,
            "fill_assumptions": Counter(),
            "gross_pnl": 0.0,
            "net_pnl": 0.0,
            "gross_known": 0,
            "net_known": 0,
            "governance_status": Counter(),
            "governance_reason": Counter(),
        }
    )

    asset_window_rows: dict[str, dict[str, Any]] = defaultdict(
        lambda: {
            "signals": 0,
            "rejected": 0,
            "fills": 0,
            "resolved_rounds": 0,
            "net_pnl": 0.0,
            "data_gaps": 0,
            "feed_checks_failed": 0,
        }
    )

    run_rows: dict[str, dict[str, Any]] = defaultdict(
        lambda: {
            "events": 0,
            "signals": 0,
            "fills": 0,
            "rounds": 0,
            "evaluations": 0,
            "data_gaps": 0,
            "feed_health": 0,
        }
    )

    total_events = 0
    feed_health_total = 0
    feed_health_failed = 0
    data_gap_total = 0

    for event in events:
        total_events += 1
        et = event.get("event_type")
        payload = event.get("payload") if isinstance(event.get("payload"), dict) else {}
        provenance = event.get("provenance") if isinstance(event.get("provenance"), dict) else {}

        strategy = payload.get("strategy_version") if isinstance(payload.get("strategy_version"), str) else "unknown"
        if strategy_filter and strategy != strategy_filter and strategy != "unknown":
            continue

        asset = payload.get("asset") if isinstance(payload.get("asset"), str) else "unknown"
        window = payload.get("window") if isinstance(payload.get("window"), str) else "unknown"
        aw_key = f"{asset}/{window}"

        source = payload.get("source") if isinstance(payload.get("source"), str) else (
            provenance.get("source") if isinstance(provenance.get("source"), str) else "unknown"
        )
        run_rows[source]["events"] += 1

        if et == "candidate_signal.scored":
            strategy_rows[strategy]["signals"] += 1
            asset_window_rows[aw_key]["signals"] += 1
            run_rows[source]["signals"] += 1
            if payload.get("rejected") is True:
                strategy_rows[strategy]["rejected_signals"] += 1
                asset_window_rows[aw_key]["rejected"] += 1
                reason = payload.get("reject_reason") if isinstance(payload.get("reject_reason"), str) else "unknown"
                strategy_rows[strategy]["rejection_reasons"][reason] += 1

        elif et == "shadow_fill.simulated":
            strategy_rows[strategy]["fills"] += 1
            asset_window_rows[aw_key]["fills"] += 1
            run_rows[source]["fills"] += 1
            assumption = payload.get("fill_assumption") if isinstance(payload.get("fill_assumption"), str) else "unknown"
            strategy_rows[strategy]["fill_assumptions"][assumption] += 1

        elif et == "strategy_round.scored":
            run_rows[source]["rounds"] += 1
            if payload.get("outcome_known") is True:
                asset_window_rows[aw_key]["resolved_rounds"] += 1

            gross = _safe_num(payload.get("gross_pnl_usdc"))
            net = _safe_num(payload.get("net_pnl_usdc"))
            if gross is not None:
                strategy_rows[strategy]["gross_pnl"] += gross
                strategy_rows[strategy]["gross_known"] += 1
            if net is not None:
                strategy_rows[strategy]["net_pnl"] += net
                strategy_rows[strategy]["net_known"] += 1
                asset_window_rows[aw_key]["net_pnl"] += net

        elif et == "candidate_strategy.evaluated":
            run_rows[source]["evaluations"] += 1
            status = payload.get("status") if isinstance(payload.get("status"), str) else "unknown"
            reason = payload.get("reason") if isinstance(payload.get("reason"), str) else "unknown"
            strategy_rows[strategy]["governance_status"][status] += 1
            strategy_rows[strategy]["governance_reason"][reason] += 1

        elif et == "feed_health.checked":
            feed_health_total += 1
            run_rows[source]["feed_health"] += 1
            ok = payload.get("ok")
            if ok is False:
                feed_health_failed += 1
                asset_window_rows[aw_key]["feed_checks_failed"] += 1

        elif et == "data_gap.detected":
            data_gap_total += 1
            run_rows[source]["data_gaps"] += 1
            asset_window_rows[aw_key]["data_gaps"] += 1

    summary = {
        "events_total": total_events,
        "strategy_count": len(strategy_rows),
        "asset_window_count": len(asset_window_rows),
        "run_count": len(run_rows),
        "feed_health_total": feed_health_total,
        "feed_health_failed": feed_health_failed,
        "data_gap_total": data_gap_total,
    }

    return {
        "summary": summary,
        "by_strategy": strategy_rows,
        "by_asset_window": asset_window_rows,
        "by_run": run_rows,
    }


def _dict_lines(counter_like: Counter[str]) -> list[str]:
    if not counter_like:
        return ["- none"]
    return [f"- {k}: {v}" for k, v in counter_like.most_common()]


def render_markdown(report: dict[str, Any], report_path: Path, strategy_filter: str) -> str:
    s = report["summary"]
    lines: list[str] = [
        "# External Candidate Report",
        "",
        "## Summary",
        f"- Report path: `{report_path}`",
        f"- Strategy filter: `{strategy_filter or 'ALL'}`",
        f"- Events total: {s['events_total']}",
        f"- Strategies: {s['strategy_count']}",
        f"- Asset/windows: {s['asset_window_count']}",
        f"- Run groups: {s['run_count']}",
    ]

    lines.extend(
        [
            "",
            "## Feed Health",
            f"- Checks total: {s['feed_health_total']}",
            f"- Failed checks: {s['feed_health_failed']}",
            "",
            "## Data Gaps",
            f"- Gaps detected: {s['data_gap_total']}",
        ]
    )

    lines.append("")
    lines.append("## Per Strategy")
    for strategy, row in sorted(report["by_strategy"].items()):
        lines.append("")
        lines.append(f"### {strategy}")
        lines.append(f"- Signal counts: {row['signals']} (rejected: {row['rejected_signals']})")
        lines.append(f"- Fill simulations: {row['fills']}")
        lines.append(
            f"- PnL gross/net: {row['gross_pnl']:.6f} / {row['net_pnl']:.6f} (known gross rows: {row['gross_known']}, known net rows: {row['net_known']})"
        )
        lines.append("- Rejection reasons:")
        lines.extend(_dict_lines(row["rejection_reasons"]))
        lines.append("- Fill simulation assumptions:")
        lines.extend(_dict_lines(row["fill_assumptions"]))
        lines.append("- Governance status:")
        lines.extend(_dict_lines(row["governance_status"]))
        lines.append("- Governance reasons:")
        lines.extend(_dict_lines(row["governance_reason"]))

    lines.append("")
    lines.append("## Per Asset/Window")
    for aw_key, row in sorted(report["by_asset_window"].items()):
        lines.append("")
        lines.append(f"### {aw_key}")
        lines.append(f"- Signal counts: {row['signals']} (rejected: {row['rejected']})")
        lines.append(f"- Fill count: {row['fills']}")
        lines.append(f"- Resolved rounds: {row['resolved_rounds']}")
        lines.append(f"- Net PnL: {row['net_pnl']:.6f}")
        lines.append(f"- Feed check failures: {row['feed_checks_failed']}")
        lines.append(f"- Data gaps: {row['data_gaps']}")

    lines.append("")
    lines.append("## Per Run")
    for run_key, row in sorted(report["by_run"].items()):
        lines.append("")
        lines.append(f"### {run_key}")
        lines.append(f"- Events: {row['events']}")
        lines.append(f"- Signals: {row['signals']}")
        lines.append(f"- Fills: {row['fills']}")
        lines.append(f"- Rounds: {row['rounds']}")
        lines.append(f"- Evaluations: {row['evaluations']}")
        lines.append(f"- Feed health events: {row['feed_health']}")
        lines.append(f"- Data gap events: {row['data_gaps']}")

    lines.append("")
    lines.append("## Next Action")
    if s["feed_health_failed"] > 0 or s["data_gap_total"] > 0:
        lines.append("- Stabilize collectors first: reduce feed failures/data gaps before promoting any candidate.")
    elif any(row["signals"] == 0 for row in report["by_strategy"].values()):
        lines.append("- Generate more candidate signals before governance evaluation.")
    else:
        lines.append("- Continue shadow rounds and re-run scorecard when sample thresholds are met.")

    return "\n".join(lines) + "\n"


def run(args: argparse.Namespace) -> dict[str, Any]:
    events = _read_events(Path(args.input_jsonl))
    report = _build_report(events, args.strategy_version)

    report_dir = Path(args.report_dir)
    report_dir.mkdir(parents=True, exist_ok=True)

    slug = args.run_id.strip() or _utc_now_slug()
    output = Path(args.output_report) if args.output_report else report_dir / f"external_candidate_report_{slug}.md"
    output.parent.mkdir(parents=True, exist_ok=True)

    md = render_markdown(report, output, args.strategy_version)
    output.write_text(md, encoding="utf-8")

    return {
        "input_jsonl": str(args.input_jsonl),
        "output_report": str(output),
        "events_seen": report["summary"]["events_total"],
        "strategies": report["summary"]["strategy_count"],
        "asset_windows": report["summary"]["asset_window_count"],
        "run_groups": report["summary"]["run_count"],
    }


def main() -> int:
    args = parse_args()
    emit_json(run(args))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
