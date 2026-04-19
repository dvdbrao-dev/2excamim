from __future__ import annotations

import json
import math
import hashlib
from contextlib import contextmanager
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterator

import fcntl


def parse_timestamp(value: Any) -> datetime | None:
    if isinstance(value, str):
        trimmed = value.strip()
        if not trimmed:
            return None
        try:
            return datetime.fromisoformat(trimmed.replace("Z", "+00:00")).astimezone(timezone.utc)
        except ValueError:
            return None
    if isinstance(value, (int, float)) and math.isfinite(float(value)):
        return datetime.fromtimestamp(float(value), tz=timezone.utc)
    return None


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    if not path.exists():
        return []

    records: list[dict[str, Any]] = []
    for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        stripped = line.strip()
        if not stripped:
            continue
        try:
            payload = json.loads(stripped)
        except json.JSONDecodeError as error:
            raise SystemExit(f"invalid JSONL in {path} at line {line_number}: {error}") from error
        if isinstance(payload, dict):
            records.append(payload)
    return records


def load_checkpoint(checkpoint_path: Path) -> dict[str, Any]:
    if not checkpoint_path.exists():
        return {}
    try:
        payload = json.loads(checkpoint_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return {}
    if isinstance(payload, dict):
        return payload
    return {}


def save_checkpoint(checkpoint_path: Path, payload: dict[str, Any]) -> None:
    checkpoint_path.parent.mkdir(parents=True, exist_ok=True)
    checkpoint_path.write_text(
        json.dumps(payload, separators=(",", ":"), sort_keys=True),
        encoding="utf-8",
    )


def read_jsonl_since(path: Path, offset: int) -> tuple[list[dict[str, Any]], int]:
    if not path.exists():
        return [], 0

    file_size = path.stat().st_size
    safe_offset = max(0, int(offset))
    if safe_offset > file_size:
        safe_offset = 0

    records: list[dict[str, Any]] = []
    with path.open("r", encoding="utf-8") as handle:
        handle.seek(safe_offset)
        line_number = 0
        for raw_line in handle:
            line_number += 1
            stripped = raw_line.strip()
            if not stripped:
                continue
            try:
                payload = json.loads(stripped)
            except json.JSONDecodeError as error:
                raise SystemExit(
                    f"invalid JSONL in {path} after byte offset {safe_offset} "
                    f"at relative line {line_number}: {error}"
                ) from error
            if isinstance(payload, dict):
                records.append(payload)
        next_offset = handle.tell()
    return records, next_offset


def default_checkpoint_path(store_path: Path, agent_id: str) -> Path:
    key = str(store_path.resolve())
    digest = hashlib.sha1(key.encode("utf-8")).hexdigest()[:12]
    return store_path.parent / ".checkpoints" / f"{agent_id}-{digest}.json"


@contextmanager
def _locked_file(path: Path, mode: str) -> Iterator[Any]:
    handle = path.open(mode, encoding="utf-8")
    try:
        fcntl.flock(handle.fileno(), fcntl.LOCK_EX)
        yield handle
    finally:
        fcntl.flock(handle.fileno(), fcntl.LOCK_UN)
        handle.close()


def _validate_event_before_append(event: dict[str, Any]) -> None:
    required = ("event_type", "schema_version", "idempotency_key", "payload")
    missing = [key for key in required if key not in event]
    if missing:
        raise SystemExit(f"event missing required keys before append: {','.join(missing)}")
    if not isinstance(event.get("payload"), dict):
        raise SystemExit("event payload must be an object before append")
    key = event.get("idempotency_key")
    if not isinstance(key, str) or not key.strip():
        raise SystemExit("event idempotency_key must be a non-empty string before append")


def append_event_idempotent(store_path: Path, event: dict[str, Any]) -> bool:
    _validate_event_before_append(event)
    idempotency_key = event["idempotency_key"]
    store_path.parent.mkdir(parents=True, exist_ok=True)

    with _locked_file(store_path, "a+") as handle:
        handle.seek(0)
        for line in handle:
            stripped = line.strip()
            if not stripped:
                continue
            try:
                payload = json.loads(stripped)
            except json.JSONDecodeError:
                continue
            if isinstance(payload, dict) and payload.get("idempotency_key") == idempotency_key:
                return False
        handle.write(json.dumps(event, separators=(",", ":")) + "\n")
        handle.flush()
    return True
