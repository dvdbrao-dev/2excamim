from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from .http_client import HttpJsonClient


@dataclass(frozen=True)
class OrderBookResult:
    best_bid: float | None
    best_ask: float | None
    spread_bps: float | None
    depth_top_n: dict[str, float] | None
    imbalance_top_n: float | None
    latency_ms: int
    error: str | None


class PolymarketOrderBookAdapter:
    VERSION = "polymarket_orderbook_adapter_v1"

    def __init__(
        self,
        timeout_sec: float = 5.0,
        max_retries: int = 2,
        top_n: int = 2,
        enabled: bool = False,
        client: HttpJsonClient | None = None,
    ) -> None:
        self.client = client or HttpJsonClient(timeout_sec=timeout_sec, max_retries=max_retries)
        self.top_n = max(1, top_n)
        self.enabled = enabled

    def observe(self, token_ids: list[str] | None) -> OrderBookResult:
        if not self.enabled:
            return OrderBookResult(None, None, None, None, None, 0, "orderbook_adapter_disabled")
        if not token_ids:
            return OrderBookResult(None, None, None, None, None, 0, "missing_token_ids")

        # Public CLOB endpoint (read-only). If unavailable in an environment, fail safely.
        result = self.client.get_json("https://clob.polymarket.com/book", params={"token_id": token_ids[0]})
        if not result.ok or not isinstance(result.data, dict):
            return OrderBookResult(None, None, None, None, None, result.latency_ms, result.error or "orderbook_failed")

        bids = _parse_levels(result.data.get("bids"))
        asks = _parse_levels(result.data.get("asks"))
        best_bid = bids[0][0] if bids else None
        best_ask = asks[0][0] if asks else None
        spread = _spread_bps(best_bid, best_ask)
        bid_depth = sum(level[1] for level in bids[: self.top_n])
        ask_depth = sum(level[1] for level in asks[: self.top_n])
        denom = bid_depth + ask_depth
        imbalance = ((bid_depth - ask_depth) / denom) if denom > 0 else None
        return OrderBookResult(
            best_bid=best_bid,
            best_ask=best_ask,
            spread_bps=spread,
            depth_top_n={"n": float(self.top_n), "bid": bid_depth, "ask": ask_depth},
            imbalance_top_n=imbalance,
            latency_ms=result.latency_ms,
            error=None,
        )


def _parse_levels(value: Any) -> list[tuple[float, float]]:
    if not isinstance(value, list):
        return []
    levels: list[tuple[float, float]] = []
    for row in value:
        if not isinstance(row, dict):
            continue
        try:
            price = float(row.get("price"))
            size = float(row.get("size"))
            levels.append((price, size))
        except (TypeError, ValueError):
            continue
    return levels


def _spread_bps(bid: float | None, ask: float | None) -> float | None:
    if bid is None or ask is None:
        return None
    mid = (bid + ask) / 2.0
    if mid <= 0:
        return None
    return ((ask - bid) / mid) * 10_000.0
