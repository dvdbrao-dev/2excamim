from __future__ import annotations

import json
import math
import hashlib
from contextlib import contextmanager
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterator

import fcntl


# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

_KILL_SWITCH_NAME = ".kill_switch"
_INDEX_NAME = ".idempotency_index.json"


# ---------------------------------------------------------------------------
# Timestamp helpers
# ---------------------------------------------------------------------------

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


# ---------------------------------------------------------------------------
# JSONL reader
# ---------------------------------------------------------------------------

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


# ---------------------------------------------------------------------------
# Checkpoint helpers
# ---------------------------------------------------------------------------

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


# ---------------------------------------------------------------------------
# Idempotency index — cache of known idempotency_keys
# ---------------------------------------------------------------------------

def _index_path_for(store_path: Path) -> Path:
    return store_path.parent / _INDEX_NAME


def _save_index(index_path: Path, keys: dict[str, bool], store_size: int) -> None:
    """Persist the index. Failures are silently swallowed — JSONL is source of truth."""
    try:
        index_path.parent.mkdir(parents=True, exist_ok=True)
        index_path.write_text(
            json.dumps({"store_size": store_size, "keys": keys}, separators=(",", ":")),
            encoding="utf-8",
        )
    except OSError:
        pass


def _load_index_raw(index_path: Path) -> dict[str, bool] | None:
    """Load index file, returning None on any parse or I/O error."""
    if not index_path.exists():
        return None
    try:
        raw = json.loads(index_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return None
    if not isinstance(raw, dict):
        return None
    keys = raw.get("keys")
    if not isinstance(keys, dict):
        return None
    return {k: True for k in keys if isinstance(k, str)}


def _load_index(store_path: Path, index_path: Path) -> dict[str, bool] | None:
    """Load index, returning None when rebuild is required.

    Rebuild is triggered when:
    - index file is missing
    - index file is corrupt / unparseable
    - recorded store_size > current store file size (truncation detected)
    """
    if not index_path.exists():
        return None
    try:
        raw = json.loads(index_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return None
    if not isinstance(raw, dict):
        return None
    keys = raw.get("keys")
    if not isinstance(keys, dict):
        return None
    recorded_size = raw.get("store_size", 0)
    if not isinstance(recorded_size, (int, float)):
        return None
    current_size = store_path.stat().st_size if store_path.exists() else 0
    if current_size < int(recorded_size):
        # Store shrank: index may reference events no longer present.
        return None
    return {k: True for k in keys if isinstance(k, str)}


def rebuild_index(store_path: Path, index_path: Path) -> dict[str, bool]:
    """Rebuild the idempotency index from scratch by scanning the JSONL store.

    The JSONL store is the authoritative source of truth. The resulting index
    is saved to *index_path* and also returned for immediate use.
    """
    keys: dict[str, bool] = {}
    if store_path.exists():
        try:
            content = store_path.read_text(encoding="utf-8")
        except OSError:
            content = ""
        for line in content.splitlines():
            stripped = line.strip()
            if not stripped:
                continue
            try:
                payload = json.loads(stripped)
            except json.JSONDecodeError:
                continue
            if isinstance(payload, dict):
                key = payload.get("idempotency_key")
                if isinstance(key, str) and key.strip():
                    keys[key] = True
    store_size = store_path.stat().st_size if store_path.exists() else 0
    _save_index(index_path, keys, store_size)
    return keys


# ---------------------------------------------------------------------------
# Lock helper
# ---------------------------------------------------------------------------

@contextmanager
def _locked_file(path: Path, mode: str) -> Iterator[Any]:
    handle = path.open(mode, encoding="utf-8")
    try:
        fcntl.flock(handle.fileno(), fcntl.LOCK_EX)
        yield handle
    finally:
        fcntl.flock(handle.fileno(), fcntl.LOCK_UN)
        handle.close()


# ---------------------------------------------------------------------------
# Event validation
# ---------------------------------------------------------------------------

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


# ---------------------------------------------------------------------------
# Core append — idempotent write with index cache
# ---------------------------------------------------------------------------

def append_event_idempotent(store_path: Path, event: dict[str, Any]) -> bool:
    """Append *event* to the JSONL store if its idempotency_key is not already present.

    Returns True when the event was written, False when it was skipped.

    Kill switch:
        If ``{store_path.parent}/.kill_switch`` exists the write is blocked and
        False is returned without touching the store.

    Index cache:
        An index of known idempotency_keys is maintained at
        ``{store_path.parent}/.idempotency_index.json``.  A cache hit skips the
        O(n) JSONL scan.  A cache miss falls through to the full scan (defence
        in depth) under an exclusive file lock.
    """
    _validate_event_before_append(event)
    idempotency_key = event["idempotency_key"]

    # 1. Kill switch — block all writes when active.
    kill_switch = store_path.parent / _KILL_SWITCH_NAME
    if kill_switch.exists():
        return False

    store_path.parent.mkdir(parents=True, exist_ok=True)
    index_path = _index_path_for(store_path)

    # 2. Index fast-path (O(1) for known duplicates).
    index = _load_index(store_path, index_path)
    if index is None:
        index = rebuild_index(store_path, index_path)
    if idempotency_key in index:
        return False

    # 3. Exclusive lock → defence-in-depth JSONL scan → write → update index.
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
                # Found in JSONL but missing from index — sync index and exit.
                index[idempotency_key] = True
                store_size = store_path.stat().st_size if store_path.exists() else 0
                _save_index(index_path, index, store_size)
                return False

        handle.write(json.dumps(event, separators=(",", ":")) + "\n")
        handle.flush()

        index[idempotency_key] = True
        store_size = handle.tell()
        _save_index(index_path, index, store_size)

    return True
