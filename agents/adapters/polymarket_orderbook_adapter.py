from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from .http_client import HttpJsonClient


@dataclass(frozen=True)
class OrderBookResult:
    token_id: str
    outcome: str
    best_bid: float | None
    best_ask: float | None
    mid_price: float | None
    spread_bps: float | None
    depth_top_n: dict[str, float] | None
    imbalance_top_n: float | None
    raw_levels_summary: dict[str, Any]
    source_quality: str
    latency_ms: int
    error: str | None


class PolymarketOrderBookAdapter:
    VERSION = "polymarket_orderbook_adapter_v2"

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

    def observe(self, outcome_tokens: list[dict[str, str]] | None) -> list[OrderBookResult]:
        if not self.enabled:
            return []
        if not outcome_tokens:
            return [
                OrderBookResult(
                    token_id="unknown",
                    outcome="unknown",
                    best_bid=None,
                    best_ask=None,
                    mid_price=None,
                    spread_bps=None,
                    depth_top_n=None,
                    imbalance_top_n=None,
                    raw_levels_summary={},
                    source_quality="missing",
                    latency_ms=0,
                    error="missing_token_ids",
                )
            ]

        out: list[OrderBookResult] = []
        for token_row in outcome_tokens:
            token_id = str(token_row.get("token_id") or "").strip()
            outcome = str(token_row.get("outcome") or "unknown")
            if not token_id:
                out.append(
                    OrderBookResult(
                        token_id="unknown",
                        outcome=outcome,
                        best_bid=None,
                        best_ask=None,
                        mid_price=None,
                        spread_bps=None,
                        depth_top_n=None,
                        imbalance_top_n=None,
                        raw_levels_summary={},
                        source_quality="missing",
                        latency_ms=0,
                        error="missing_token_id",
                    )
                )
                continue

            result = self.client.get_json("https://clob.polymarket.com/book", params={"token_id": token_id})
            if not result.ok or not isinstance(result.data, dict):
                out.append(
                    OrderBookResult(
                        token_id=token_id,
                        outcome=outcome,
                        best_bid=None,
                        best_ask=None,
                        mid_price=None,
                        spread_bps=None,
                        depth_top_n=None,
                        imbalance_top_n=None,
                        raw_levels_summary={},
                        source_quality="missing",
                        latency_ms=result.latency_ms,
                        error=_normalize_error(result.error),
                    )
                )
                continue

            bids = _parse_levels(result.data.get("bids"))
            asks = _parse_levels(result.data.get("asks"))
            best_bid = bids[0][0] if bids else None
            best_ask = asks[0][0] if asks else None
            if best_bid is None or best_ask is None:
                out.append(
                    OrderBookResult(
                        token_id=token_id,
                        outcome=outcome,
                        best_bid=best_bid,
                        best_ask=best_ask,
                        mid_price=None,
                        spread_bps=None,
                        depth_top_n={"n": float(self.top_n), "bid": sum(v for _, v in bids[: self.top_n]), "ask": sum(v for _, v in asks[: self.top_n])},
                        imbalance_top_n=None,
                        raw_levels_summary={"bids": len(bids), "asks": len(asks)},
                        source_quality="partial",
                        latency_ms=result.latency_ms,
                        error="parse_error",
                    )
                )
                continue

            mid = (best_bid + best_ask) / 2.0
            spread = _spread_bps(best_bid, best_ask)
            bid_depth = sum(level[1] for level in bids[: self.top_n])
            ask_depth = sum(level[1] for level in asks[: self.top_n])
            denom = bid_depth + ask_depth
            imbalance = ((bid_depth - ask_depth) / denom) if denom > 0 else None
            out.append(
                OrderBookResult(
                    token_id=token_id,
                    outcome=outcome,
                    best_bid=best_bid,
                    best_ask=best_ask,
                    mid_price=mid,
                    spread_bps=spread,
                    depth_top_n={"n": float(self.top_n), "bid": bid_depth, "ask": ask_depth},
                    imbalance_top_n=imbalance,
                    raw_levels_summary={
                        "bids": len(bids),
                        "asks": len(asks),
                        "top_bid_levels": bids[: self.top_n],
                        "top_ask_levels": asks[: self.top_n],
                    },
                    source_quality="read_only_orderbook",
                    latency_ms=result.latency_ms,
                    error=None,
                )
            )
        return out


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


def _normalize_error(error: str | None) -> str:
    if not error:
        return "orderbook_failed"
    if error.startswith("http_error:"):
        return "http_" + error.split(":", 1)[1]
    if error == "timeout":
        return "timeout"
    low = error.lower()
    if "name or service not known" in low or "nodename" in low or "temporary failure" in low:
        return "dns"
    if error == "invalid_json":
        return "parse_error"
    if error.startswith("url_error:"):
        return "url_error"
    return error.replace(":", "_")
