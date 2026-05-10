from __future__ import annotations

from dataclasses import dataclass
from typing import Any

import urllib.request

from agents.adapters.http_client import HttpJsonClient
from agents.adapters.polymarket_metadata_adapter import PolymarketMetadataAdapter
from agents.adapters.polymarket_orderbook_adapter import PolymarketOrderBookAdapter


@dataclass
class FakeResult:
    ok: bool
    data: Any
    error: str | None
    latency_ms: int


class SeqClient:
    def __init__(self, results: list[FakeResult]) -> None:
        self._results = results
        self.calls: list[tuple[str, dict[str, str] | None]] = []

    def get_json(self, url: str, params: dict[str, str] | None = None):
        self.calls.append((url, params))
        if not self._results:
            raise AssertionError("unexpected get_json call")
        row = self._results.pop(0)
        return type("Resp", (), {"ok": row.ok, "data": row.data, "error": row.error, "latency_ms": row.latency_ms})


def test_http_client_injects_polymarket_headers_gamma(monkeypatch) -> None:
    captured: dict[str, str] = {}

    class _Resp:
        status = 200

        def __enter__(self):
            return self

        def __exit__(self, exc_type, exc, tb):
            return False

        def read(self) -> bytes:
            return b"{}"

    def _fake_urlopen(request: urllib.request.Request, timeout: float):
        captured["accept"] = request.get_header("Accept") or ""
        captured["user_agent"] = request.get_header("User-agent") or ""
        return _Resp()

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    out = HttpJsonClient(max_retries=0).get_json("https://gamma-api.polymarket.com/markets", params={"slug": "x"})
    assert out.ok is True
    assert captured["accept"] == "application/json"
    assert captured["user_agent"] == "Mozilla/5.0"


def test_http_client_injects_polymarket_headers_clob(monkeypatch) -> None:
    captured: dict[str, str] = {}

    class _Resp:
        status = 200

        def __enter__(self):
            return self

        def __exit__(self, exc_type, exc, tb):
            return False

        def read(self) -> bytes:
            return b"{}"

    def _fake_urlopen(request: urllib.request.Request, timeout: float):
        captured["accept"] = request.get_header("Accept") or ""
        captured["user_agent"] = request.get_header("User-agent") or ""
        return _Resp()

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    out = HttpJsonClient(max_retries=0).get_json("https://clob.polymarket.com/book", params={"token_id": "10"})
    assert out.ok is True
    assert captured["accept"] == "application/json"
    assert captured["user_agent"] == "Mozilla/5.0"


def test_http_client_does_not_change_binance_headers(monkeypatch) -> None:
    captured: dict[str, str] = {}

    class _Resp:
        status = 200

        def __enter__(self):
            return self

        def __exit__(self, exc_type, exc, tb):
            return False

        def read(self) -> bytes:
            return b"{}"

    def _fake_urlopen(request: urllib.request.Request, timeout: float):
        captured["accept"] = request.get_header("Accept") or ""
        captured["user_agent"] = request.get_header("User-agent") or ""
        return _Resp()

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    out = HttpJsonClient(max_retries=0).get_json("https://api.binance.com/api/v3/ticker/price", params={"symbol": "BTCUSDT"})
    assert out.ok is True
    assert captured["accept"] == "application/json"
    assert captured["user_agent"] == ""


def test_http_client_headers_are_overrideable(monkeypatch) -> None:
    captured: dict[str, str] = {}

    class _Resp:
        status = 200

        def __enter__(self):
            return self

        def __exit__(self, exc_type, exc, tb):
            return False

        def read(self) -> bytes:
            return b"{}"

    def _fake_urlopen(request: urllib.request.Request, timeout: float):
        captured["accept"] = request.get_header("Accept") or ""
        captured["user_agent"] = request.get_header("User-agent") or ""
        return _Resp()

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    out = HttpJsonClient(max_retries=0).get_json(
        "https://gamma-api.polymarket.com/markets",
        params={"slug": "x"},
        headers={"User-Agent": "Custom-UA", "Accept": "application/custom+json"},
    )
    assert out.ok is True
    assert captured["accept"] == "application/custom+json"
    assert captured["user_agent"] == "Custom-UA"


