from __future__ import annotations

import json
import time
from pathlib import Path

import pytest

from agents.core.event_store import (
    _load_index,
    append_event_idempotent,
    rebuild_index,
)


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _event(key: str) -> dict:
    return {
        "event_type": "test.event",
        "schema_version": "v1",
        "idempotency_key": key,
        "payload": {"key": key},
    }


def _write_raw(path: Path, events: list[dict]) -> None:
    """Write events directly to JSONL, bypassing append_event_idempotent."""
    path.write_text("\n".join(json.dumps(e) for e in events) + "\n", encoding="utf-8")


def _index_path(store: Path) -> Path:
    return store.parent / ".idempotency_index.json"


def _read_index_raw(store: Path) -> dict:
    return json.loads(_index_path(store).read_text(encoding="utf-8"))


# ---------------------------------------------------------------------------
# 1. rebuild_index reads all keys from JSONL
# ---------------------------------------------------------------------------

def test_idempotency_index_rebuilds_from_jsonl(tmp_path: Path) -> None:
    store = tmp_path / "events.jsonl"
    idx = _index_path(store)

    events = [_event(f"key-{i}") for i in range(20)]
    _write_raw(store, events)

    result = rebuild_index(store, idx)

    assert idx.exists(), "rebuild_index must write the index file"
    assert len(result) == 20
    for i in range(20):
        assert f"key-{i}" in result, f"key-{i} missing from rebuilt index"

    raw = _read_index_raw(store)
    assert raw["store_size"] == store.stat().st_size


# ---------------------------------------------------------------------------
# 2. Index is consistent with JSONL after normal writes
# ---------------------------------------------------------------------------

def test_idempotency_index_consistent_with_jsonl(tmp_path: Path) -> None:
    store = tmp_path / "events.jsonl"
    idx = _index_path(store)

    n = 30
    for i in range(n):
        assert append_event_idempotent(store, _event(f"cons-key-{i}")) is True

    # Rebuild fresh from JSONL and compare counts
    rebuilt = rebuild_index(store, idx)
    assert len(rebuilt) == n

    # Every key written must appear in the rebuilt index
    for i in range(n):
        assert f"cons-key-{i}" in rebuilt

    # Recorded store_size must match actual file size
    raw = _read_index_raw(store)
    assert raw["store_size"] == store.stat().st_size, (
        "store_size in index must match actual JSONL file size"
    )


# ---------------------------------------------------------------------------
# 3. Kill switch blocks writes without touching the store
# ---------------------------------------------------------------------------

def test_kill_switch_blocks_writes(tmp_path: Path) -> None:
    store = tmp_path / "events.jsonl"
    kill_switch = tmp_path / ".kill_switch"
    kill_switch.touch()

    result = append_event_idempotent(store, _event("blocked-key"))

    assert result is False
    # Store must not have been created or must remain empty
    assert not store.exists() or store.read_text(encoding="utf-8").strip() == ""


def test_kill_switch_does_not_block_when_absent(tmp_path: Path) -> None:
    store = tmp_path / "events.jsonl"
    # No kill switch file

    result = append_event_idempotent(store, _event("allowed-key"))
    assert result is True


# ---------------------------------------------------------------------------
# 4. Duplicate event returns False and is not written twice
# ---------------------------------------------------------------------------

def test_duplicate_event_returns_false(tmp_path: Path) -> None:
    store = tmp_path / "events.jsonl"
    ev = _event("dup-key")

    first = append_event_idempotent(store, ev)
    second = append_event_idempotent(store, ev)
    third = append_event_idempotent(store, ev)

    assert first is True
    assert second is False
    assert third is False

    lines = [ln for ln in store.read_text(encoding="utf-8").splitlines() if ln.strip()]
    assert len(lines) == 1, f"expected 1 line, got {len(lines)}"


