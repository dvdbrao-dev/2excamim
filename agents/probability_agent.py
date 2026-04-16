#!/usr/bin/env python3
"""Probability Agent v1.

Reads confirmed signals from the append-only JSONL store, evaluates each market
with the OpenAI chat completions API, and persists either a follow-up
`signal.confirmed` event or a `veto.raised` event when the blended probability
falls outside the allowed band.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import subprocess
import socket
import sys
import uuid
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


env_path = Path(__file__).resolve().parents[1] / ".env"
if env_path.exists():
    for line in env_path.read_text().splitlines():
        line = line.strip()
        if line and not line.startswith("#") and "=" in line:
            key, _, val = line.partition("=")
            os.environ.setdefault(key.strip(), val.strip().strip('"'))


AGENT_ID = "probability-agent-v1"
PRODUCED_BY = "runtime.agent.probability"
MODEL_NAME = "gpt-4o-mini"
OPENAI_ENDPOINT = "https://api.openai.com/v1/chat/completions"
DEFAULT_STORE = Path("./var/events.jsonl")
DEFAULT_WATCH_DIR = Path("./var/market-watch")
DEFAULT_MAX_SIGNALS = 50
ALPHA = 0.6
MIN_FINAL_PROBABILITY = 0.10
MAX_FINAL_PROBABILITY = 0.90
INPUT_COST_PER_1M_TOKENS = 0.15
OUTPUT_COST_PER_1M_TOKENS = 0.60
API_TIMEOUT_SECONDS = 60


@dataclass
class SignalCandidate:
    signal_id: str
    market_id: str
    aggregate_key: str | None
    title: str
    midpoint: float
    generated_event_id: str | None
    confirmation_event_id: str | None
    parent_event_id: str | None
    generated_at: datetime | None
    confirmation_at: datetime | None
    confirmation_score: float | None
    confirmed_by: str | None
    hypothesis_id: str | None
    correlation_id: str | None


@dataclass
class SnapshotCandidate:
    market_id: str
    title: str
    observed_at: datetime
    midpoint: float


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Estimate market probabilities with OpenAI.")
    parser.add_argument("--store", default=str(DEFAULT_STORE), help="Path to the JSONL store.")
    parser.add_argument(
        "--watch-dir",
        default=str(DEFAULT_WATCH_DIR),
        help="Path to the market-watch state directory.",
    )
    parser.add_argument(
        "--max-signals",
        type=int,
        default=DEFAULT_MAX_SIGNALS,
        help="Maximum number of signals to evaluate in a single run.",
    )
    return parser.parse_args()


def utc_now_rfc3339() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def execution_run_id() -> str:
    timestamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S%fZ")
    return f"{AGENT_ID}-{timestamp}"


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    if not path.exists():
        return []

    records: list[dict[str, Any]] = []
    for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        stripped = line.strip()
        if not stripped:
            continue
        try:
            records.append(json.loads(stripped))
        except json.JSONDecodeError as error:
            raise SystemExit(f"invalid JSONL in {path} at line {line_number}: {error}") from error
    return records


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


def normalized_market_id(value: Any) -> str | None:
    if not isinstance(value, str):
        return None
    trimmed = value.strip()
    if not trimmed:
        return None
    if ":" in trimmed:
        return trimmed.split(":", 1)[1]
    return trimmed


def market_midpoint(snapshot: dict[str, Any]) -> float | None:
    best_bid = snapshot.get("best_bid")
    best_ask = snapshot.get("best_ask")
    last_price = snapshot.get("last_price")

    if isinstance(best_bid, (int, float)) and isinstance(best_ask, (int, float)):
        if math.isfinite(float(best_bid)) and math.isfinite(float(best_ask)):
            return (float(best_bid) + float(best_ask)) / 2.0
    if isinstance(last_price, (int, float)) and math.isfinite(float(last_price)):
        return float(last_price)
    if isinstance(best_bid, (int, float)) and math.isfinite(float(best_bid)):
        return float(best_bid)
    if isinstance(best_ask, (int, float)) and math.isfinite(float(best_ask)):
        return float(best_ask)
    return None


def latest_snapshots(watch_dir: Path) -> dict[str, SnapshotCandidate]:
    snapshots_path = watch_dir / "snapshots.jsonl"
    latest: dict[str, SnapshotCandidate] = {}

    for snapshot in load_jsonl(snapshots_path):
        market_id = snapshot.get("market_id")
        observed_at = parse_timestamp(snapshot.get("observed_at"))
        midpoint = market_midpoint(snapshot)
        title = snapshot.get("title")
        if (
            not isinstance(market_id, str)
            or not market_id.strip()
            or observed_at is None
            or midpoint is None
            or not isinstance(title, str)
            or not title.strip()
        ):
            continue

        candidate = SnapshotCandidate(
            market_id=market_id,
            title=title.strip(),
            observed_at=observed_at,
            midpoint=midpoint,
        )
        current = latest.get(market_id)
        if current is None or candidate.observed_at > current.observed_at:
            latest[market_id] = candidate

    return latest


def collect_candidates(
    events: list[dict[str, Any]],
) -> tuple[dict[str, SignalCandidate], set[str], set[str], set[str]]:
    contexts: dict[str, SignalCandidate] = {}
    confirmed_signal_ids: set[str] = set()
    self_processed_signal_ids: set[str] = set()
    vetoed_signal_ids: set[str] = set()

    for event in events:
        event_type = event.get("event_type")
        payload = event.get("payload") or {}
        linkage = event.get("linkage") or {}

        if event_type == "signal.generated":
            signal_id = payload.get("signal_id")
            if not isinstance(signal_id, str) or not signal_id.strip():
                continue

            market_id = normalized_market_id(event.get("aggregate_key")) or normalized_market_id(
                payload.get("market_id")
            ) or normalized_market_id(payload.get("instrument"))
            if market_id is None:
                continue

            context = contexts.get(signal_id)
            if context is None:
                context = SignalCandidate(
                    signal_id=signal_id,
                    market_id=market_id,
                    aggregate_key=event.get("aggregate_key"),
                    title="",
                    midpoint=0.0,
                    generated_event_id=event.get("event_id"),
                    confirmation_event_id=None,
                    parent_event_id=linkage.get("parent_event_id"),
                    generated_at=parse_timestamp(event.get("occurred_at")),
                    confirmation_at=None,
                    confirmation_score=None,
                    confirmed_by=None,
                    hypothesis_id=linkage.get("hypothesis_id"),
                    correlation_id=linkage.get("correlation_id"),
                )
                contexts[signal_id] = context

            context.market_id = context.market_id or market_id
            context.aggregate_key = context.aggregate_key or event.get("aggregate_key")
            context.generated_event_id = context.generated_event_id or event.get("event_id")
            context.parent_event_id = context.parent_event_id or linkage.get("parent_event_id")
            context.generated_at = context.generated_at or parse_timestamp(event.get("occurred_at"))
            context.hypothesis_id = context.hypothesis_id or linkage.get("hypothesis_id")
            context.correlation_id = context.correlation_id or linkage.get("correlation_id")

        elif event_type == "signal.confirmed":
            signal_id = payload.get("signal_id")
            confirmed_by = payload.get("confirmed_by")
            confirmation_score = payload.get("confirmation_score")
            if not isinstance(signal_id, str) or not signal_id.strip():
                continue

            confirmed_signal_ids.add(signal_id)
            context = contexts.get(signal_id)
            if context is None:
                context = SignalCandidate(
                    signal_id=signal_id,
                    market_id=normalized_market_id(event.get("aggregate_key")) or "",
                    aggregate_key=event.get("aggregate_key"),
                    title="",
                    midpoint=0.0,
                    generated_event_id=None,
                    confirmation_event_id=event.get("event_id"),
                    parent_event_id=linkage.get("parent_event_id"),
                    generated_at=None,
                    confirmation_at=parse_timestamp(event.get("occurred_at")),
                    confirmation_score=float(confirmation_score)
                    if isinstance(confirmation_score, (int, float))
                    else None,
                    confirmed_by=confirmed_by if isinstance(confirmed_by, str) else None,
                    hypothesis_id=linkage.get("hypothesis_id"),
                    correlation_id=linkage.get("correlation_id"),
                )
                contexts[signal_id] = context
            else:
                context.confirmation_event_id = context.confirmation_event_id or event.get("event_id")
                context.parent_event_id = context.parent_event_id or linkage.get("parent_event_id")
                context.confirmation_at = context.confirmation_at or parse_timestamp(event.get("occurred_at"))
                if isinstance(confirmation_score, (int, float)):
                    context.confirmation_score = float(confirmation_score)
                if isinstance(confirmed_by, str) and confirmed_by.strip():
                    context.confirmed_by = context.confirmed_by or confirmed_by.strip()
                context.aggregate_key = context.aggregate_key or event.get("aggregate_key")
                context.hypothesis_id = context.hypothesis_id or linkage.get("hypothesis_id")
                context.correlation_id = context.correlation_id or linkage.get("correlation_id")

            if isinstance(confirmed_by, str) and confirmed_by == AGENT_ID:
                self_processed_signal_ids.add(signal_id)

        elif event_type == "veto.raised":
            payload_scope = payload.get("scope")
            target_id = payload.get("target_id")
            raised_by = payload.get("raised_by")
            if payload_scope == "Signal" and isinstance(target_id, str) and target_id.strip():
                vetoed_signal_ids.add(target_id)
                if raised_by == AGENT_ID:
                    self_processed_signal_ids.add(target_id)

    return contexts, confirmed_signal_ids, self_processed_signal_ids, vetoed_signal_ids


def build_prompt(title: str, midpoint: float) -> tuple[str, str]:
    system_prompt = "Eres un forecaster calibrado. Responde SOLO con JSON válido."
    user_prompt = (
        f"Mercado: {title}\n"
        f"Precio actual: {midpoint} (probabilidad implícita del mercado)\n\n"
        f"El mercado dice que la probabilidad es {midpoint:.0%}.\n"
        "Basándote en tu conocimiento, ¿es correcta, alta o baja?\n\n"
        "JSON exacto requerido:\n"
        "{\"estimated_probability\": <float 0-1>, \"confidence\": \"low|medium|high\", "
        "\"reasoning\": \"<max 40 words>\", "
        "\"direction\": \"market_too_high|market_too_low|fair\"}"
    )
    return system_prompt, user_prompt


def resolve_openai_ip() -> str | None:
    try:
        infos = socket.getaddrinfo("api.openai.com", 443, type=socket.SOCK_STREAM)
    except socket.gaierror as error:
        return None

    for info in infos:
        address = info[4][0]
        if isinstance(address, str) and address:
            return address
    return None


def call_openai_api(title: str, midpoint: float) -> tuple[dict[str, Any], int, int] | tuple[None, str]:
    api_key = os.getenv("OPENAI_API_KEY")
    if not api_key:
        return None, "OPENAI_API_KEY missing"
    if not isinstance(title, str) or not title.strip():
        return None, "market title missing"

    normalized_title = title.strip()
    system_prompt, user_prompt = build_prompt(normalized_title, midpoint)
    payload = {
        "model": MODEL_NAME,
        "max_tokens": 150,
        "response_format": {"type": "json_object"},
        "messages": [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": user_prompt},
        ],
    }

    openai_ip = resolve_openai_ip()
    if openai_ip is None:
        return None, "failed to resolve api.openai.com"

    try:
        result = subprocess.run(
            [
                "curl",
                "-s",
                "--retry",
                "3",
                "--retry-all-errors",
                "--retry-delay",
                "1",
                "--resolve",
                f"api.openai.com:443:{openai_ip}",
                "-X",
                "POST",
                OPENAI_ENDPOINT,
                "-H",
                "Content-Type: application/json",
                "-H",
                f"Authorization: Bearer {api_key}",
                "-d",
                json.dumps(payload),
                "--max-time",
                "20",
            ],
            capture_output=True,
            text=True,
        )
    except OSError as error:
        return None, f"{error.__class__.__name__}: {error}"

    if result.returncode != 0:
        stderr = result.stderr.strip()
        stdout = result.stdout.strip()
        detail = stderr or stdout or f"curl exited with status {result.returncode}"
        return None, detail

    try:
        payload = json.loads(result.stdout)
    except ValueError:
        return None, "invalid JSON response from OpenAI"

    if isinstance(payload, dict) and "error" in payload:
        error = payload.get("error")
        if isinstance(error, dict):
            message = error.get("message")
            if isinstance(message, str) and message.strip():
                return None, f"OpenAI error: {message.strip()}"
        return None, "OpenAI returned an error response"

    if not isinstance(payload, dict):
        return None, "OpenAI response was not a JSON object"

    choices = payload.get("choices")
    if not isinstance(choices, list) or not choices:
        return None, "OpenAI response missing choices"

    message = choices[0].get("message") if isinstance(choices[0], dict) else None
    content = message.get("content") if isinstance(message, dict) else None
    if not isinstance(content, str):
        return None, "OpenAI response missing message.content"

    try:
        parsed = json.loads(content)
    except json.JSONDecodeError:
        return None, "OpenAI message content was not valid JSON"

    if not isinstance(parsed, dict):
        return None, "OpenAI message content was not a JSON object"

    usage = payload.get("usage") if isinstance(payload.get("usage"), dict) else {}
    input_tokens = usage.get("prompt_tokens")
    output_tokens = usage.get("completion_tokens")
    return (
        parsed,
        int(input_tokens) if isinstance(input_tokens, int) else 0,
        int(output_tokens) if isinstance(output_tokens, int) else 0,
    )


def normalize_probability(value: Any) -> float | None:
    if isinstance(value, (int, float)) and math.isfinite(float(value)):
        probability = float(value)
        if 0.0 <= probability <= 1.0:
            return probability
    return None


def normalize_confidence(value: Any) -> str | None:
    if isinstance(value, str):
        trimmed = value.strip()
        if trimmed in {"low", "medium", "high"}:
            return trimmed
    return None


def normalize_direction(value: Any) -> str | None:
    if isinstance(value, str):
        trimmed = value.strip()
        if trimmed in {"market_too_high", "market_too_low", "fair"}:
            return trimmed
    return None


def normalize_reasoning(value: Any) -> str | None:
    if not isinstance(value, str):
        return None
    trimmed = " ".join(value.split())
    if not trimmed:
        return None
    words = trimmed.split()
    if len(words) > 40:
        trimmed = " ".join(words[:40])
    return trimmed


def estimated_cost_usd(input_tokens: int, output_tokens: int) -> float:
    return (
        (input_tokens / 1_000_000.0) * INPUT_COST_PER_1M_TOKENS
        + (output_tokens / 1_000_000.0) * OUTPUT_COST_PER_1M_TOKENS
    )


def key_fingerprint(value: str) -> str:
    digest = hashlib.sha256(value.encode("utf-8")).hexdigest()
    return digest[:12]


def deterministic_veto_id(signal_id: str) -> str:
    return f"probability-veto-{signal_id}"


def existing_event_ids(events: list[dict[str, Any]]) -> tuple[set[str], set[str]]:
    confirmed: set[str] = set()
    vetoed: set[str] = set()

    for event in events:
        event_type = event.get("event_type")
        payload = event.get("payload") or {}
        if event_type == "signal.confirmed" and payload.get("confirmed_by") == AGENT_ID:
            signal_id = payload.get("signal_id")
            if isinstance(signal_id, str):
                confirmed.add(signal_id)
        elif event_type == "veto.raised" and payload.get("raised_by") == AGENT_ID:
            target_id = payload.get("target_id")
            if isinstance(target_id, str):
                vetoed.add(target_id)

    return confirmed, vetoed


def build_provenance(
    run_id: str,
    watch_dir: Path,
    signal_id: str,
    notes: dict[str, Any],
) -> dict[str, Any]:
    return {
        "source_kind": "Runtime",
        "source_ref": str(watch_dir),
        "producer_run_id": run_id,
        "actor": AGENT_ID,
        "trace_id": f"{run_id}:{signal_id}",
        "notes": json.dumps(notes, separators=(",", ":"), sort_keys=True),
    }


def build_confirmed_event(
    candidate: SignalCandidate,
    run_id: str,
    watch_dir: Path,
    api_result: dict[str, Any],
    input_tokens: int,
    output_tokens: int,
) -> dict[str, Any]:
    estimated_probability = normalize_probability(api_result.get("estimated_probability"))
    assert estimated_probability is not None
    midpoint = candidate.midpoint
    p_final = (ALPHA * estimated_probability) + ((1.0 - ALPHA) * midpoint)
    confidence = normalize_confidence(api_result.get("confidence")) or "medium"
    direction = normalize_direction(api_result.get("direction")) or "fair"
    reasoning = normalize_reasoning(api_result.get("reasoning")) or "model output unavailable"
    cost = estimated_cost_usd(input_tokens, output_tokens)

    notes = {
        "model": MODEL_NAME,
        "alpha": ALPHA,
        "market_midpoint": midpoint,
        "p_market": midpoint,
        "estimated_probability": estimated_probability,
        "p_final": p_final,
        "confidence": confidence,
        "direction": direction,
        "reasoning": reasoning,
        "input_tokens": input_tokens,
        "output_tokens": output_tokens,
        "estimated_cost_usd": round(cost, 8),
        "action": "signal.confirmed",
    }

    return {
        "event_id": str(uuid.uuid4()),
        "event_type": "signal.confirmed",
        "schema_version": "v1",
        "occurred_at": utc_now_rfc3339(),
        "produced_by": PRODUCED_BY,
        "idempotency_key": f"signal.confirmed:v1:{candidate.signal_id}:{AGENT_ID}",
        "aggregate_key": candidate.aggregate_key,
        "linkage": {
            "hypothesis_id": candidate.hypothesis_id,
            "signal_id": candidate.signal_id,
            "decision_id": None,
            "order_id": None,
            "position_id": None,
            "parent_event_id": candidate.confirmation_event_id or candidate.generated_event_id,
            "correlation_id": candidate.correlation_id,
        },
        "provenance": build_provenance(run_id, watch_dir, candidate.signal_id, notes),
        "payload": {
            "signal_id": candidate.signal_id,
            "confirmed_by": AGENT_ID,
            "confirmation_reason": (
                f"mixmcp alpha={ALPHA:.1f} direction={direction} confidence={confidence}"
            ),
            "confirmation_score": p_final,
            "estimated_probability": estimated_probability,
            "market_midpoint": midpoint,
            "p_final": p_final,
            "confidence": confidence,
            "reasoning": reasoning,
            "direction": direction,
            "model": MODEL_NAME,
            "alpha": ALPHA,
            "input_tokens": input_tokens,
            "output_tokens": output_tokens,
            "estimated_cost_usd": round(cost, 8),
        },
    }


def build_veto_event(
    candidate: SignalCandidate,
    run_id: str,
    watch_dir: Path,
    api_result: dict[str, Any],
    input_tokens: int,
    output_tokens: int,
    p_final: float,
) -> dict[str, Any]:
    estimated_probability = normalize_probability(api_result.get("estimated_probability"))
    assert estimated_probability is not None
    confidence = normalize_confidence(api_result.get("confidence")) or "medium"
    direction = normalize_direction(api_result.get("direction")) or "fair"
    reasoning = normalize_reasoning(api_result.get("reasoning")) or "model output unavailable"
    cost = estimated_cost_usd(input_tokens, output_tokens)

    reason_code = "probability_out_of_bounds"
    reason_text = (
        f"probability blend {p_final:.6f} is outside [{MIN_FINAL_PROBABILITY:.2f}, "
        f"{MAX_FINAL_PROBABILITY:.2f}]"
    )
    veto_id = deterministic_veto_id(candidate.signal_id)

    notes = {
        "model": MODEL_NAME,
        "alpha": ALPHA,
        "market_midpoint": candidate.midpoint,
        "p_market": candidate.midpoint,
        "estimated_probability": estimated_probability,
        "p_final": p_final,
        "confidence": confidence,
        "direction": direction,
        "reasoning": reasoning,
        "input_tokens": input_tokens,
        "output_tokens": output_tokens,
        "estimated_cost_usd": round(cost, 8),
        "action": "veto.raised",
    }

    return {
        "event_id": str(uuid.uuid4()),
        "event_type": "veto.raised",
        "schema_version": "v1",
        "occurred_at": utc_now_rfc3339(),
        "produced_by": PRODUCED_BY,
        "idempotency_key": f"veto.raised:v1:{veto_id}",
        "aggregate_key": candidate.aggregate_key,
        "linkage": {
            "hypothesis_id": candidate.hypothesis_id,
            "signal_id": candidate.signal_id,
            "decision_id": None,
            "order_id": None,
            "position_id": None,
            "parent_event_id": candidate.confirmation_event_id or candidate.generated_event_id,
            "correlation_id": candidate.correlation_id,
        },
        "provenance": build_provenance(run_id, watch_dir, candidate.signal_id, notes),
        "payload": {
            "veto_id": veto_id,
            "scope": "Signal",
            "target_id": candidate.signal_id,
            "reason_code": reason_code,
            "reason_text": reason_text,
            "raised_by": AGENT_ID,
            "estimated_probability": estimated_probability,
            "market_midpoint": candidate.midpoint,
            "p_final": p_final,
            "confidence": confidence,
            "reasoning": reasoning,
            "direction": direction,
            "model": MODEL_NAME,
            "alpha": ALPHA,
            "input_tokens": input_tokens,
            "output_tokens": output_tokens,
            "estimated_cost_usd": round(cost, 8),
        },
    }


def append_event(store_path: Path, event: dict[str, Any]) -> None:
    store_path.parent.mkdir(parents=True, exist_ok=True)
    with store_path.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(event, separators=(",", ":")) + "\n")


def main() -> int:
    args = parse_args()
    if args.max_signals < 0:
        raise SystemExit("--max-signals must be >= 0")

    api_key = os.environ.get("OPENAI_API_KEY", "")
    if not api_key or api_key == "tu api aqui perro":
        print(json.dumps({"error": "OPENAI_API_KEY no configurada"}))
        return 1
    openai_key_fingerprint = key_fingerprint(api_key)

    store_path = Path(args.store)
    watch_dir = Path(args.watch_dir)
    run_id = execution_run_id()

    events = load_jsonl(store_path)
    latest = latest_snapshots(watch_dir)
    contexts, confirmed_signal_ids, self_processed_signal_ids, vetoed_signal_ids = collect_candidates(
        events
    )
    already_confirmed, already_vetoed = existing_event_ids(events)
    skipped_missing_snapshot = 0

    eligible_candidates: list[SignalCandidate] = []
    processed_signal_ids = self_processed_signal_ids | already_confirmed | already_vetoed
    for signal_id in sorted(confirmed_signal_ids):
        if signal_id in processed_signal_ids:
            continue

        context = contexts.get(signal_id)
        if context is None:
            continue
        if context.market_id not in latest:
            skipped_missing_snapshot += 1
            continue

        snapshot = latest[context.market_id]
        context.title = snapshot.title
        context.midpoint = snapshot.midpoint
        eligible_candidates.append(context)

    eligible_candidates.sort(
        key=lambda candidate: (
            candidate.confirmation_at or candidate.generated_at or datetime.min.replace(tzinfo=timezone.utc),
            candidate.signal_id,
        ),
        reverse=True,
    )
    eligible_candidates = eligible_candidates[: args.max_signals]

    confirmed_written = 0
    vetoed_written = 0
    skipped_existing = 0
    skipped_api_failure = 0
    skipped_unscorable = 0
    input_tokens_total = 0
    output_tokens_total = 0
    estimated_cost_total = 0.0
    last_api_error = ""
    last_title_sample = ""

    for candidate in eligible_candidates:
        last_title_sample = candidate.title[:60]
        if candidate.signal_id in vetoed_signal_ids:
            skipped_existing += 1
            continue

        api_result = call_openai_api(candidate.title, candidate.midpoint)
        if isinstance(api_result, tuple) and len(api_result) == 2 and api_result[0] is None:
            last_api_error = api_result[1]
            skipped_api_failure += 1
            continue

        parsed, input_tokens, output_tokens = api_result
        estimated_probability = normalize_probability(parsed.get("estimated_probability"))
        if estimated_probability is None:
            skipped_unscorable += 1
            continue

        p_final = (ALPHA * estimated_probability) + ((1.0 - ALPHA) * candidate.midpoint)
        if not math.isfinite(p_final):
            skipped_unscorable += 1
            continue

        input_tokens_total += input_tokens
        output_tokens_total += output_tokens
        estimated_cost_total += estimated_cost_usd(input_tokens, output_tokens)

        if p_final < MIN_FINAL_PROBABILITY or p_final > MAX_FINAL_PROBABILITY:
            append_event(
                store_path,
                build_veto_event(candidate, run_id, watch_dir, parsed, input_tokens, output_tokens, p_final),
            )
            vetoed_written += 1
            continue

        append_event(
            store_path,
            build_confirmed_event(candidate, run_id, watch_dir, parsed, input_tokens, output_tokens),
        )
        confirmed_written += 1

    print(
        json.dumps(
            {
                "actor": AGENT_ID,
                "producer_run_id": run_id,
                "store": str(store_path),
                "watch_dir": str(watch_dir),
                "model": MODEL_NAME,
                "alpha": ALPHA,
                "openai_key_loaded": True,
                "openai_key_fingerprint": openai_key_fingerprint,
                "max_signals": args.max_signals,
                "eligible_candidates": len(eligible_candidates),
                "confirmed_written": confirmed_written,
                "vetoed_written": vetoed_written,
                "skipped_existing": skipped_existing,
                "skipped_missing_snapshot": skipped_missing_snapshot,
                "skipped_api_failure": skipped_api_failure,
                "skipped_unscorable": skipped_unscorable,
                "last_api_error": last_api_error,
                "last_title_sample": last_title_sample,
                "input_tokens": input_tokens_total,
                "output_tokens": output_tokens_total,
                "estimated_cost_usd": round(estimated_cost_total, 8),
            },
            separators=(",", ":"),
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
