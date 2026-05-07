from __future__ import annotations

import ast
import importlib.util
import json
from pathlib import Path

import pytest

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

AGENT_PATH = Path("agents/confirmation_agent.py")
AGENT_MODULE = "agents.confirmation_agent"


def _source() -> str:
    return AGENT_PATH.read_text(encoding="utf-8")


def _ast_names(source: str) -> set[str]:
    """Collect all top-level and nested names defined in the module."""
    tree = ast.parse(source)
    return {node.id for node in ast.walk(tree) if isinstance(node, ast.Name)}


def _import_names(source: str) -> set[str]:
    """Collect all module names that are directly imported."""
    tree = ast.parse(source)
    names: set[str] = set()
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            for alias in node.names:
                names.add(alias.name.split(".")[0])
        elif isinstance(node, ast.ImportFrom):
            if node.module:
                names.add(node.module.split(".")[0])
    return names


def _function_names(source: str) -> set[str]:
    """Collect all function definition names at any nesting level."""
    tree = ast.parse(source)
    return {node.name for node in ast.walk(tree) if isinstance(node, ast.FunctionDef)}


def _write_events(path: Path, events: list[dict]) -> None:
    path.write_text("\n".join(json.dumps(e) for e in events) + "\n", encoding="utf-8")


def _strong_market_signal(signal_id: str = "sig-market-1") -> list[dict]:
    """Return a signal.generated event plus a market.scored event that passes all checks."""
    return [
        {
            "event_type": "signal.generated",
            "payload": {
                "signal_id": signal_id,
                "strength": 0.85,
            },
            "aggregate_key": "polymarket:0xabc123",
            "linkage": {
                "signal_id": signal_id,
                "hypothesis_id": None,
                "correlation_id": signal_id,
                "parent_event_id": None,
            },
        },
        {
            "event_type": "market.scored",
            "aggregate_key": "polymarket:0xabc123",
            "payload": {
                "market_id": "polymarket:0xabc123",
                "score": 0.9,
                "volume_usdc": 50_000.0,
                "hours_to_resolution": 48.0,
            },
            "linkage": {},
        },
    ]


def _run_agent(monkeypatch, store: Path, checkpoint: Path, extra: list[str] | None = None) -> None:
    """Import and run the agent's main() with patched sys.argv."""
    argv = [
        "confirmation_agent.py",
        "--store", str(store),
        "--checkpoint", str(checkpoint),
        "--threshold", "0.6",
        "--full-replay",
    ]
    if extra:
        argv.extend(extra)
    monkeypatch.setattr("sys.argv", argv)

    spec = importlib.util.spec_from_file_location(AGENT_MODULE, AGENT_PATH)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    rc = mod.main()
    assert rc == 0


# ---------------------------------------------------------------------------
# 1. No subprocess import
# ---------------------------------------------------------------------------

def test_no_subprocess_import() -> None:
    """confirmation_agent.py must NOT import subprocess."""
    imports = _import_names(_source())
    assert "subprocess" not in imports, (
        "confirmation_agent.py still imports subprocess — cargo dependency not removed"
    )


# ---------------------------------------------------------------------------
# 2. No confirm_via_cli function
# ---------------------------------------------------------------------------

def test_no_confirm_via_cli_function() -> None:
    """confirm_via_cli must not exist in confirmation_agent.py."""
    functions = _function_names(_source())
    assert "confirm_via_cli" not in functions, (
        "confirm_via_cli is still defined — it must be removed"
    )


# ---------------------------------------------------------------------------
# 3. confirm_via_jsonl is defined and callable
# ---------------------------------------------------------------------------

def test_confirm_via_jsonl_exists() -> None:
    functions = _function_names(_source())
    assert "confirm_via_jsonl" in functions, (
        "confirm_via_jsonl must be defined in confirmation_agent.py"
    )


# ---------------------------------------------------------------------------
# 4. Confirmation writes a signal.confirmed event to the JSONL store
# ---------------------------------------------------------------------------

def test_confirmation_writes_signal_confirmed_event(tmp_path: Path, monkeypatch) -> None:
    store = tmp_path / "events.jsonl"
    checkpoint = tmp_path / "ckpt.json"

    _write_events(store, _strong_market_signal())

    # Patch PYTHONPATH so relative imports work
    import sys
    sys.path.insert(0, str(Path("agents").resolve()))

    monkeypatch.setattr(
        "sys.argv",
        [
            "confirmation_agent.py",
            "--store", str(store),
            "--checkpoint", str(checkpoint),
            "--threshold", "0.6",
            "--full-replay",
        ],
    )

    # Import and run via importlib so we get the modified module
    import importlib
    import agents.confirmation_agent as agent_mod
    importlib.reload(agent_mod)
    rc = agent_mod.main()
    assert rc == 0

    events = [json.loads(line) for line in store.read_text().splitlines() if line.strip()]
    confirmed_events = [e for e in events if e.get("event_type") == "signal.confirmed"]

    assert len(confirmed_events) == 1, f"expected 1 signal.confirmed event, got {len(confirmed_events)}"

    ev = confirmed_events[0]
    assert ev["schema_version"] == "v1"
    assert ev["produced_by"] == "runtime.agent.confirmation"
    assert ev["payload"]["signal_id"] == "sig-market-1"
    assert ev["payload"]["confirmed_by"] == "confirmation-agent-v1"
    assert isinstance(ev["payload"]["confirmation_reasons"], list)
    assert isinstance(ev["payload"]["rejection_reasons"], list)
    assert isinstance(ev["payload"]["confirmation_score"], float)
    assert "signal_id" in ev["linkage"]
    assert "actor" in ev["provenance"]
    assert ev["provenance"]["actor"] == "confirmation-agent-v1"


