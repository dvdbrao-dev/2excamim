from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime, timezone
from typing import Literal, Optional

Price = float
Amount = float


@dataclass
class MarketRecord:
    market_id: str
    source: Literal["kalshi", "polymarket"]
    question: str
    probability: Price
    yes_bid: Price
    yes_ask: Price
    volume_24h: Amount
    open_interest: Amount
    status: Literal["open", "closed", "resolved"]
    resolution_time: Optional[datetime]
    fetched_at: datetime


@dataclass
class TradeRecord:
    trade_id: str
    market_id: str
    source: Literal["kalshi", "polymarket"]
    price: Price
    size: Amount
    side: Literal["yes", "no"]
    timestamp: datetime
    taker: bool


@dataclass
class BlockRecord:
    block_number: int
    timestamp: datetime


@dataclass
class FeatureRecord:
    market_id: str
    timestamp: datetime
    feature_name: str
    value: float
    metadata: dict


@dataclass
class SignalCandidateRecord:
    market_id: str
    timestamp: datetime
    signal_name: str
    strength: float
    direction: Literal["long_yes", "long_no", "short_yes", "short_no"]
    metadata: dict


def utc_now() -> datetime:
    return datetime.now(timezone.utc)


def normalize_probability(value: object, default: float = 0.0) -> float:
    if value is None:
        return default

    if isinstance(value, str):
        raw = value.strip()
        if not raw:
            return default
        if raw.endswith("%"):
            raw = raw[:-1]
        value = raw

    try:
        numeric = float(value)
    except (TypeError, ValueError):
        return default

    if numeric > 1.0:
        numeric /= 100.0
    if numeric < 0.0:
        return 0.0
    if numeric > 1.0:
        return 1.0
    return numeric


def normalize_amount(value: object, default: float = 0.0) -> float:
    if value is None:
        return default
    try:
        return float(value)
    except (TypeError, ValueError):
        return default


def parse_datetime(value: object) -> Optional[datetime]:
    if value in (None, ""):
        return None

    if isinstance(value, datetime):
        if value.tzinfo is None:
            return value.replace(tzinfo=timezone.utc)
        return value.astimezone(timezone.utc)

    if isinstance(value, (int, float)):
        ts = float(value)
        if ts > 10_000_000_000:
            ts /= 1000.0
        return datetime.fromtimestamp(ts, tz=timezone.utc)

    if isinstance(value, str):
        raw = value.strip()
        if not raw:
            return None
        if raw.endswith("Z"):
            raw = raw[:-1] + "+00:00"
        try:
            parsed = datetime.fromisoformat(raw)
        except ValueError:
            try:
                ts = float(raw)
            except ValueError:
                return None
            return parse_datetime(ts)
        if parsed.tzinfo is None:
            return parsed.replace(tzinfo=timezone.utc)
        return parsed.astimezone(timezone.utc)

    return None
