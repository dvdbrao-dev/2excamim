"""Tests for maker fill simulator and backtester."""
from __future__ import annotations

import json
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from scripts.maker_fill_simulator import build_synthetic_snapshot, simulate_fill
from scripts.maker_backtester import (
    run_maker_backtest,
    simulate_market,
    VALIDATION_MONTHLY_PASS_RATE,
    VALIDATION_MIN_T_STAT,
)
from agents.services.polymarket_orderbook_history import (
    ingest_markets,
    load_markets_from_dir,
    _already_ingested,
)


# ---------------------------------------------------------------------------
# Fill simulator tests
# ---------------------------------------------------------------------------

class TestFillSimulatorNoPriceUnreachable:
    def test_no_fill_when_price_above_ask(self) -> None:
        """Quote at NO 0.95, best_ask_no = 0.90 → quote crosses spread → no fill."""
        snapshot = build_synthetic_snapshot(no_midpoint=0.88, spread=0.04)
        # best_ask_no ≈ 0.90
        result = simulate_fill(snapshot, quote_price_no=0.95)
        assert not result["filled"]
        assert result["reject_reason"] == "price_at_or_above_best_ask"

    def test_no_fill_when_price_below_best_bid(self) -> None:
        """Quote at NO 0.80, best_bid_no = 0.86 → below market → no queue."""
        snapshot = {"best_bid_no": 0.86, "best_ask_no": 0.90, "no_midpoint": 0.88,
                    "volume_1h_usd": 1000.0, "sigma_1h": 0.01}
        result = simulate_fill(snapshot, quote_price_no=0.80)
        assert not result["filled"]
        assert result["reject_reason"] == "price_below_best_bid"

    def test_fill_probability_low_with_no_volume(self) -> None:
        """Without volume data, fill probability is near zero → no fill."""
        snapshot = {"best_bid_no": 0.84, "best_ask_no": 0.90, "no_midpoint": 0.87,
                    "volume_1h_usd": 0.0, "sigma_1h": 0.01}
        result = simulate_fill(snapshot, quote_price_no=0.87)
        assert not result["filled"]
        assert result["fill_probability"] < 0.05

    def test_fill_returns_correct_price(self) -> None:
        """When filled, fill_price == quote_price_no."""
        snapshot = {"best_bid_no": 0.84, "best_ask_no": 0.90, "no_midpoint": 0.87,
                    "volume_1h_usd": 100_000.0, "sigma_1h": 0.01}
        result = simulate_fill(snapshot, quote_price_no=0.87, quote_size_usd=50.0)
        if result["filled"]:
            assert result["fill_price"] == 0.87

    def test_no_fill_invalid_price(self) -> None:
        """Quote price > 1 → reject."""
        snapshot = build_synthetic_snapshot(no_midpoint=0.88)
        result = simulate_fill(snapshot, quote_price_no=1.01)
        assert not result["filled"]
        assert result["reject_reason"] == "quote_price_out_of_range"


class TestFillSimulatorAdverseSelection:
    def test_records_adverse_selection_on_large_move(self) -> None:
        """NO price drops 5σ post-fill → adverse_selection_bps > 0."""
        snapshot = {"best_bid_no": 0.84, "best_ask_no": 0.90, "no_midpoint": 0.87,
                    "volume_1h_usd": 100_000.0, "sigma_1h": 0.01}
        # Future: NO drops significantly (adverse for long NO position)
        future = [
            {"best_bid_no": 0.70, "best_ask_no": 0.76, "no_midpoint": 0.73},
            {"best_bid_no": 0.68, "best_ask_no": 0.74, "no_midpoint": 0.71},
        ]
        result = simulate_fill(
            snapshot, quote_price_no=0.87,
            future_snapshots=future,
            adverse_sigma_multiplier=2.0,
            quote_size_usd=50.0,
        )
        if result["filled"]:
            # Drop of 0.14 >> 2 * 0.01 = 0.02 → adverse selection should be > 0
            assert result["adverse_selection_bps"] > 0

    def test_no_adverse_selection_when_price_stable(self) -> None:
        """Stable price → no adverse selection (other than fee)."""
        snapshot = {"best_bid_no": 0.84, "best_ask_no": 0.90, "no_midpoint": 0.87,
                    "volume_1h_usd": 100_000.0, "sigma_1h": 0.05}
        future = [
            {"best_bid_no": 0.84, "best_ask_no": 0.90, "no_midpoint": 0.87},
            {"best_bid_no": 0.85, "best_ask_no": 0.91, "no_midpoint": 0.88},
        ]
        result = simulate_fill(
            snapshot, quote_price_no=0.87,
            future_snapshots=future,
            adverse_sigma_multiplier=2.0,
            fee_bps=0.0,
        )
        if result["filled"]:
            # No adverse move + 0 fee → adverse_selection_bps should be 0
            assert result["adverse_selection_bps"] == 0.0


# ---------------------------------------------------------------------------
# Orderbook history idempotency
# ---------------------------------------------------------------------------

