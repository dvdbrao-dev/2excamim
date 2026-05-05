#!/usr/bin/env python3
"""Drawdown circuit breaker.

Computes rolling drawdown for paper and shadow equity curves.
Triggers var/.kill_switch if thresholds are breached.
Triggers permanent kill (var/.kill_switch.permanent) for 30d breach.
"""
from __future__ import annotations

import argparse
import json
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
AGENTS_DIR = ROOT / "agents"
for _p in [str(ROOT), str(AGENTS_DIR)]:
    if _p not in sys.path:
        sys.path.insert(0, _p)

from core.event_store import load_jsonl
from core.logging import emit_json
from core.time import utc_now_rfc3339
from core.equity_curve import compute_equity_curve, rolling_drawdown

AGENT_ID = "drawdown-guard-v1"
DEFAULT_STORE = Path("./var/events.jsonl")
DEFAULT_CONFIG = Path("config/risk_limits.yaml")

DEFAULT_SHADOW_DD_7D = 0.05
DEFAULT_PAPER_DD_7D = 0.08
DEFAULT_DD_30D_PERMANENT = 0.15
DEFAULT_KILL_SWITCH = Path("var/.kill_switch")
DEFAULT_KILL_PERMANENT = Path("var/.kill_switch.permanent")
DEFAULT_KILL_REASON = Path("runtime/kill_switch_reason.json")
DEFAULT_BANKROLL = 1000.0


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Drawdown circuit breaker.")
    parser.add_argument("--store", default=str(DEFAULT_STORE))
    parser.add_argument("--config", default=str(DEFAULT_CONFIG))
    parser.add_argument("--bankroll", type=float, default=DEFAULT_BANKROLL)
    parser.add_argument("--equity-output-dir", default="runtime", dest="equity_output_dir")
    return parser.parse_args()


def _load_config(config_path: Path) -> dict[str, Any]:
    from scripts.edge_validation_gauntlet import load_yaml_simple
    if config_path.exists():
        return load_yaml_simple(config_path)
    return {}


def _write_kill_reason(
    reason_path: Path,
    reason: str,
    scope: str,
    dd_value: float,
    threshold: float,
    permanent: bool,
) -> None:
    reason_path.parent.mkdir(parents=True, exist_ok=True)
    data = {
        "triggered_at": utc_now_rfc3339(),
        "reason": reason,
        "scope": scope,
        "drawdown_observed": round(dd_value, 6),
        "threshold": threshold,
        "permanent": permanent,
    }
    reason_path.write_text(json.dumps(data, indent=2), encoding="utf-8")


def _send_telegram_notification(message: str) -> None:
    """Best-effort Telegram notification."""
    try:
        import subprocess
        script = ROOT / "scripts" / "telegram.sh"
        if script.exists():
            subprocess.run(
                ["bash", str(script), message],
                timeout=10,
                capture_output=True,
            )
    except Exception:
        pass


def _equity_to_file(curve: list[tuple[float, float]], output_path: Path) -> None:
    output_path.parent.mkdir(parents=True, exist_ok=True)
    data = [{"ts": ts, "equity": eq} for ts, eq in curve]
    output_path.write_text(json.dumps(data), encoding="utf-8")


