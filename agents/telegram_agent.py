#!/usr/bin/env python3
"""Telegram notification agent for EXCAMIM."""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from dataclasses import dataclass
from datetime import date, datetime, timezone
from pathlib import Path
from typing import Any


AGENT_ID = "telegram-agent-v1"
DEFAULT_STORE = Path("./var/events.jsonl")
DEFAULT_SENT_STATE = Path("./var/telegram_sent.json")
DEFAULT_WATCH_DIR = Path("./var/market-watch")
DEFAULT_DAILY_SUMMARY = False
DEFAULT_CHECK_API: int | None = None
DEFAULT_CHECK_DISK: int | None = None
PROBABILITY_AGENT = "probability-agent-v1"
SIZING_AGENT = "sizing-agent-v1"
EXIT_AGENT = "exit-agent-v1"
REASON_SNIPPET_LENGTH = 80
USD_TO_EUR = 0.92

RE_BANKROLL = re.compile(r"\bbankroll=([0-9]+(?:\.[0-9]+)?)\b")


@dataclass
class NotificationState:
    event_ids: set[str]
    notification_ids: set[str]


@dataclass
class SignalContext:
    event_id: str
    signal_id: str
    market_id: str
    title: str
    p_market: float | None
    p_llm: float | None
    p_final: float | None
    direction: str | None
    confidence: str | None


@dataclass
class DecisionContext:
    event_id: str
    decision_id: str
    signal_id: str | None
    market_id: str
    title: str
    size_hint: float | None
    bankroll: float | None
    p_final: float | None
    direction: str | None


@dataclass
class VetoContext:
    event_id: str
    veto_id: str
    target_id: str
    scope: str | None
    raised_by: str | None
    reason_code: str | None
    reason_text: str | None
    market_id: str
    title: str


@dataclass
class CryptoSignalContext:
    event_id: str
    signal_id: str
    symbol: str
    price_now: float
    change_pct: float
    trend: str
    signal_strength: float
    n_markets: int


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Send Telegram notifications for EXCAMIM.")
    parser.add_argument("--store", default=str(DEFAULT_STORE), help="Path to the JSONL store.")
    parser.add_argument(
        "--sent-state",
        default=str(DEFAULT_SENT_STATE),
        help="Path to the Telegram notification state file.",
    )
    parser.add_argument(
        "--watch-dir",
        default=str(DEFAULT_WATCH_DIR),
        help="Path to the market-watch state directory.",
    )
    parser.add_argument(
        "--daily-summary",
        action="store_true",
        default=DEFAULT_DAILY_SUMMARY,
        help="Send the daily operational summary.",
    )
    parser.add_argument(
        "--check-api",
        type=int,
        default=DEFAULT_CHECK_API,
        help="Send the OpenAI API alert when failures exceed the threshold.",
    )
    parser.add_argument(
        "--check-disk",
        type=int,
        default=DEFAULT_CHECK_DISK,
        help="Send the disk usage alert when usage exceeds the threshold.",
    )
    return parser.parse_args()


def utc_now_rfc3339() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def parse_timestamp(value: Any) -> datetime | None:
    if isinstance(value, str):
        trimmed = value.strip()
        if not trimmed:
            return None
        try:
            return datetime.fromisoformat(trimmed.replace("Z", "+00:00")).astimezone(timezone.utc)
        except ValueError:
            return None
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
            records.append(json.loads(stripped))
        except json.JSONDecodeError as error:
            raise SystemExit(f"invalid JSONL in {path} at line {line_number}: {error}") from error
    return records


def load_latest_titles(watch_dir: Path) -> dict[str, str]:
    snapshots_path = watch_dir / "snapshots.jsonl"
    latest: dict[str, tuple[datetime, str]] = {}

    for snapshot in load_jsonl(snapshots_path):
        market_id = normalized_market_id(
            snapshot.get("market_id") or snapshot.get("instrument") or snapshot.get("aggregate_key")
        )
        observed_at = parse_timestamp(snapshot.get("observed_at"))
        title = snapshot.get("title")
        if market_id is None or observed_at is None or not isinstance(title, str) or not title.strip():
            continue

        current = latest.get(market_id)
        if current is None or observed_at > current[0]:
            latest[market_id] = (observed_at, title.strip())

    return {market_id: title for market_id, (_, title) in latest.items()}