def test_different_keys_both_written(tmp_path: Path) -> None:
    store = tmp_path / "events.jsonl"

    assert append_event_idempotent(store, _event("key-a")) is True
    assert append_event_idempotent(store, _event("key-b")) is True
    assert append_event_idempotent(store, _event("key-a")) is False  # dup

    lines = [ln for ln in store.read_text(encoding="utf-8").splitlines() if ln.strip()]
    assert len(lines) == 2


# ---------------------------------------------------------------------------
# 5. Corrupt index triggers silent rebuild
# ---------------------------------------------------------------------------

def test_index_corruption_rebuilds_cleanly(tmp_path: Path) -> None:
    store = tmp_path / "events.jsonl"
    idx = _index_path(store)

    # Write 5 events via normal path (builds index)
    for i in range(5):
        append_event_idempotent(store, _event(f"pre-key-{i}"))

    assert idx.exists()

    # Corrupt the index with invalid JSON
    idx.write_text("{not valid json!!!", encoding="utf-8")

    # Next append should silently rebuild and succeed
    result = append_event_idempotent(store, _event("post-corruption-key"))
    assert result is True

    # Index must be valid again
    assert idx.exists()
    raw = json.loads(idx.read_text(encoding="utf-8"))
    assert isinstance(raw.get("keys"), dict)
    assert "post-corruption-key" in raw["keys"]
    # Pre-existing keys should also be in the rebuilt index
    for i in range(5):
        assert f"pre-key-{i}" in raw["keys"]


def test_index_wrong_type_triggers_rebuild(tmp_path: Path) -> None:
    """Index with non-dict 'keys' field triggers rebuild."""
    store = tmp_path / "events.jsonl"
    idx = _index_path(store)

    append_event_idempotent(store, _event("base-key"))

    # Write a malformed index (keys is a list, not a dict)
    idx.write_text(
        json.dumps({"store_size": store.stat().st_size, "keys": ["base-key"]}),
        encoding="utf-8",
    )

    # Append should rebuild and still detect the duplicate
    result = append_event_idempotent(store, _event("base-key"))
    assert result is False


# ---------------------------------------------------------------------------
# 6. Store shrinkage detected — index rebuilt automatically
# ---------------------------------------------------------------------------

def test_store_shrinkage_invalidates_index(tmp_path: Path) -> None:
    """If store_size in index > current file size, the index is considered stale."""
    store = tmp_path / "events.jsonl"
    idx = _index_path(store)

    # Write events and build index
    for i in range(10):
        append_event_idempotent(store, _event(f"shrink-key-{i}"))

    raw_before = _read_index_raw(store)
    assert raw_before["store_size"] == store.stat().st_size

    # Simulate store replacement with a smaller file (only 1 event)
    _write_raw(store, [_event("shrink-key-0")])
    assert store.stat().st_size < raw_before["store_size"]

    # _load_index must detect shrinkage and return None
    loaded = _load_index(store, idx)
    assert loaded is None, "stale index should be rejected when store shrank"


# ---------------------------------------------------------------------------
# 7. Performance: 10 000-key index lookup must be fast
# ---------------------------------------------------------------------------

def test_idempotency_index_performance(tmp_path: Path) -> None:
    """Checking a known-duplicate key via the index must be O(1), not O(n)."""
    store = tmp_path / "events.jsonl"
    idx = _index_path(store)

    n = 10_000
    events = [_event(f"perf-key-{i}") for i in range(n)]
    _write_raw(store, events)

    # Warm up the index
    rebuild_index(store, idx)
    assert idx.exists()

    # The last key is known to exist — must return False quickly via index
    existing = _event("perf-key-9999")

    start = time.perf_counter()
    result = append_event_idempotent(store, existing)
    elapsed = time.perf_counter() - start

    assert result is False, "known duplicate must return False"
    assert elapsed < 2.0, (
        f"index lookup for 10 000-key store took {elapsed:.3f}s — expected < 2.0s. "
        "Index cache may not be working."
    )