# ---------------------------------------------------------------------------
# 5. Idempotency — running twice does not duplicate the event
# ---------------------------------------------------------------------------

def test_confirmation_idempotent_no_duplicate(tmp_path: Path, monkeypatch) -> None:
    store = tmp_path / "events.jsonl"
    checkpoint = tmp_path / "ckpt.json"

    _write_events(store, _strong_market_signal())

    import importlib
    import agents.confirmation_agent as agent_mod

    def run() -> None:
        importlib.reload(agent_mod)
        monkeypatch.setattr(
            "sys.argv",
            [
                "confirmation_agent.py",
                "--store", str(store),
                "--checkpoint", str(checkpoint),
                "--threshold", "0.6",
                "--full-replay",
            ],
        )
        rc = agent_mod.main()
        assert rc == 0

    run()
    run()

    events = [json.loads(line) for line in store.read_text().splitlines() if line.strip()]
    confirmed_events = [e for e in events if e.get("event_type") == "signal.confirmed"]

    assert len(confirmed_events) == 1, (
        f"idempotency broken: found {len(confirmed_events)} signal.confirmed events after 2 runs"
    )


# ---------------------------------------------------------------------------
# 6. Idempotency key format matches expected pattern
# ---------------------------------------------------------------------------

def test_idempotency_key_format(tmp_path: Path, monkeypatch) -> None:
    store = tmp_path / "events.jsonl"
    checkpoint = tmp_path / "ckpt.json"

    _write_events(store, _strong_market_signal("sig-key-test"))

    import importlib
    import agents.confirmation_agent as agent_mod
    importlib.reload(agent_mod)
    monkeypatch.setattr(
        "sys.argv",
        [
            "confirmation_agent.py",
            "--store", str(store),
            "--checkpoint", str(checkpoint),
            "--threshold", "0.6",
            "--full-replay",
        ],
    )
    agent_mod.main()

    events = [json.loads(line) for line in store.read_text().splitlines() if line.strip()]
    confirmed_events = [e for e in events if e.get("event_type") == "signal.confirmed"]
    assert confirmed_events, "no signal.confirmed event written"

    key = confirmed_events[0]["idempotency_key"]
    assert key.startswith("signal.confirmed:v1:sig-key-test:"), (
        f"unexpected idempotency_key format: {key}"
    )
    assert "confirmation-agent-v1" in key


# ---------------------------------------------------------------------------
# 7. Weak signal is not confirmed
# ---------------------------------------------------------------------------

def test_signal_without_market_context_not_confirmed(tmp_path: Path, monkeypatch) -> None:
    """A signal with no market.scored context and low strength must not be confirmed.

    The confirmation rule requires at least 2 positive checks.  Without a
    market.scored event, market_score / volume / hours checks all fail, leaving
    zero positive checks — regardless of strength — so no signal.confirmed event
    should be written.
    """
    store = tmp_path / "events.jsonl"
    checkpoint = tmp_path / "ckpt.json"

    # No market.scored event → market_info is None for this signal.
    events = [
        {
            "event_type": "signal.generated",
            "payload": {"signal_id": "sig-nomarket", "strength": 0.3},
            "aggregate_key": "polymarket:0xnomarket",
            "linkage": {
                "signal_id": "sig-nomarket",
                "hypothesis_id": None,
                "correlation_id": None,
                "parent_event_id": None,
            },
        },
    ]
    _write_events(store, events)

    import importlib
    import agents.confirmation_agent as agent_mod
    importlib.reload(agent_mod)
    monkeypatch.setattr(
        "sys.argv",
        [
            "confirmation_agent.py",
            "--store", str(store),
            "--checkpoint", str(checkpoint),
            "--threshold", "0.6",
            "--full-replay",
        ],
    )
    agent_mod.main()

    all_events = [json.loads(line) for line in store.read_text().splitlines() if line.strip()]
    confirmed = [e for e in all_events if e.get("event_type") == "signal.confirmed"]
    assert len(confirmed) == 0, (
        "signal without market context and low strength must not produce a signal.confirmed event"
    )