def normalized_market_id(value: Any) -> str | None:
    if not isinstance(value, str):
        return None
    trimmed = value.strip()
    if not trimmed:
        return None
    if ":" in trimmed:
        trimmed = trimmed.split(":", 1)[1]
    return trimmed


def market_id_from_event(event: dict[str, Any]) -> str | None:
    payload = event.get("payload") or {}
    linkage = event.get("linkage") or {}
    return (
        normalized_market_id(event.get("aggregate_key"))
        or normalized_market_id(payload.get("market_id"))
        or normalized_market_id(payload.get("instrument"))
        or normalized_market_id(linkage.get("signal_id"))
    )


def market_title(
    market_id: str | None, titles_by_market: dict[str, str], fallback: str | None = None
) -> str:
    if market_id is not None:
        title = titles_by_market.get(market_id)
        if title:
            return title
    if fallback:
        return fallback
    if market_id:
        return market_id
    return "unknown"


def escape_markdown(text: str) -> str:
    escaped = text.replace("\\", "\\\\")
    for char in ("_", "*", "[", "]", "(", ")"):
        escaped = escaped.replace(char, f"\\{char}")
    return escaped


def clean_snippet(text: Any, length: int) -> str:
    if not isinstance(text, str):
        return ""
    snippet = " ".join(text.strip().split())
    return snippet[:length]


def parse_bankroll(value: Any) -> float | None:
    if not isinstance(value, str):
        return None
    match = RE_BANKROLL.search(value)
    if not match:
        return None
    try:
        bankroll = float(match.group(1))
    except ValueError:
        return None
    if bankroll <= 0:
        return None
    return bankroll


def numeric_value(value: Any) -> float | None:
    if isinstance(value, (int, float)):
        return float(value)
    return None


def load_sent_state(path: Path) -> NotificationState:
    if not path.exists():
        return NotificationState(event_ids=set(), notification_ids=set())

    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return NotificationState(event_ids=set(), notification_ids=set())
    event_ids: set[str] = set()
    notification_ids: set[str] = set()

    if isinstance(raw, list):
        for value in raw:
            if isinstance(value, str) and value.strip():
                event_ids.add(value.strip())
    elif isinstance(raw, dict):
        for key, target in (("event_ids", event_ids), ("notification_ids", notification_ids), ("sent_ids", event_ids)):
            values = raw.get(key)
            if isinstance(values, list):
                for value in values:
                    if isinstance(value, str) and value.strip():
                        target.add(value.strip())
    else:
        raise SystemExit(f"invalid notification state in {path}")

    return NotificationState(event_ids=event_ids, notification_ids=notification_ids)


def save_sent_state(path: Path, state: NotificationState) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    payload = {
        "event_ids": sorted(state.event_ids),
        "notification_ids": sorted(state.notification_ids),
        "updated_at": utc_now_rfc3339(),
    }
    path.write_text(json.dumps(payload, separators=(",", ":"), sort_keys=True) + "\n", encoding="utf-8")


def append_sent_id(state: NotificationState, kind: str, value: str) -> None:
    if kind == "event":
        state.event_ids.add(value)
    else:
        state.notification_ids.add(value)


def already_sent(state: NotificationState, kind: str, value: str) -> bool:
    return value in state.event_ids if kind == "event" else value in state.notification_ids


def provenance_actor(event: dict[str, Any]) -> str | None:
    provenance = event.get("provenance") or {}
    actor = provenance.get("actor")
    if isinstance(actor, str) and actor.strip():
        return actor.strip()
    payload = event.get("payload") or {}
    for key in ("confirmed_by", "raised_by"):
        candidate = payload.get(key)
        if isinstance(candidate, str) and candidate.strip():
            return candidate.strip()
    return None


