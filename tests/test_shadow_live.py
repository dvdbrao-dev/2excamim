"""Tests for shadow live agent."""
from __future__ import annotations

import json
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from agents.shadow_live_agent import (
    build_shadow_fill_event,
    collect_unprocessed_decisions,
    _apply_slippage,
    _market_midpoint,
    _is_crypto_instrument,
)
from agents.core.equity_curve import compute_equity_curve, rolling_drawdown


def _make_decision(decision_id: str, instrument: str = "polymarket:0xtest", size: float = 50.0) -> dict:
    return {
        "event_type": "decision.formed",
        "event_id": f"ev-{decision_id}",
        "aggregate_key": instrument,
        "occurred_at": "2026-04-15T10:00:00Z",
        "payload": {
            "decision_id": decision_id,
            "instrument": instrument,
            "side": "YES",
            "size_hint": size,
        },
        "linkage": {"decision_id": decision_id},
    }


def _make_shadow_fill(decision_id: str) -> dict:
    return {
        "event_type": "shadow.fill.received",
        "aggregate_key": "shadow:polymarket:0xtest",
        "payload": {"decision_id": decision_id},
        "linkage": {"decision_id": decision_id},
    }


class TestShadowFillSlippage:
    def test_uses_midpoint_plus_slippage_not_favorable_price(self) -> None:
        """Shadow fill must apply adverse slippage, not use favorable price."""
        midpoint = 0.80
        slippage_bps = 30.0
        fill_price = _apply_slippage(midpoint, "YES", slippage_bps)
        expected = midpoint * (1 + slippage_bps / 10_000.0)
        assert abs(fill_price - expected) < 1e-9
        assert fill_price > midpoint  # worse than midpoint

    def test_shadow_fill_price_worse_than_midpoint(self) -> None:
        """Fill price must always be more expensive than the snapshot midpoint."""
        for mid in [0.30, 0.50, 0.80, 0.90]:
            fill = _apply_slippage(mid, "YES", 30.0)
            assert fill > mid

    def test_crypto_slippage_lower(self) -> None:
        mid = 0.50
        poly_fill = _apply_slippage(mid, "YES", 30.0)
        crypto_fill = _apply_slippage(mid, "YES", 5.0)
        assert crypto_fill < poly_fill

    def test_is_crypto_instrument(self) -> None:
        assert _is_crypto_instrument("BTCUSDT")
        assert not _is_crypto_instrument("0xabc123")
        assert not _is_crypto_instrument("polymarket:0xabc")


class TestShadowEventIdempotency:
    def test_idempotent_by_decision_id(self) -> None:
        """If shadow fill already exists for decision_id, it's excluded from pending."""
        d1 = _make_decision("dec-001")
        shadow = _make_shadow_fill("dec-001")
        events = [d1, shadow]

        pending = collect_unprocessed_decisions(events, processed_decision_ids=set())
        # dec-001 already shadow-filled → not in pending
        assert not any(
            (e.get("payload") or {}).get("decision_id") == "dec-001"
            for e in pending
        )

    def test_unprocessed_decision_is_returned(self) -> None:
        d2 = _make_decision("dec-002")
        events = [d2]
        pending = collect_unprocessed_decisions(events, processed_decision_ids=set())
        assert any(
            (e.get("payload") or {}).get("decision_id") == "dec-002"
            for e in pending
        )

    def test_processed_set_excludes_known(self) -> None:
        d3 = _make_decision("dec-003")
        events = [d3]
        pending = collect_unprocessed_decisions(events, processed_decision_ids={"dec-003"})
        assert len(pending) == 0

    def test_shadow_event_aggregate_key_has_prefix(self) -> None:
        decision = _make_decision("dec-100", instrument="polymarket:0xabc")
        fill_event = build_shadow_fill_event(decision, fill_price=0.82, quantity=60.0,
                                              run_id="run-1", slippage_bps=30.0)
        assert fill_event["aggregate_key"].startswith("shadow:")

    def test_shadow_event_idempotency_key_stable(self) -> None:
        decision = _make_decision("dec-200")
        fill1 = build_shadow_fill_event(decision, 0.80, 50.0, "run-1", 30.0)
        fill2 = build_shadow_fill_event(decision, 0.80, 50.0, "run-2", 30.0)
        assert fill1["idempotency_key"] == fill2["idempotency_key"]


