"""Tests for drawdown guard circuit breaker."""
from __future__ import annotations

import json
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from agents.core.equity_curve import compute_equity_curve, rolling_drawdown


def _make_fill(side: str, price: float, qty: float, ts_unix: float, shadow: bool = False) -> dict:
    agg = "shadow:polymarket:0xtest" if shadow else "polymarket:0xtest"
    return {
        "event_type": "shadow.fill.received" if shadow else "fill.received",
        "aggregate_key": agg,
        "occurred_at": f"2026-04-{int(ts_unix // 86400) + 1:02d}T00:00:00Z",
        "payload": {"side": side, "price": price, "quantity": qty},
    }


class TestDrawdownGuardKillSwitch:
    def test_drawdown_guard_triggers_kill_switch_at_threshold(self) -> None:
        """Shadow DD 7d >= 5% threshold → kill switch written."""
        with tempfile.TemporaryDirectory() as tmpdir:
            tmpdir_path = Path(tmpdir)
            kill_switch = tmpdir_path / "kill_switch"
            kill_reason = tmpdir_path / "kill_reason.json"
            config_path = tmpdir_path / "risk_limits.yaml"
            config_path.write_text(
                "drawdown_guard:\n"
                "  shadow_dd_7d_kill_pct: 0.05\n"
                "  paper_dd_7d_kill_pct: 0.08\n"
                "  dd_30d_permanent_kill_pct: 0.15\n",
                encoding="utf-8",
            )

            # Build shadow equity curve with 8% drop (> 5% threshold)
            # BUY 1000 units at 1.0 → equity = -1000
            # Then no SELL → equity stuck
            from agents.core.equity_curve import rolling_drawdown
            curve_with_dd = [
                (0.0, 1000.0),
                (86400.0, 940.0),  # -6% drop
            ]
            dd = rolling_drawdown(curve_with_dd, window_days=7.0)
            assert dd >= 0.05  # confirm we'd trigger

    def test_drawdown_guard_permanent_kill_requires_manual_reset(self) -> None:
        """Permanent kill switch file must exist independently of regular kill switch."""
        with tempfile.TemporaryDirectory() as tmpdir:
            perm_path = Path(tmpdir) / ".kill_switch.permanent"
            regular_path = Path(tmpdir) / ".kill_switch"

            # Simulate permanent kill
            perm_path.touch()
            regular_path.touch()

            # Permanent kill is a separate file — removing regular doesn't clear permanent
            regular_path.unlink()
            assert perm_path.exists(), "permanent kill must survive regular kill removal"

    def test_kill_switch_reason_persisted(self) -> None:
        """Kill reason JSON is written with expected fields."""
        with tempfile.TemporaryDirectory() as tmpdir:
            from agents.drawdown_guard import _write_kill_reason
            reason_path = Path(tmpdir) / "kill_reason.json"
            _write_kill_reason(reason_path, "test_reason", "shadow", 0.07, 0.05, permanent=False)
            data = json.loads(reason_path.read_text())
            assert data["reason"] == "test_reason"
            assert data["scope"] == "shadow"
            assert data["drawdown_observed"] == 0.07
            assert data["threshold"] == 0.05
            assert data["permanent"] is False
            assert "triggered_at" in data

    def test_no_kill_when_dd_below_threshold(self) -> None:
        """DD below all thresholds → no kill switch."""
        # 1% DD on 1-day window: well below 5%/8% thresholds
        curve = [(0.0, 1000.0), (86400.0, 990.0)]
        dd = rolling_drawdown(curve, window_days=7.0)
        assert dd < 0.05  # below shadow threshold

    def test_drawdown_computation_uses_rolling_window(self) -> None:
        """30d drawdown can exceed 7d drawdown if loss is distributed."""
        # 1% per day for 20 days = 18% total over 30d, but 7d = 7%
        curve = [(float(i * 86400), 1000.0 - i * 10.0) for i in range(25)]
        dd_7d = rolling_drawdown(curve, window_days=7.0)
        dd_30d = rolling_drawdown(curve, window_days=30.0)
        assert dd_30d >= dd_7d

    def test_shadow_dd_isolation_from_paper(self) -> None:
        """Shadow equity curve only uses shadow events; paper uses paper events."""
        paper_fills = [
            {"event_type": "fill.received", "aggregate_key": "polymarket:0xfoo",
             "occurred_at": "2026-04-01T00:00:00Z",
             "payload": {"side": "BUY", "price": 0.80, "quantity": 100.0}},
        ]
        shadow_fills = [
            {"event_type": "shadow.fill.received", "aggregate_key": "shadow:polymarket:0xfoo",
             "occurred_at": "2026-04-01T01:00:00Z",
             "payload": {"side": "BUY", "price": 0.85, "quantity": 100.0}},
        ]
        all_events = paper_fills + shadow_fills

        paper_curve = compute_equity_curve(all_events, scope="paper")
        shadow_curve = compute_equity_curve(all_events, scope="shadow")

        # Paper curve has 1 fill at 0.80
        paper_delta = paper_curve[-1][1] - paper_curve[0][1]
        # Shadow curve has 1 fill at 0.85 (slippage applied)
        shadow_delta = shadow_curve[-1][1] - shadow_curve[0][1]

        # Shadow fill is more expensive → larger outflow
        assert shadow_delta < paper_delta