def collect_signal_contexts(
    events: list[dict[str, Any]], titles_by_market: dict[str, str]
) -> dict[str, SignalContext]:
    contexts: dict[str, SignalContext] = {}

    for event in events:
        if event.get("event_type") != "signal.confirmed":
            continue
        if provenance_actor(event) != PROBABILITY_AGENT:
            continue

        payload = event.get("payload") or {}
        signal_id = payload.get("signal_id")
        if not isinstance(signal_id, str) or not signal_id.strip():
            continue

        market_id = market_id_from_event(event)
        title = market_title(market_id, titles_by_market)
        contexts[signal_id] = SignalContext(
            event_id=str(event.get("event_id") or ""),
            signal_id=signal_id,
            market_id=market_id or title,
            title=title,
            p_market=numeric_value(payload.get("p_market") or payload.get("market_midpoint")),
            p_llm=numeric_value(payload.get("estimated_probability")),
            p_final=numeric_value(payload.get("p_final") or payload.get("confirmation_score")),
            direction=payload.get("direction") if isinstance(payload.get("direction"), str) else None,
            confidence=payload.get("confidence") if isinstance(payload.get("confidence"), str) else None,
        )

    return contexts


def collect_decision_contexts(
    events: list[dict[str, Any]], titles_by_market: dict[str, str], signal_contexts: dict[str, SignalContext]
) -> dict[str, DecisionContext]:
    contexts: dict[str, DecisionContext] = {}

    for event in events:
        if event.get("event_type") != "decision.formed":
            continue
        if provenance_actor(event) != SIZING_AGENT:
            continue

        payload = event.get("payload") or {}
        linkage = event.get("linkage") or {}
        decision_id = payload.get("decision_id")
        if not isinstance(decision_id, str) or not decision_id.strip():
            continue

        market_id = market_id_from_event(event)
        signal_id = linkage.get("signal_id") if isinstance(linkage.get("signal_id"), str) else None
        upstream = signal_contexts.get(signal_id) if signal_id else None
        title = market_title(market_id or (upstream.market_id if upstream else None), titles_by_market)

        contexts[decision_id] = DecisionContext(
            event_id=str(event.get("event_id") or ""),
            decision_id=decision_id,
            signal_id=signal_id,
            market_id=market_id or title,
            title=title,
            size_hint=numeric_value(payload.get("size_hint")),
            bankroll=parse_bankroll(payload.get("rationale")) or parse_bankroll(
                (event.get("provenance") or {}).get("notes")
            ),
            p_final=upstream.p_final if upstream else None,
            direction=upstream.direction if upstream else None,
        )

    return contexts


def collect_veto_contexts(
    events: list[dict[str, Any]], titles_by_market: dict[str, str]
) -> dict[str, VetoContext]:
    contexts: dict[str, VetoContext] = {}

    for event in events:
        if event.get("event_type") != "veto.raised":
            continue

        actor = provenance_actor(event)
        if actor not in {PROBABILITY_AGENT, EXIT_AGENT}:
            continue

        payload = event.get("payload") or {}
        veto_id = payload.get("veto_id")
        target_id = payload.get("target_id")
        if not isinstance(veto_id, str) or not veto_id.strip():
            continue
        if not isinstance(target_id, str) or not target_id.strip():
            continue

        market_id = market_id_from_event(event)
        title = market_title(market_id, titles_by_market)
        contexts[veto_id] = VetoContext(
            event_id=str(event.get("event_id") or ""),
            veto_id=veto_id,
            target_id=target_id,
            scope=payload.get("scope") if isinstance(payload.get("scope"), str) else None,
            raised_by=actor,
            reason_code=payload.get("reason_code") if isinstance(payload.get("reason_code"), str) else None,
            reason_text=payload.get("reason_text") if isinstance(payload.get("reason_text"), str) else None,
            market_id=market_id or title,
            title=title,
        )

    return contexts