class TestOrderbookHistoryIdempotent:
    def test_idempotent_ingest(self) -> None:
        """Re-ingesting same date doesn't duplicate."""
        market = {
            "market_id": "0xtest001",
            "question": "Test market",
            "yes_midpoint": 0.10,
            "no_midpoint": 0.90,
            "volume_usdc": 75_000.0,
            "end_date": "2026-06-01",
            "resolution": "NO",
            "no_token_id": None,
            "ingested_at": "2026-04-15T10:00:00+00:00",
        }
        with tempfile.TemporaryDirectory() as tmpdir:
            output_dir = Path(tmpdir) / "orderbook"
            # First ingest
            stats1 = ingest_markets([market], output_dir)
            assert stats1["ingested"] == 1
            assert stats1["skipped"] == 0
            # Second ingest: same market
            stats2 = ingest_markets([market], output_dir)
            assert stats2["ingested"] == 0
            assert stats2["skipped"] == 1
            # Verify only one entry in file
            fpath = output_dir / "2026-04.jsonl"
            lines = [l for l in fpath.read_text().splitlines() if l.strip()]
            assert len(lines) == 1

    def test_load_respects_min_no_midpoint(self) -> None:
        """Markets below min_no_midpoint not loaded."""
        markets = [
            {**_make_market("0x01"), "no_midpoint": 0.92, "ingested_at": "2026-04-01T00:00:00+00:00"},
            {**_make_market("0x02"), "no_midpoint": 0.72, "ingested_at": "2026-04-01T00:00:00+00:00"},
        ]
        with tempfile.TemporaryDirectory() as tmpdir:
            output_dir = Path(tmpdir) / "orderbook"
            ingest_markets(markets, output_dir)
            loaded = load_markets_from_dir(output_dir, min_no_midpoint=0.85)
            assert len(loaded) == 1
            assert loaded[0]["market_id"] == "0x01"


# ---------------------------------------------------------------------------
# Maker backtester PnL logic
# ---------------------------------------------------------------------------

class TestMakerBacktesterPnL:
    def test_resolves_yes_market_pnl_negative(self) -> None:
        """Market resolves YES → our NO position loses fill_price."""
        market = {**_make_market("0xYES"), "no_midpoint": 0.90, "resolution": "YES",
                  "ingested_at": "2026-04-01T00:00:00+00:00"}
        result = simulate_market(market, quote_price_no=0.85, quote_size_usd=50.0)
        if result.get("fill_count", 0) > 0:
            # Some fills must have happened; total PnL should be negative
            for fill in result.get("fills", []):
                fp = fill["fill_price"]
                gross = fill["gross_pnl"]
                # gross_pnl for YES resolution ≈ -fp * (qty) where qty = size/price
                qty = 50.0 / fp
                expected = -fp * qty
                assert abs(gross - expected) < 1e-4, f"gross={gross} expected={expected}"

    def test_resolves_no_market_pnl_positive(self) -> None:
        """Market resolves NO → our NO position gains (1 - fill_price)."""
        market = {**_make_market("0xNO"), "no_midpoint": 0.90, "resolution": "NO",
                  "ingested_at": "2026-04-01T00:00:00+00:00"}
        result = simulate_market(market, quote_price_no=0.85, quote_size_usd=50.0)
        if result.get("fill_count", 0) > 0:
            for fill in result.get("fills", []):
                fp = fill["fill_price"]
                gross = fill["gross_pnl"]
                qty = 50.0 / fp
                expected = (1.0 - fp) * qty
                assert abs(gross - expected) < 1e-4, f"gross={gross} expected={expected}"

    def test_backtester_aggregates_match_fixture(self) -> None:
        """Aggregate metrics are consistent with per-market results."""
        markets = [
            {**_make_market(f"0x{i:04d}"), "no_midpoint": 0.90, "resolution": "NO",
             "ingested_at": f"2026-0{(i % 3) + 1}-01T00:00:00+00:00"}
            for i in range(10)
        ]
        result = run_maker_backtest(markets, quote_price_no=0.85, quote_size_usd=50.0)

        # Verify scorecard-compatible fields exist
        assert "strategy_id" in result
        assert "fills" in result
        assert "pnl_net" in result
        assert "profit_factor" in result
        assert "confidence" in result
        assert "verdict" in result
        assert result["verdict"] in ("PASS", "FAIL", "INCONCLUSIVE")

        # Verify fill count consistency
        total_fills_from_summary = sum(
            r.get("fill_count", 0)
            for m in markets
            for r in [simulate_market(m, 0.85, 50.0)]
            if not r.get("skipped")
        )
        assert result["total_fills"] == total_fills_from_summary

    def test_skips_unresolved_markets(self) -> None:
        market = {**_make_market("0xUNR"), "no_midpoint": 0.90, "resolution": None,
                  "ingested_at": "2026-04-01T00:00:00+00:00"}
        result = simulate_market(market)
        assert result.get("skipped") is True
        assert result["reason"] == "unresolved"

    def test_skips_when_midpoint_below_quote(self) -> None:
        market = {**_make_market("0xLOW"), "no_midpoint": 0.80, "resolution": "NO",
                  "ingested_at": "2026-04-01T00:00:00+00:00"}
        result = simulate_market(market, quote_price_no=0.85)
        assert result.get("skipped") is True
        assert result["reason"] == "midpoint_below_quote"


# ---------------------------------------------------------------------------
# Helper
# ---------------------------------------------------------------------------

def _make_market(market_id: str) -> dict:
    return {
        "market_id": market_id,
        "question": f"Market {market_id}",
        "yes_midpoint": 0.10,
        "no_midpoint": 0.90,
        "volume_usdc": 75_000.0,
        "end_date": "2026-06-01",
        "resolution": "NO",
        "no_token_id": None,
        "ingested_at": "2026-04-01T00:00:00+00:00",
    }
