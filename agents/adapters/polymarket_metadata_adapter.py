from __future__ import annotations

import re
from dataclasses import dataclass
from datetime import datetime, timezone
from typing import Any

from .http_client import HttpJsonClient


@dataclass(frozen=True)
class MetadataResult:
    market_id: str | None
    condition_id: str | None
    question: str | None
    active: bool | None
    closed: bool | None
    resolved: bool | None
    outcome_tokens: list[dict[str, str]]
    end_date: str | None
    resolution_date: str | None
    raw_source_summary: str
    match_confidence: float
    match_reason: str
    latency_ms: int
    error: str | None


class PolymarketMetadataAdapter:
    VERSION = "polymarket_metadata_adapter_v2"

    def __init__(self, timeout_sec: float = 5.0, max_retries: int = 2, client: HttpJsonClient | None = None) -> None:
        self.client = client or HttpJsonClient(timeout_sec=timeout_sec, max_retries=max_retries)

    def observe(self, candidate_slug: str) -> MetadataResult:
        exact = self.client.get_json(
            "https://gamma-api.polymarket.com/markets",
            params={"slug": candidate_slug, "limit": "5"},
        )
        if exact.ok and isinstance(exact.data, list):
            rows = [row for row in exact.data if isinstance(row, dict)]
            if len(rows) == 1:
                return _to_result(rows[0], exact.latency_ms, "gamma.markets.slug", 1.0, "exact_slug")
            if len(rows) > 1:
                return _error_result(
                    "gamma.markets.slug",
                    exact.latency_ms,
                    "metadata_ambiguous_slug",
                    confidence=0.0,
                    match_reason="ambiguous_slug",
                )
        if not exact.ok:
            return _error_result("gamma.markets.slug", exact.latency_ms, _normalize_error(exact.error), 0.0, "exact_slug_failed")

        parsed = _parse_candidate_slug(candidate_slug)
        if parsed is None:
            return _error_result("gamma.markets.slug", exact.latency_ms, "metadata_not_found", 0.0, "slug_not_parseable")

        fallback = self.client.get_json(
            "https://gamma-api.polymarket.com/markets",
            params={"active": "true", "closed": "false", "limit": "200"},
        )
        latency_total = exact.latency_ms + fallback.latency_ms
        if not fallback.ok or not isinstance(fallback.data, list):
            return _error_result("gamma.markets.search", latency_total, _normalize_error(fallback.error), 0.0, "fallback_failed")

        candidates: list[tuple[float, str, dict[str, Any]]] = []
        for row in fallback.data:
            if not isinstance(row, dict):
                continue
            score, reason = _score_market_match(row, parsed)
            if score <= 0:
                continue
            candidates.append((score, reason, row))

        if not candidates:
            return _error_result("gamma.markets.search", latency_total, "metadata_not_found", 0.0, "fallback_no_match")

        candidates.sort(key=lambda item: item[0], reverse=True)
        best_score, reason, best_row = candidates[0]
        if best_score < 0.7:
            return _error_result("gamma.markets.search", latency_total, "metadata_low_confidence", best_score, reason)
        if len(candidates) > 1 and abs(candidates[0][0] - candidates[1][0]) < 0.1:
            return _error_result("gamma.markets.search", latency_total, "metadata_ambiguous_fallback", best_score, "ambiguous_fallback")

        return _to_result(best_row, latency_total, "gamma.markets.search", best_score, reason)


def _to_result(
    row: dict[str, Any], latency_ms: int, raw_source_summary: str, match_confidence: float, match_reason: str
) -> MetadataResult:
    outcomes = _extract_outcome_tokens(row)
    return MetadataResult(
        market_id=_str_or_none(row.get("id")),
        condition_id=_str_or_none(row.get("conditionId")),
        question=_str_or_none(row.get("question")) or _str_or_none(row.get("title")),
        active=_bool_or_none(row.get("active")),
        closed=_bool_or_none(row.get("closed")),
        resolved=_bool_or_none(row.get("resolved")) or _bool_or_none(row.get("archived")),
        outcome_tokens=outcomes,
        end_date=_str_or_none(row.get("endDate")) or _str_or_none(row.get("end_date_iso")),
        resolution_date=_str_or_none(row.get("resolutionDate")) or _str_or_none(row.get("resolvedAt")),
        raw_source_summary=raw_source_summary,
        match_confidence=round(max(0.0, min(1.0, match_confidence)), 4),
        match_reason=match_reason,
        latency_ms=latency_ms,
        error=None,
    )