def collect_crypto_signal_contexts(events: list[dict[str, Any]]) -> dict[str, CryptoSignalContext]:
    contexts: dict[str, tuple[datetime, CryptoSignalContext]] = {}

    for event in events:
        event_type = event.get("event_type")
        payload = event.get("payload") or {}
        occurred_at = parse_timestamp(event.get("occurred_at")) or datetime.min.replace(
            tzinfo=timezone.utc
        )

        if event_type == "crypto.signal.generated":
            signal_id = payload.get("signal_id")
            symbol = payload.get("symbol")
            price_now = numeric_value(payload.get("price_now"))
            change_pct = numeric_value(payload.get("change_pct"))
            trend = payload.get("trend")
            signal_strength = numeric_value(payload.get("signal_strength"))
            if (
                not isinstance(signal_id, str)
                or not signal_id.strip()
                or not isinstance(symbol, str)
                or not symbol.strip()
                or price_now is None
                or change_pct is None
                or trend not in {"UP", "DOWN"}
                or signal_strength is None
            ):
                continue

            current = contexts.get(signal_id)
            candidate = CryptoSignalContext(
                event_id=str(event.get("event_id") or ""),
                signal_id=signal_id,
                symbol=symbol.strip(),
                price_now=price_now,
                change_pct=change_pct,
                trend=trend,
                signal_strength=signal_strength,
                n_markets=0,
            )
            if current is None or occurred_at > current[0]:
                contexts[signal_id] = (occurred_at, candidate)
            continue

        if event_type != "crypto.market.matched":
            continue

        signal_id = payload.get("signal_id")
        matched_markets = payload.get("matched_markets")
        if not isinstance(signal_id, str) or not signal_id.strip() or not isinstance(matched_markets, list):
            continue

        current = contexts.get(signal_id)
        if current is None:
            continue

        context = current[1]
        updated = CryptoSignalContext(
            event_id=str(event.get("event_id") or ""),
            signal_id=context.signal_id,
            symbol=context.symbol,
            price_now=context.price_now,
            change_pct=context.change_pct,
            trend=context.trend,
            signal_strength=context.signal_strength,
            n_markets=len(matched_markets),
        )
        if occurred_at >= current[0]:
            contexts[signal_id] = (occurred_at, updated)

    return {signal_id: context for signal_id, (_, context) in contexts.items()}


def sorted_events(events: list[dict[str, Any]]) -> list[dict[str, Any]]:
    indexed = []
    for index, event in enumerate(events):
        indexed.append((parse_timestamp(event.get("occurred_at")) or datetime.min.replace(tzinfo=timezone.utc), index, event))
    indexed.sort(key=lambda item: (item[0], item[1]))
    return [event for _, _, event in indexed]


def send_telegram(message: str) -> tuple[bool, str]:
    script = Path(__file__).resolve().parents[1] / "scripts" / "telegram.sh"
    result = subprocess.run(
        [str(script), message],
        cwd=Path(__file__).resolve().parents[1],
        capture_output=True,
        text=True,
    )
    if result.returncode == 0:
        return True, ""
    detail = (result.stderr or result.stdout or "").strip()
    return False, detail


def format_signal_confirmed(context: SignalContext) -> str:
    return (
        "🟡 *Señal confirmada*\n"
        f"Mercado: {escape_markdown(context.title)}\n"
        f"Precio mercado: {context.p_market if context.p_market is not None else 0.0:.0%}\n"
        f"Estimación LLM: {context.p_llm if context.p_llm is not None else 0.0:.0%}\n"
        f"Blend final: {context.p_final if context.p_final is not None else 0.0:.0%}\n"
        f"Direction: {escape_markdown(context.direction or 'unknown')}\n"
        f"Confidence: {escape_markdown(context.confidence or 'unknown')}"
    )


def format_decision_formed(context: DecisionContext) -> str:
    bankroll = context.bankroll or 0.0
    size_hint = context.size_hint or 0.0
    pct = (size_hint / bankroll * 100.0) if bankroll > 0 else 0.0
    return (
        "🟢 *DECISIÓN EXCAMIM*\n"
        f"Mercado: {escape_markdown(context.title)}\n"
        f"Kelly size: ${size_hint:.2f}\n"
        f"p\\_final: {context.p_final if context.p_final is not None else 0.0:.0%}\n"
        f"Direction: {escape_markdown(context.direction or 'unknown')}\n"
        f"Bankroll usado: {pct:.1f}%"
    )


def format_probability_veto(context: VetoContext) -> str:
    return (
        f"🔴 *Veto*: {escape_markdown(context.reason_code or 'unknown')}\n"
        f"{escape_markdown(context.reason_text[:REASON_SNIPPET_LENGTH] if context.reason_text else '')}"
    )


def format_exit_veto(context: VetoContext) -> str:
    return (
        f"🏁 *Exit trigger*: {escape_markdown(context.reason_code or 'unknown')}\n"
        f"Mercado: {escape_markdown(context.title)}\n"
        f"Razón: {escape_markdown(context.reason_text or '')}"
    )