class TestEquityCurve:
    def _make_fill_event(self, side: str, price: float, qty: float, ts: str, shadow: bool = False) -> dict:
        agg = "shadow:polymarket:0xfoo" if shadow else "polymarket:0xfoo"
        return {
            "event_type": "shadow.fill.received" if shadow else "fill.received",
            "aggregate_key": agg,
            "occurred_at": ts,
            "payload": {
                "side": side,
                "price": price,
                "quantity": qty,
                "instrument": "polymarket:0xfoo",
            },
        }

    def test_equity_curve_monotonic_after_winning_trade(self) -> None:
        events = [
            self._make_fill_event("BUY", 0.80, 100.0, "2026-04-01T00:00:00Z"),
            self._make_fill_event("SELL", 1.00, 100.0, "2026-04-02T00:00:00Z"),
        ]
        curve = compute_equity_curve(events, initial_bankroll=1000.0, scope="paper")
        # After BUY: equity drops (outflow), after SELL: equity rises (inflow)
        assert len(curve) >= 2
        final_equity = curve[-1][1]
        assert final_equity > 1000.0 - 0.80 * 100.0  # at least partial recovery

    def test_equity_curve_scope_separation(self) -> None:
        """Paper scope ignores shadow events; shadow scope includes only shadow events."""
        paper_fill = self._make_fill_event("SELL", 1.0, 50.0, "2026-04-01T01:00:00Z", shadow=False)
        shadow_fill = self._make_fill_event("SELL", 1.0, 50.0, "2026-04-01T02:00:00Z", shadow=True)
        events = [paper_fill, shadow_fill]

        paper_curve = compute_equity_curve(events, scope="paper")
        shadow_curve = compute_equity_curve(events, scope="shadow")

        assert len(paper_curve) == 2  # initial + 1 paper fill
        assert len(shadow_curve) == 2  # initial + 1 shadow fill

    def test_rolling_drawdown_recovers_after_recovery(self) -> None:
        # Equity drops then recovers: drawdown should be 0 at end
        curve = [
            (0.0, 1000.0),
            (86400.0, 950.0),   # day 1: drop
            (172800.0, 1000.0), # day 2: recovery
            (259200.0, 1050.0), # day 3: new high
        ]
        dd = rolling_drawdown(curve, window_days=7.0)
        # max dd during window: (1000 - 950) / 1000 = 0.05
        assert abs(dd - 0.05) < 0.01

    def test_rolling_drawdown_zero_for_monotonic_increase(self) -> None:
        curve = [(float(i * 86400), 1000.0 + i * 10.0) for i in range(10)]
        dd = rolling_drawdown(curve, window_days=7.0)
        assert dd == 0.0

    def test_rolling_drawdown_empty_curve(self) -> None:
        assert rolling_drawdown([], window_days=7.0) == 0.0
        assert rolling_drawdown([(0.0, 1000.0)], window_days=7.0) == 0.0


class TestPaperVsShadowDivergence:
    def test_paper_vs_shadow_divergence_calculation(self) -> None:
        from scripts.shadow_vs_paper_report import compute_divergence
        paper = [(0.0, 1000.0), (86400.0, 1050.0)]   # paper: +50
        shadow = [(0.0, 1000.0), (86400.0, 1030.0)]  # shadow: +30
        div = compute_divergence(paper, shadow, initial_bankroll=1000.0)
        assert div["paper_pnl"] == 50.0
        assert div["shadow_pnl"] == 30.0
        assert div["divergence_abs"] == 20.0
        assert abs(div["divergence_pct"] - 0.40) < 0.01  # 20/50 = 40%
        assert div["paper_optimism_flag"] is True

    def test_no_optimism_flag_within_threshold(self) -> None:
        from scripts.shadow_vs_paper_report import compute_divergence
        paper = [(0.0, 1000.0), (86400.0, 1100.0)]
        shadow = [(0.0, 1000.0), (86400.0, 1090.0)]  # 9% divergence
        div = compute_divergence(paper, shadow, initial_bankroll=1000.0)
        assert div["paper_optimism_flag"] is False