def _error_result(
    raw_source_summary: str, latency_ms: int, error: str, confidence: float, match_reason: str
) -> MetadataResult:
    return MetadataResult(
        market_id=None,
        condition_id=None,
        question=None,
        active=None,
        closed=None,
        resolved=None,
        outcome_tokens=[],
        end_date=None,
        resolution_date=None,
        raw_source_summary=raw_source_summary,
        match_confidence=round(max(0.0, min(1.0, confidence)), 4),
        match_reason=match_reason,
        latency_ms=latency_ms,
        error=error,
    )


def _extract_outcome_tokens(row: dict[str, Any]) -> list[dict[str, str]]:
    labels = _parse_string_list(row.get("outcomes"))
    token_ids = _parse_string_list(row.get("clobTokenIds"))
    out: list[dict[str, str]] = []
    for idx, token_id in enumerate(token_ids):
        outcome = labels[idx] if idx < len(labels) else f"outcome_{idx}"
        out.append({"outcome": outcome, "token_id": token_id})
    return out


def _parse_string_list(value: Any) -> list[str]:
    if isinstance(value, list):
        return [str(v) for v in value if str(v)]
    if isinstance(value, str):
        text = value.strip()
        if text.startswith("[") and text.endswith("]"):
            inner = text[1:-1].strip()
            if not inner:
                return []
            parts = [p.strip().strip('"').strip("'") for p in inner.split(",")]
            return [p for p in parts if p]
    return []


def _parse_candidate_slug(slug: str) -> dict[str, Any] | None:
    lower = slug.lower()
    match = re.match(r"^(btc|eth|sol)-up-or-down-([a-z]{3})-(\d{2})-(\d{4})-utc-(5m|15m)$", lower)
    if not match:
        return None
    asset, mon, day, hhmm, window = match.groups()
    month_map = {
        "jan": 1,
        "feb": 2,
        "mar": 3,
        "apr": 4,
        "may": 5,
        "jun": 6,
        "jul": 7,
        "aug": 8,
        "sep": 9,
        "oct": 10,
        "nov": 11,
        "dec": 12,
    }
    month = month_map.get(mon)
    if month is None:
        return None
    hour = int(hhmm[:2])
    minute = int(hhmm[2:])
    slot_dt = datetime(datetime.now(timezone.utc).year, month, int(day), hour, minute, tzinfo=timezone.utc)
    return {"asset": asset.upper(), "window": window, "slot_dt": slot_dt}


def _score_market_match(row: dict[str, Any], parsed: dict[str, Any]) -> tuple[float, str]:
    score = 0.0
    reasons: list[str] = []
    question = str(row.get("question") or "").lower()

    if parsed["asset"].lower() in question:
        score += 0.45
        reasons.append("asset_in_question")
    if parsed["window"] in question:
        score += 0.25
        reasons.append("window_in_question")

    market_time = _extract_market_time(row)
    if market_time is not None:
        delta = abs((market_time - parsed["slot_dt"]).total_seconds())
        if delta <= 300:
            score += 0.3
            reasons.append("slot_time_close")
        elif delta <= 900:
            score += 0.12
            reasons.append("slot_time_near")

    return score, ",".join(reasons) if reasons else "no_reason"


def _extract_market_time(row: dict[str, Any]) -> datetime | None:
    for key in ("endDate", "startDate", "createdAt"):
        val = row.get(key)
        if isinstance(val, str):
            try:
                return datetime.fromisoformat(val.replace("Z", "+00:00")).astimezone(timezone.utc)
            except ValueError:
                continue
    return None


def _str_or_none(value: Any) -> str | None:
    return value if isinstance(value, str) and value else None


def _bool_or_none(value: Any) -> bool | None:
    return value if isinstance(value, bool) else None


def _normalize_error(error: str | None) -> str:
    if not error:
        return "unknown_error"
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