def format_crypto_signal(context: CryptoSignalContext) -> str:
    return (
        "⚡ *SEÑAL CRIPTO*\n"
        f"{escape_markdown(context.symbol)}: {context.change_pct:+.2f}% en 15min\n"
        f"Precio: ${context.price_now:,.0f}\n"
        f"Tendencia: {escape_markdown(context.trend)}\n"
        f"Mercados Polymarket afectados: {context.n_markets}\n"
        f"Strength: {context.signal_strength:.0%}"
    )


def parse_notes_cost(notes: Any) -> float:
    if not isinstance(notes, str) or not notes.strip():
        return 0.0
    try:
        parsed = json.loads(notes)
    except json.JSONDecodeError:
        return 0.0
    value = parsed.get("estimated_cost_usd", 0.0)
    try:
        return float(value)
    except (TypeError, ValueError):
        return 0.0


def build_daily_summary(events: list[dict[str, Any]], titles_by_market: dict[str, str]) -> str:
    today = datetime.now(timezone.utc).date()
    todays_events = [event for event in events if (parse_timestamp(event.get("occurred_at")) or datetime.min.replace(tzinfo=timezone.utc)).date() == today]
    todays_events = sorted_events(todays_events)

    active_decision_ids: set[str] = set()
    closed_decision_ids: set[str] = set()
    generated = confirmed = vetoed = decisions = 0
    n_signals = 0
    cost_today = 0.0
    last_run = "n/a"

    for event in events:
        if event.get("event_type") == "decision.formed":
            payload = event.get("payload") or {}
            decision_id = payload.get("decision_id")
            if isinstance(decision_id, str) and decision_id.strip():
                active_decision_ids.add(decision_id.strip())
        elif event.get("event_type") == "veto.raised":
            payload = event.get("payload") or {}
            if payload.get("scope") == "Decision":
                target_id = payload.get("target_id")
                if isinstance(target_id, str) and target_id.strip():
                    closed_decision_ids.add(target_id.strip())

    active_count = len(active_decision_ids - closed_decision_ids)
    closed_count = len(closed_decision_ids)

    for event in todays_events:
        event_type = event.get("event_type")
        if event_type == "signal.generated":
            generated += 1
        elif event_type == "signal.confirmed":
            confirmed += 1
        elif event_type == "veto.raised":
            vetoed += 1
        elif event_type == "decision.formed":
            decisions += 1

        occurred_at = event.get("occurred_at")
        if isinstance(occurred_at, str) and occurred_at.strip():
            last_run = occurred_at

        if provenance_actor(event) == PROBABILITY_AGENT and event_type in {"signal.confirmed", "veto.raised"}:
            n_signals += 1
            provenance = event.get("provenance") or {}
            cost_today += parse_notes_cost(provenance.get("notes"))

    cost_eur = cost_today * USD_TO_EUR
    monthly = cost_today * 30.0
    return (
        "📊 *EXCAMIM Daily Report*\n"
        "━━━━━━━━━━━━━━━\n"
        "💰 *PnL paper*\n"
        f"Decisiones activas: {active_count}\n"
        f"Decisiones cerradas: {closed_count}\n"
        "Win rate estimado: n/a (pendiente resolución)\n\n"
        "🤖 *LLM*\n"
        f"Señales procesadas: {n_signals}\n"
        f"Coste hoy: ${cost_today:.4f} (€{cost_eur:.4f})\n"
        f"Proyección mes: ${monthly:.2f}/mes\n\n"
        "📈 *Pipeline hoy*\n"
        f"Signals generados: {generated}\n"
        f"Confirmados: {confirmed}\n"
        f"Vetados: {vetoed}\n"
        f"Decisiones formadas: {decisions}\n\n"
        "⚙️ *Sistema*\n"
        f"Último run: {escape_markdown(last_run)}\n"
        "Timers activos: OK"
    )


def build_api_alert(failures: int) -> str:
    return (
        "🔴 *ALERTA EXCAMIM*\n"
        f"OpenAI API: {failures} fallos en 24h\n"
        "Revisar créditos en platform.openai.com"
    )


