from __future__ import annotations

import ast
import json
import urllib.error
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest

ROOT = Path(__file__).resolve().parents[1]

from agents.openfang_bridge import build_state, post_state, run, write_state

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

FORBIDDEN_MODULES = {
    "agents.sizing_agent",
    "agents.live_gateway",
    "agents.confirmation_agent",
    "agents.exit_agent",
    "agents.veto_agent",
}

REQUIRED_TOP_LEVEL_KEYS = {
    "schema_version", "system_id", "timestamp", "mode",
    "kill_switch_active", "strategies", "alerts",
}

REQUIRED_STRATEGY_KEYS = {"id", "status", "metrics", "last_evaluated_at"}


class _MockResponse:
    def __init__(self, status: int = 200):
        self.status = status

    def __enter__(self):
        return self

    def __exit__(self, *_):
        return False


def _write(path: Path, obj: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(obj), encoding="utf-8")


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------


def test_bridge_imports_are_safe() -> None:
    source = (ROOT / "agents" / "openfang_bridge.py").read_text(encoding="utf-8")
    tree = ast.parse(source)

    imported: set[str] = set()
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            for alias in node.names:
                imported.add(alias.name)
        elif isinstance(node, ast.ImportFrom):
            if node.module:
                imported.add(node.module)
                for alias in node.names:
                    imported.add(f"{node.module}.{alias.name}")

    violations = imported & FORBIDDEN_MODULES
    assert not violations, f"Forbidden imports detected: {violations}"


def test_bridge_outputs_match_schema(tmp_path: Path) -> None:
    _write(
        tmp_path / "registry.json",
        {
            "strategies": [
                {
                    "strategy_id": "strat_a",
                    "status": "shadow",
                    "last_evaluated_at": "2026-01-01T00:00:00Z",
                }
            ]
        },
    )
    _write(
        tmp_path / "scorecard.json",
        {
            "strategies": [
                {
                    "strategy_id": "strat_a",
                    "signals_generated": 50,
                    "confidence": 0.72,
                    "profit_factor": 1.3,
                }
            ]
        },
    )

    state = build_state(tmp_path / "registry.json", tmp_path / "scorecard.json", tmp_path / ".ks")

    # Top-level schema keys
    assert REQUIRED_TOP_LEVEL_KEYS <= state.keys()
    assert state["schema_version"] == 1
    assert state["system_id"] == "2excamim"
    assert state["mode"] == "paper"
    assert isinstance(state["kill_switch_active"], bool)
    assert isinstance(state["strategies"], list)
    assert isinstance(state["alerts"], list)

    # Strategy shape
    assert len(state["strategies"]) == 1
    s = state["strategies"][0]
    assert REQUIRED_STRATEGY_KEYS <= s.keys()
    assert s["id"] == "strat_a"
    assert s["status"] == "shadow"
    assert isinstance(s["metrics"], dict)
    assert s["metrics"]["signals_generated"] == 50

    # No kill-switch alert (file absent)
    assert state["kill_switch_active"] is False
    assert not any("kill" in a["message"].lower() for a in state["alerts"])


def test_bridge_handles_missing_runtime_files_gracefully(tmp_path: Path) -> None:
    state = build_state(
        tmp_path / "no_registry.json",
        tmp_path / "no_scorecard.json",
        tmp_path / ".kill_switch",
    )

    assert state["schema_version"] == 1
    assert state["strategies"] == []
    assert state["alerts"] == []
    assert state["kill_switch_active"] is False


def test_bridge_does_not_write_events_jsonl(tmp_path: Path) -> None:
    output = tmp_path / "openfang_state.json"
    rc = run(
        registry_path=tmp_path / "r.json",
        scorecard_path=tmp_path / "s.json",
        kill_switch_path=tmp_path / "k",
        output_path=output,
        ingest_url=None,
    )

    assert rc == 0
    assert output.exists()
    # Bridge must never write events.jsonl or any execution artefact
    assert not (tmp_path / "events.jsonl").exists()
    assert not (tmp_path / "var" / "events.jsonl").exists()
    assert not (tmp_path / ".idempotency_index.json").exists()


def test_bridge_respects_kill_switch(tmp_path: Path) -> None:
    kill_switch = tmp_path / ".kill_switch"
    kill_switch.touch()

    state = build_state(
        tmp_path / "r.json",
        tmp_path / "s.json",
        kill_switch,
    )

    assert state["kill_switch_active"] is True
    critical = [a for a in state["alerts"] if a["level"] == "critical"]
    assert any("kill" in a["message"].lower() for a in critical)

    # Without kill switch
    state_off = build_state(tmp_path / "r.json", tmp_path / "s.json", tmp_path / "absent")
    assert state_off["kill_switch_active"] is False


def test_bridge_post_retries_on_failure() -> None:
    call_count = [0]

    def _mock_urlopen(req, timeout=None):
        call_count[0] += 1
        if call_count[0] < 2:
            raise urllib.error.URLError("connection refused")
        return _MockResponse(200)

    state = {"schema_version": 1, "system_id": "2excamim"}
    with patch("urllib.request.urlopen", side_effect=_mock_urlopen):
        result = post_state(state, "http://localhost/ingest", timeout=1, max_attempts=2)

    assert call_count[0] == 2
    assert result is True


def test_bridge_post_skipped_when_url_unset(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("OPENFANG_INGEST_URL", raising=False)
    output = tmp_path / "state.json"

    with patch("urllib.request.urlopen") as mock_urlopen:
        rc = run(
            registry_path=tmp_path / "r.json",
            scorecard_path=tmp_path / "s.json",
            kill_switch_path=tmp_path / "k",
            output_path=output,
            ingest_url=None,
        )

    assert rc == 0
    assert output.exists()
    mock_urlopen.assert_not_called()


def test_pipeline_runs_bridge_before_cargo() -> None:
    """Regression: openfang_bridge must appear before 'cargo run' in run_pipeline.sh.

    If 'cargo run' times out or fails, the pipeline aborts (set -euo pipefail).
    The bridge must therefore be positioned before that hard dependency.
    """
    script = (ROOT / "scripts" / "run_pipeline.sh").read_text(encoding="utf-8")
    lines = script.splitlines()

    bridge_line = next(
        (i for i, l in enumerate(lines) if "openfang_bridge.py" in l and not l.strip().startswith("#")),
        None,
    )
    cargo_line = next(
        (i for i, l in enumerate(lines) if "cargo run" in l and not l.strip().startswith("#")),
        None,
    )

    assert bridge_line is not None, "openfang_bridge.py call not found in run_pipeline.sh"
    assert cargo_line is not None, "'cargo run' not found in run_pipeline.sh"
    assert bridge_line < cargo_line, (
        f"openfang_bridge (line {bridge_line + 1}) must come before "
        f"'cargo run' (line {cargo_line + 1}) — see fix: run OpenFang bridge before blocking pipeline phase"
    )
