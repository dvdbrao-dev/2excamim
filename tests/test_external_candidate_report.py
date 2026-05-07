from __future__ import annotations

import json
from argparse import Namespace
from pathlib import Path

from agents.external_candidate_report import run


def _write(path: Path, rows: list[dict]) -> None:
    path.write_text("\n".join(json.dumps(r) for r in rows) + "\n", encoding="utf-8")


def _ev(event_type: str, payload: dict, source: str = "test_source") -> dict:
    return {
        "event_type": event_type,
        "event_id": f"{event_type}-1-{payload.get('asset', 'x')}",
        "timestamp": "2026-05-07T18:00:00Z",
        "idempotency_key": f"{event_type}:1:{payload.get('asset', 'x')}",
        "aggregate_key": f"polymarket:{payload.get('asset', 'btc').lower()}:{payload.get('window', '5m')}",
        "provenance": {"agent_id": "t", "source": source, "generated_at": "2026-05-07T18:00:00Z"},
        "payload": payload,
    }


def test_generate_report_contains_required_sections(tmp_path: Path) -> None:
    inp = tmp_path / "events.jsonl"
    report_dir = tmp_path / "reports"

    rows = [
        _ev(
            "candidate_signal.scored",
            {
                "strategy_version": "oracle_lag_v1",
                "asset": "BTC",
                "window": "5m",
                "rejected": False,
                "source": "oracle_lag_signal_candidate",
            },
            source="oracle_lag_signal_candidate",
        ),
        _ev(
            "candidate_signal.scored",
            {
                "strategy_version": "oracle_lag_v1",
                "asset": "ETH",
                "window": "15m",
                "rejected": True,
                "reject_reason": "spread_too_wide",
                "source": "oracle_lag_signal_candidate",
            },
            source="oracle_lag_signal_candidate",
        ),
        _ev(
            "shadow_fill.simulated",
            {
                "strategy_version": "oracle_lag_v1",
                "asset": "BTC",
                "window": "5m",
                "fill_assumption": "conservative_partial",
                "source": "shadow_execution_simulator",
            },
            source="shadow_execution_simulator",
        ),
        _ev(
            "strategy_round.scored",
            {
                "strategy_version": "oracle_lag_v1",
                "asset": "BTC",
                "window": "5m",
                "outcome_known": True,
                "gross_pnl_usdc": 1.2,
                "net_pnl_usdc": 1.0,
                "source": "shadow_execution_simulator",
            },
            source="shadow_execution_simulator",
        ),
        _ev(
            "feed_health.checked",
            {
                "asset": "BTC",
                "window": "5m",
                "ok": False,
                "reason": "stale",
                "source": "research_collector_candidate",
            },
            source="research_collector_candidate",
        ),
        _ev(
            "data_gap.detected",
            {
                "asset": "BTC",
                "window": "5m",
                "gap_type": "missing_oracle",
                "source": "research_collector_candidate",
            },
            source="research_collector_candidate",
        ),
        _ev(
            "candidate_strategy.evaluated",
            {
                "strategy_version": "oracle_lag_v1",
                "status": "candidate",
                "reason": "insufficient_sample",
                "source": "external_candidate_scorecard",
            },
            source="external_candidate_scorecard",
        ),
    ]
    _write(inp, rows)

    out = run(
        Namespace(
            input_jsonl=str(inp),
            report_dir=str(report_dir),
            strategy_version="oracle_lag_v1",
            run_id="test_run",
            output_report="",
        )
    )

    report = Path(out["output_report"])
    assert report.exists()
    content = report.read_text(encoding="utf-8")

    for section in [
        "# External Candidate Report",
        "## Summary",
        "## Feed Health",
        "## Data Gaps",
        "## Per Strategy",
        "## Per Asset/Window",
        "## Per Run",
        "## Next Action",
    ]:
        assert section in content

    assert "spread_too_wide" in content
    assert "conservative_partial" in content
    assert "candidate" in content