def test_metadata_success_by_slug() -> None:
    client = SeqClient(
        [
            FakeResult(
                ok=True,
                data=[
                    {
                        "id": "m1",
                        "conditionId": "c1",
                        "question": "BTC up or down in 5m",
                        "active": True,
                        "closed": False,
                        "resolved": False,
                        "outcomes": ["YES", "NO"],
                        "clobTokenIds": ["10", "11"],
                        "endDate": "2026-05-10T00:05:00Z",
                    }
                ],
                error=None,
                latency_ms=7,
            )
        ]
    )
    out = PolymarketMetadataAdapter(client=client).observe("btc-updown-5m-1778361600")
    assert out.error is None
    assert out.market_id == "m1"
    assert out.outcome_tokens[0]["token_id"] == "10"
    assert out.match_reason == "exact_canonical_slug"


def test_metadata_not_found_emits_structured_error() -> None:
    client = SeqClient([FakeResult(ok=True, data=[], error=None, latency_ms=6), FakeResult(ok=True, data=[], error=None, latency_ms=8)])
    out = PolymarketMetadataAdapter(client=client).observe("btc-updown-5m-1778361600")
    assert out.error == "metadata_not_found"
    assert out.match_reason in {"fallback_no_match", "slug_not_parseable"}


def test_metadata_ambiguous_fallback_rejected() -> None:
    client = SeqClient(
        [
            FakeResult(ok=True, data=[], error=None, latency_ms=5),
            FakeResult(
                ok=True,
                data=[
                    {"question": "BTC up or down 5m", "endDate": "2026-05-10T00:00:30Z", "active": True, "closed": False},
                    {"question": "BTC up or down 5m", "endDate": "2026-05-10T00:00:40Z", "active": True, "closed": False},
                ],
                error=None,
                latency_ms=5,
            ),
        ]
    )
    out = PolymarketMetadataAdapter(client=client).observe("btc-updown-5m-1778361600")
    assert out.error == "metadata_ambiguous_fallback"


def test_metadata_exact_404_kept_distinct_from_fallback_403() -> None:
    client = SeqClient(
        [
            FakeResult(ok=False, data=None, error="http_error:404", latency_ms=4),
            FakeResult(ok=False, data=None, error="http_error:403", latency_ms=5),
        ]
    )
    out = PolymarketMetadataAdapter(client=client).observe("btc-updown-5m-1778361600")
    assert out.error == "http_404"
    assert out.adapter_errors["exact_slug"] == "http_404"
    assert out.adapter_errors["fallback_search"] == "http_403"


def test_orderbook_success_parses_levels() -> None:
    client = SeqClient(
        [
            FakeResult(
                ok=True,
                data={
                    "bids": [{"price": "0.48", "size": "1000"}],
                    "asks": [{"price": "0.50", "size": "900"}],
                },
                error=None,
                latency_ms=9,
            )
        ]
    )
    rows = PolymarketOrderBookAdapter(enabled=True, client=client, top_n=2).observe([{"outcome": "YES", "token_id": "10"}])
    assert len(rows) == 1
    assert rows[0].error is None
    assert rows[0].best_bid == 0.48
    assert rows[0].best_ask == 0.5
    assert rows[0].mid_price == 0.49
    assert rows[0].spread_bps is not None


def test_orderbook_parse_error_is_structured() -> None:
    client = SeqClient([FakeResult(ok=True, data={"bids": [], "asks": []}, error=None, latency_ms=4)])
    rows = PolymarketOrderBookAdapter(enabled=True, client=client).observe([{"outcome": "YES", "token_id": "10"}])
    assert rows[0].error == "parse_error"
