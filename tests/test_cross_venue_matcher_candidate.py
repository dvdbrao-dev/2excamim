from __future__ import annotations

import json
from argparse import Namespace
from pathlib import Path

from agents.core.event_store import load_jsonl
from agents.cross_venue_matcher_candidate import run


def _write_json(path: Path, rows: list[dict]) -> None:
    path.write_text(json.dumps(rows), encoding="utf-8")


def _run(tmp_path: Path, poly: list[dict], kalshi: list[dict], min_conf: float = 0.9) -> list[dict]:
    poly_p = tmp_path / "poly.json"
    kalshi_p = tmp_path / "kalshi.json"
    out_p = tmp_path / "out.jsonl"
    _write_json(poly_p, poly)
    _write_json(kalshi_p, kalshi)

    run(
        Namespace(
            polymarket_markets_json=str(poly_p),
            kalshi_markets_json=str(kalshi_p),
            output_jsonl=str(out_p),
            min_confidence=min_conf,
            dry_run=False,
        )
    )
    return load_jsonl(out_p)


def test_exact_btc_hourly_match(tmp_path: Path) -> None:
    events = _run(
        tmp_path,
        poly=[{"id": "pm-1", "title": "BTC hourly above $70000"}],
        kalshi=[{"id": "ks-1", "title": "BTC hourly above $70000"}],
    )
    payload = events[-1]["payload"]
    assert events[-1]["event_type"] == "cross_venue.market_match_scored"
    assert payload["rejected"] is False
    assert payload["asset"] == "BTC"
    assert payload["window"] == "1h"
    assert payload["strike"] == 70000.0


def test_false_positive_similar_text_rejected(tmp_path: Path) -> None:
    events = _run(
        tmp_path,
        poly=[{"id": "pm-1", "title": "BTC hourly above $70000"}],
        kalshi=[{"id": "ks-1", "title": "ETH hourly above $70000"}],
    )
    payload = events[-1]["payload"]
    assert payload["rejected"] is True
    assert payload["reject_reason"] == "asset_mismatch"


def test_different_strike_rejected(tmp_path: Path) -> None:
    events = _run(
        tmp_path,
        poly=[{"id": "pm-1", "title": "BTC hourly above $70000"}],
        kalshi=[{"id": "ks-1", "title": "BTC hourly above $71000"}],
    )
    payload = events[-1]["payload"]
    assert payload["rejected"] is True
    assert payload["reject_reason"] == "strike_mismatch"


def test_different_settlement_window_rejected(tmp_path: Path) -> None:
    events = _run(
        tmp_path,
        poly=[{"id": "pm-1", "title": "BTC 5m above $70000"}],
        kalshi=[{"id": "ks-1", "title": "BTC 15m above $70000"}],
    )
    payload = events[-1]["payload"]
    assert payload["rejected"] is True
    assert payload["reject_reason"] == "settlement_window_mismatch"
