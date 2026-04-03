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


class KalshiClient:
    BASE_URL = "https://api.elections.kalshi.com/trade-api/v2"

    def __init__(self, session: requests.Session | None = None, timeout: int = 15) -> None:
        self.session = session or requests.Session()
        self.timeout = timeout

    def _get_json(self, path: str, params: dict[str, Any] | None = None) -> Any:
        url = f"{self.BASE_URL}{path}"
        response = self.session.get(url, params=params, timeout=self.timeout)
        response.raise_for_status()
        return response.json()

    def get_markets(self, limit: int = 200, status: str = "open") -> list[MarketRecord]:
        fetched_at = utc_now()
        try:
            payload = self._get_json("/markets", params={"limit": limit, "status": status})
        except requests.RequestException:
            return []

        records: list[MarketRecord] = []
        for item in payload.get("markets", []):
            market = self._parse_market(item, fetched_at=fetched_at)
            if market is not None:
                records.append(market)
        return records

    def get_trades(self) -> list[TradeRecord]:
        # Kalshi documents a public trades endpoint, but schema details are not stable
        # enough here for a confident MVP parser. Keep ingestion conservative.
        return []

    def _parse_market(self, item: dict[str, Any], fetched_at) -> MarketRecord | None:
        market_id = str(item.get("ticker") or "").strip()
        question = str(
            item.get("title")
            or item.get("subtitle")
            or item.get("yes_sub_title")
            or market_id
        ).strip()
        if not market_id or not question:
            return None

        yes_bid = normalize_probability(
            item.get("yes_bid_dollars", item.get("yes_bid", item.get("best_bid")))
        )
        yes_ask = normalize_probability(
            item.get("yes_ask_dollars", item.get("yes_ask", item.get("best_ask")))
        )
        probability = normalize_probability(
            item.get("last_price_dollars", item.get("last_price", (yes_bid + yes_ask) / 2.0))
        )

        raw_status = str(item.get("status", "open")).lower()
        if raw_status in {"settled", "resolved"}:
            status = "resolved"
        elif raw_status in {"closed", "finalized"}:
            status = "closed"
        else:
            status = "open"

        resolution_time = parse_datetime(
            item.get("latest_expiration_time")
            or item.get("expiration_time")
            or item.get("close_time")
            or item.get("settled_time")
        )

        return MarketRecord(
            market_id=market_id,
            source="kalshi",
            question=question,
            probability=probability,
            yes_bid=yes_bid,
            yes_ask=yes_ask if yes_ask >= yes_bid else yes_bid,
            volume_24h=normalize_amount(item.get("volume_24h_fp", item.get("volume_24h"))),
            open_interest=normalize_amount(item.get("open_interest", item.get("open_interest_fp"))),
            status=status,
            resolution_time=resolution_time,
            fetched_at=fetched_at,
        )