def main() -> int:
    args = parse_args()
    store_path = ROOT / args.store
    config_path = ROOT / args.config
    equity_dir = ROOT / args.equity_output_dir

    cfg = _load_config(config_path)
    dd_cfg = cfg.get("drawdown_guard", {})

    shadow_dd_7d_threshold = float(dd_cfg.get("shadow_dd_7d_kill_pct", DEFAULT_SHADOW_DD_7D))
    paper_dd_7d_threshold = float(dd_cfg.get("paper_dd_7d_kill_pct", DEFAULT_PAPER_DD_7D))
    dd_30d_threshold = float(dd_cfg.get("dd_30d_permanent_kill_pct", DEFAULT_DD_30D_PERMANENT))
    kill_switch_path = ROOT / dd_cfg.get("kill_switch_path", str(DEFAULT_KILL_SWITCH))
    kill_permanent_path = ROOT / dd_cfg.get("kill_switch_permanent_path", str(DEFAULT_KILL_PERMANENT))
    kill_reason_path = ROOT / dd_cfg.get("kill_reason_path", str(DEFAULT_KILL_REASON))

    if not store_path.exists():
        emit_json({"actor": AGENT_ID, "status": "no_store", "store": str(store_path)})
        return 0

    events = list(load_jsonl(store_path))
    bankroll = args.bankroll

    # Compute equity curves
    paper_curve = compute_equity_curve(events, initial_bankroll=bankroll, scope="paper")
    shadow_curve = compute_equity_curve(events, initial_bankroll=bankroll, scope="shadow")

    # Persist curves for reporting
    _equity_to_file(paper_curve, equity_dir / "equity_curve_paper.json")
    _equity_to_file(shadow_curve, equity_dir / "equity_curve_shadow.json")

    # Compute rolling drawdowns
    paper_dd_7d = rolling_drawdown(paper_curve, window_days=7.0)
    shadow_dd_7d = rolling_drawdown(shadow_curve, window_days=7.0)
    paper_dd_30d = rolling_drawdown(paper_curve, window_days=30.0)
    shadow_dd_30d = rolling_drawdown(shadow_curve, window_days=30.0)

    emit_json({
        "actor": AGENT_ID,
        "paper_dd_7d": round(paper_dd_7d, 6),
        "shadow_dd_7d": round(shadow_dd_7d, 6),
        "paper_dd_30d": round(paper_dd_30d, 6),
        "shadow_dd_30d": round(shadow_dd_30d, 6),
        "paper_fills": len([p for p in paper_curve]) - 1,
        "shadow_fills": len([p for p in shadow_curve]) - 1,
    })

    # Check permanent kill (30d)
    worst_dd_30d = max(paper_dd_30d, shadow_dd_30d)
    if worst_dd_30d >= dd_30d_threshold:
        scope = "paper" if paper_dd_30d >= shadow_dd_30d else "shadow"
        reason = f"dd_30d_{scope}_breach:{worst_dd_30d:.4f}>={dd_30d_threshold}"
        _write_kill_reason(kill_reason_path, reason, scope, worst_dd_30d, dd_30d_threshold, permanent=True)
        kill_permanent_path.parent.mkdir(parents=True, exist_ok=True)
        kill_permanent_path.touch()
        kill_switch_path.parent.mkdir(parents=True, exist_ok=True)
        kill_switch_path.touch()
        msg = f"[drawdown_guard] PERMANENT KILL: {reason}"
        emit_json({"actor": AGENT_ID, "status": "permanent_kill", "reason": reason})
        _send_telegram_notification(msg)
        return 1

    # Check rolling 7d kills
    kill_triggered = False
    kill_reason = ""

    if shadow_dd_7d >= shadow_dd_7d_threshold:
        kill_reason = f"shadow_dd_7d:{shadow_dd_7d:.4f}>={shadow_dd_7d_threshold}"
        _write_kill_reason(kill_reason_path, kill_reason, "shadow", shadow_dd_7d, shadow_dd_7d_threshold, permanent=False)
        kill_triggered = True

    if paper_dd_7d >= paper_dd_7d_threshold:
        paper_reason = f"paper_dd_7d:{paper_dd_7d:.4f}>={paper_dd_7d_threshold}"
        if not kill_triggered:
            kill_reason = paper_reason
            _write_kill_reason(kill_reason_path, kill_reason, "paper", paper_dd_7d, paper_dd_7d_threshold, permanent=False)
        kill_triggered = True

    if kill_triggered:
        kill_switch_path.parent.mkdir(parents=True, exist_ok=True)
        kill_switch_path.touch()
        msg = f"[drawdown_guard] KILL SWITCH: {kill_reason}"
        emit_json({"actor": AGENT_ID, "status": "kill_switch_triggered", "reason": kill_reason})
        _send_telegram_notification(msg)
        return 1

    emit_json({"actor": AGENT_ID, "status": "ok", "no_breach": True})
    return 0


if __name__ == "__main__":
    sys.exit(main())
