from __future__ import annotations

from typing import Any

import requests

from research_prediction_markets.schemas.records import (
    MarketRecord,
    TradeRecord,
    normalize_amount,
    normalize_probability,
    parse_datetime,
    utc_now,
)


class PolymarketClient:
    GAMMA_BASE_URL = "https://gamma-api.polymarket.com"
    DATA_BASE_URL = "https://data-api.polymarket.com"

    def __init__(self, session: requests.Session | None = None, timeout: int = 15) -> None:
        self.session = session or requests.Session()
        self.timeout = timeout

    def _get_json(self, base_url: str, path: str, params: dict[str, Any] | None = None) -> Any:
        url = f"{base_url}{path}"
        response = self.session.get(url, params=params, timeout=self.timeout)
        response.raise_for_status()
        return response.json()

    def get_markets(self, limit: int = 200, closed: bool = False) -> list[MarketRecord]:
        fetched_at = utc_now()
        params = {"limit": limit, "closed": str(closed).lower()}
        try:
            payload = self._get_json(self.GAMMA_BASE_URL, "/markets", params=params)
        except requests.RequestException:
            return []

        items = payload if isinstance(payload, list) else payload.get("data", [])
        records: list[MarketRecord] = []
        for item in items:
            market = self._parse_market(item, fetched_at=fetched_at)
            if market is not None:
                records.append(market)
        return records

    def get_recent_trades(self, limit: int = 500) -> list[TradeRecord]:
        try:
            payload = self._get_json(self.DATA_BASE_URL, "/trades", params={"limit": limit})
        except requests.RequestException:
            return []

        items = payload if isinstance(payload, list) else payload.get("data", [])
        records: list[TradeRecord] = []
        for item in items:
            trade = self._parse_trade(item)
            if trade is not None:
                records.append(trade)
        return records

    def _parse_market(self, item: dict[str, Any], fetched_at) -> MarketRecord | None:
        market_id = str(item.get("conditionId") or item.get("id") or "").strip()
        question = str(item.get("question") or item.get("title") or market_id).strip()
        if not market_id or not question:
            return None

        outcome_prices = item.get("outcomePrices")
        probability = self._extract_yes_probability(item, outcome_prices)

        best_bid = item.get("bestBid")
        best_ask = item.get("bestAsk")
        if best_bid is None or best_ask is None:
            best_bid, best_ask = self._fallback_bid_ask(probability)

        active = bool(item.get("active", False))
        closed = bool(item.get("closed", False))
        archived = bool(item.get("archived", False))
        if archived or bool(item.get("resolved", False)):
            status = "resolved"
        elif closed or not active:
            status = "closed"
        else:
            status = "open"

        return MarketRecord(
            market_id=market_id,
            source="polymarket",
            question=question,
            probability=probability,
            yes_bid=normalize_probability(best_bid),
            yes_ask=normalize_probability(best_ask),
            volume_24h=normalize_amount(item.get("volume24hr", item.get("volume24h"))),
            open_interest=normalize_amount(item.get("liquidity", item.get("openInterest"))),
            status=status,
            resolution_time=parse_datetime(item.get("endDate") or item.get("end_date_iso")),
            fetched_at=fetched_at,
        )

    def _parse_trade(self, item: dict[str, Any]) -> TradeRecord | None:
        trade_id = str(item.get("transactionHash") or item.get("id") or "").strip()
        market_id = str(item.get("conditionId") or item.get("market") or "").strip()
        if not trade_id or not market_id:
            return None

        outcome = str(item.get("outcome") or "").strip().lower()
        if outcome == "yes":
            side = "yes"
        elif outcome == "no":
            side = "no"
        else:
            side = "yes"

        return TradeRecord(
            trade_id=trade_id,
            market_id=market_id,
            source="polymarket",
            price=normalize_probability(item.get("price")),
            size=normalize_amount(item.get("size")),
            side=side,
            timestamp=parse_datetime(item.get("timestamp")) or utc_now(),
            taker=str(item.get("side", "")).upper() == "BUY",
        )

    def _extract_yes_probability(self, item: dict[str, Any], outcome_prices: Any) -> float:
        if isinstance(outcome_prices, str):
            stripped = outcome_prices.strip()
            if stripped.startswith("[") and stripped.endswith("]"):
                stripped = stripped[1:-1]
            parts = [part.strip().strip('"') for part in stripped.split(",") if part.strip()]
            if parts:
                return normalize_probability(parts[0])

        if isinstance(outcome_prices, list) and outcome_prices:
            return normalize_probability(outcome_prices[0])

        token_ids = item.get("clobTokenIds")
        if token_ids and item.get("lastTradePrice") is not None:
            return normalize_probability(item.get("lastTradePrice"))

        return normalize_probability(item.get("probability", item.get("price")))

    def _fallback_bid_ask(self, probability: float) -> tuple[float, float]:
        spread = 0.02
        yes_bid = max(0.0, probability - spread / 2.0)
        yes_ask = min(1.0, probability + spread / 2.0)
        return yes_bid, yes_ask
