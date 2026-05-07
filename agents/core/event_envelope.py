from __future__ import annotations

import hashlib
import json
import uuid
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

try:
    from core.event_store import append_event_idempotent
except ModuleNotFoundError:  # pragma: no cover - fallback for package-style imports in tests
    from agents.core.event_store import append_event_idempotent

# Stable namespace for deterministic UUID5 generation of external-candidate events.
EVENT_NAMESPACE = uuid.UUID("2d9fdb89-3dc5-4d20-8abd-5ec929852a4d")

REQUIRED_EVENT_FIELDS = (
    "event_type",
    "event_id",
    "timestamp",
    "idempotency_key",
    "aggregate_key",
    "provenance",
    "payload",
)

ALLOWED_EXTERNAL_EVENT_TYPES = {
    "external_repo.audit_recorded",
    "market_slot.discovered",
    "market_snapshot.observed",
    "oracle_lag.observed",
    "candidate_signal.scored",
    "shadow_fill.simulated",
    "strategy_round.scored",
    "candidate_strategy.evaluated",
    "data_gap.detected",
    "feed_health.checked",
}


def utc_now_iso() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def build_idempotency_key(event_type: str, aggregate_key: str, unique_components: list[str]) -> str:
    joined = ":".join([event_type, "v1", aggregate_key, *unique_components])
    digest = hashlib.sha256(joined.encode("utf-8")).hexdigest()[:24]
    return f"{event_type}:v1:{aggregate_key}:{digest}"


def build_aggregate_key(domain: str, identifier: str, timeframe: str | None = None) -> str:
    if timeframe:
        return f"{domain}:{identifier}:{timeframe}"
    return f"{domain}:{identifier}"


def deterministic_event_id(event_type: str, aggregate_key: str, idempotency_key: str) -> str:
    seed = f"{event_type}|{aggregate_key}|{idempotency_key}"
    return str(uuid.uuid5(EVENT_NAMESPACE, seed))


def build_provenance(agent_id: str, source: str, notes: str | None = None) -> dict[str, Any]:
    provenance: dict[str, Any] = {
        "agent_id": agent_id,
        "source": source,
        "generated_at": utc_now_iso(),
    }
    if notes:
        provenance["notes"] = notes
    return provenance


def validate_event(event: dict[str, Any], *, strict_type: bool = False) -> None:
    missing = [field for field in REQUIRED_EVENT_FIELDS if field not in event]
    if missing:
        raise ValueError(f"event missing required fields: {','.join(missing)}")

    for field in ("event_type", "event_id", "timestamp", "idempotency_key", "aggregate_key"):
        value = event.get(field)
        if not isinstance(value, str) or not value.strip():
            raise ValueError(f"event field {field} must be a non-empty string")

    if not isinstance(event.get("provenance"), dict):
        raise ValueError("event field provenance must be an object")
    if not isinstance(event.get("payload"), dict):
        raise ValueError("event field payload must be an object")

    try:
        uuid.UUID(event["event_id"])
    except (ValueError, AttributeError) as error:
        raise ValueError("event_id must be a valid UUID") from error

    try:
        datetime.fromisoformat(event["timestamp"].replace("Z", "+00:00"))
    except ValueError as error:
        raise ValueError("timestamp must be ISO-8601") from error

    if strict_type and event["event_type"] not in ALLOWED_EXTERNAL_EVENT_TYPES:
        raise ValueError(f"event_type not allowed for external candidates: {event['event_type']}")


def build_event(
    *,
    event_type: str,
    aggregate_key: str,
    payload: dict[str, Any],
    provenance: dict[str, Any],
    unique_components: list[str],
    timestamp: str | None = None,
) -> dict[str, Any]:
    ts = timestamp or utc_now_iso()
    idem = build_idempotency_key(event_type, aggregate_key, unique_components)
    event = {
        "event_type": event_type,
        "event_id": deterministic_event_id(event_type, aggregate_key, idem),
        "timestamp": ts,
        "schema_version": "v1",
        "idempotency_key": idem,
        "aggregate_key": aggregate_key,
        "provenance": provenance,
        "payload": payload,
    }
    validate_event(event)
    return event


def append_event_jsonl(store_path: Path, event: dict[str, Any], *, dry_run: bool = False) -> bool:
    validate_event(event)
    if dry_run:
        return True
    return append_event_idempotent(store_path, event)


def encode_jsonl_line(event: dict[str, Any]) -> str:
    validate_event(event)
    return json.dumps(event, separators=(",", ":"))