def build_disk_alert(usage: int) -> str:
    return (
        "⚠️ *ALERTA EXCAMIM*\n"
        f"Disco al {usage}% - limpiar snapshots"
    )


def main() -> int:
    args = parse_args()
    store_path = Path(args.store)
    sent_state_path = Path(args.sent_state)
    watch_dir = Path(args.watch_dir)

    events = load_jsonl(store_path)
    events = sorted_events(events)
    titles_by_market = load_latest_titles(watch_dir)
    signal_contexts = collect_signal_contexts(events, titles_by_market)
    decision_contexts = collect_decision_contexts(events, titles_by_market, signal_contexts)
    veto_contexts = collect_veto_contexts(events, titles_by_market)
    crypto_signal_contexts = collect_crypto_signal_contexts(events)
    sent_state = load_sent_state(sent_state_path)

    sent_event_notifications = 0
    sent_other_notifications = 0
    skipped_existing = 0
    failures = 0

    for event in events:
        event_type = event.get("event_type")
        event_id = event.get("event_id")
        if not isinstance(event_id, str) or not event_id.strip():
            continue
        if already_sent(sent_state, "event", event_id):
            skipped_existing += 1
            continue

        actor = provenance_actor(event)
        message: str | None = None

        if event_type == "signal.confirmed" and actor == PROBABILITY_AGENT:
            payload = event.get("payload") or {}
            signal_id = payload.get("signal_id")
            if isinstance(signal_id, str):
                context = signal_contexts.get(signal_id)
                if context is not None:
                    message = format_signal_confirmed(context)

        if message is None and event_type == "crypto.market.matched" and actor == "crypto-matcher-v1":
            payload = event.get("payload") or {}
            signal_id = payload.get("signal_id")
            if isinstance(signal_id, str):
                context = crypto_signal_contexts.get(signal_id)
                if context is not None:
                    message = format_crypto_signal(context)

        if message is None:
            continue

        ok, detail = send_telegram(message)
        if ok:
            append_sent_id(sent_state, "event", event_id)
            sent_event_notifications += 1
        else:
            failures += 1
            print(json.dumps({"actor": AGENT_ID, "error": "telegram_send_failed", "detail": detail}))

    if args.daily_summary:
        notification_id = f"daily-summary:{datetime.now(timezone.utc).date().isoformat()}"
        if not already_sent(sent_state, "notification", notification_id):
            message = build_daily_summary(events, titles_by_market)
            ok, detail = send_telegram(message)
            if ok:
                append_sent_id(sent_state, "notification", notification_id)
                sent_other_notifications += 1
            else:
                failures += 1
                print(json.dumps({"actor": AGENT_ID, "error": "telegram_send_failed", "detail": detail}))

    if args.check_api is not None and args.check_api > 50:
        notification_id = f"check-api:{datetime.now(timezone.utc).date().isoformat()}:{args.check_api}"
        if not already_sent(sent_state, "notification", notification_id):
            ok, detail = send_telegram(build_api_alert(args.check_api))
            if ok:
                append_sent_id(sent_state, "notification", notification_id)
                sent_other_notifications += 1
            else:
                failures += 1
                print(json.dumps({"actor": AGENT_ID, "error": "telegram_send_failed", "detail": detail}))

    if args.check_disk is not None and args.check_disk > 80:
        notification_id = f"check-disk:{datetime.now(timezone.utc).date().isoformat()}:{args.check_disk}"
        if not already_sent(sent_state, "notification", notification_id):
            ok, detail = send_telegram(build_disk_alert(args.check_disk))
            if ok:
                append_sent_id(sent_state, "notification", notification_id)
                sent_other_notifications += 1
            else:
                failures += 1
                print(json.dumps({"actor": AGENT_ID, "error": "telegram_send_failed", "detail": detail}))

    save_sent_state(sent_state_path, sent_state)
    print(
        json.dumps(
            {
                "actor": AGENT_ID,
                "store": str(store_path),
                "sent_state": str(sent_state_path),
                "watch_dir": str(watch_dir),
                "sent_event_notifications": sent_event_notifications,
                "sent_other_notifications": sent_other_notifications,
                "skipped_existing": skipped_existing,
                "failures": failures,
            },
            separators=(",", ":"),
        )
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
