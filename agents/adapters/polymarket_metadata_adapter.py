from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from .http_client import HttpJsonClient


@dataclass(frozen=True)
class MetadataResult:
    market_id: str | None
    condition_id: str | None
    token_ids: list[str] | None
    title: str | None
    active: bool | None
    resolved: bool | None
    raw_source_summary: str
    latency_ms: int
    error: str | None


class PolymarketMetadataAdapter:
    VERSION = "polymarket_metadata_adapter_v1"

    def __init__(self, timeout_sec: float = 5.0, max_retries: int = 2, client: HttpJsonClient | None = None) -> None:
        self.client = client or HttpJsonClient(timeout_sec=timeout_sec, max_retries=max_retries)

    def observe(self, candidate_slug: str) -> MetadataResult:
        # Public Gamma API endpoint; read-only, unauthenticated.
        result = self.client.get_json(
            "https://gamma-api.polymarket.com/markets",
            params={"slug": candidate_slug, "limit": "1"},
        )
        if not result.ok:
            return MetadataResult(None, None, None, None, None, None, "gamma_lookup_failed", result.latency_ms, result.error)
        if not isinstance(result.data, list) or not result.data:
            return MetadataResult(None, None, None, None, None, None, "gamma_lookup_empty", result.latency_ms, "metadata_not_found")

        row = result.data[0] if isinstance(result.data[0], dict) else {}
        token_ids = row.get("clobTokenIds")
        if not isinstance(token_ids, list):
            token_ids = None
        else:
            token_ids = [str(x) for x in token_ids]

        return MetadataResult(
            market_id=_str_or_none(row.get("id")),
            condition_id=_str_or_none(row.get("conditionId")),
            token_ids=token_ids,
            title=_str_or_none(row.get("question")),
            active=_bool_or_none(row.get("active")),
            resolved=_bool_or_none(row.get("closed")),
            raw_source_summary="gamma.markets.slug",
            latency_ms=result.latency_ms,
            error=None,
        )


def _str_or_none(value: Any) -> str | None:
    return value if isinstance(value, str) and value else None


def _bool_or_none(value: Any) -> bool | None:
    return value if isinstance(value, bool) else None
