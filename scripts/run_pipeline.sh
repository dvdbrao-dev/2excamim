#!/usr/bin/env bash
set -euo pipefail

export PYTHONPATH="${PYTHONPATH:-/root/2excamim}"

KILL_SWITCH="./var/.kill_switch"
FAILURES_DIR="./var/.failures"
MAX_FAILURES=5

# ---------------------------------------------------------------------------
# Kill switch — abort silently when the file exists
# ---------------------------------------------------------------------------
if [[ -f "${KILL_SWITCH}" ]]; then
    echo "[pipeline] Kill switch active (${KILL_SWITCH}). Remove the file to re-enable." >&2
    exit 0
fi

# ---------------------------------------------------------------------------
# Repair event store (must run before any agent touches events.jsonl)
# ---------------------------------------------------------------------------
python3 scripts/repair_store.py

# ---------------------------------------------------------------------------
# run_agent <name> <cmd...>
#
# Runs a command. On failure:
#   - increments var/.failures/<name>.count
#   - when count reaches MAX_FAILURES, creates the kill switch
#   - never aborts the pipeline (returns 0 regardless)
# On success:
#   - resets the counter file
# ---------------------------------------------------------------------------
run_agent() {
    local agent_name="$1"
    shift
    local counter_file="${FAILURES_DIR}/${agent_name}.count"

    if "$@"; then
        rm -f "${counter_file}"
        return 0
    fi

    mkdir -p "${FAILURES_DIR}"
    local count=0
    if [[ -f "${counter_file}" ]]; then
        count=$(cat "${counter_file}" 2>/dev/null || echo 0)
    fi
    count=$(( count + 1 ))
    printf '%s\n' "${count}" > "${counter_file}"
    echo "[pipeline] WARN: ${agent_name} failed (consecutive: ${count}/${MAX_FAILURES})" >&2

    if (( count >= MAX_FAILURES )); then
        echo "[pipeline] KILL SWITCH triggered by ${agent_name} after ${count} consecutive failures." >&2
        touch "${KILL_SWITCH}"
    fi

    return 0
}

# ---------------------------------------------------------------------------
# Crypto strategy agents (opt-in via ENABLE_CRYPTO_STRATEGIES=1)
# These agents only need events.jsonl + crypto_ohlcv — no market-watch dep.
# Running before the market-watch hard dependency ensures the OpenFang bridge
# always sees up-to-date registry/scorecard data regardless of whether the
# Rust runtime completes.
# ---------------------------------------------------------------------------
if [[ "${ENABLE_CRYPTO_STRATEGIES:-0}" == "1" ]]; then
    run_agent "crypto_adx_ema_pullback" \
        python3 agents/crypto_adx_ema_pullback_agent.py \
            --store ./var/events.jsonl --cache-dir ./var/crypto_ohlcv

    run_agent "crypto_volatility_breakout" \
        python3 agents/crypto_volatility_breakout_agent.py \
            --store ./var/events.jsonl --cache-dir ./var/crypto_ohlcv

    run_agent "crypto_strategy_scorecard" \
        python3 agents/crypto_strategy_scorecard_agent.py \
            --store ./var/events.jsonl --json

    run_agent "crypto_strategy_incubator" \
        python3 agents/crypto_strategy_incubator_agent.py --json
fi

# ---------------------------------------------------------------------------
# OpenFang bridge — read-only state export (never aborts pipeline).
# Placed here so it always runs with the freshest registry/scorecard data
# and before the market-watch hard dependency that could abort the pipeline.
# Falls back to files from the previous run when ENABLE_CRYPTO_STRATEGIES=0.
# ---------------------------------------------------------------------------
python3 agents/openfang_bridge.py \
    --registry ./runtime/crypto_strategy_registry.json \
    --scorecard ./runtime/crypto_strategy_scorecard.json \
    --kill-switch ./var/.kill_switch \
    --output ./runtime/openfang_state.json \
    || echo "[pipeline] WARN: openfang_bridge failed, continuing"

# ---------------------------------------------------------------------------
# Market-watch Rust runtime — hard dependency; pipeline aborts if this fails.
# Core agents below require ./var/market-watch snapshots.
# ---------------------------------------------------------------------------
timeout 60 cargo run -p market-watch -- --state-dir ./var/market-watch

# ---------------------------------------------------------------------------
# Core pipeline agents
# ---------------------------------------------------------------------------
run_agent "scoring" \
    python3 agents/scoring_agent.py \
        --store ./var/events.jsonl --watch-dir ./var/market-watch

run_agent "signal" \
    python3 agents/signal_agent.py \
        --store ./var/events.jsonl --watch-dir ./var/market-watch

run_agent "confirmation" \
    python3 agents/confirmation_agent.py \
        --store ./var/events.jsonl

run_agent "probability" \
    python3 agents/probability_agent.py \
        --store ./var/events.jsonl --watch-dir ./var/market-watch --max-signals 20

run_agent "veto" \
    python3 agents/veto_agent.py \
        --store ./var/events.jsonl --probability-floor 0.05

run_agent "sizing" \
    python3 agents/sizing_agent.py \
        --store ./var/events.jsonl --watch-dir ./var/market-watch --bankroll 1000.0

run_agent "exit" \
    python3 agents/exit_agent.py \
        --store ./var/events.jsonl --watch-dir ./var/market-watch

run_agent "telegram" \
    python3 agents/telegram_agent.py \
        --store ./var/events.jsonl

# ---------------------------------------------------------------------------
# Shadow live — simulate realistic fills for every decision.formed
# ---------------------------------------------------------------------------
run_agent "shadow_live" \
    python3 agents/shadow_live_agent.py \
        --store ./var/events.jsonl --watch-dir ./var/market-watch

# ---------------------------------------------------------------------------
# Drawdown guard — circuit breaker; may touch var/.kill_switch
# ---------------------------------------------------------------------------
run_agent "drawdown_guard" \
    python3 agents/drawdown_guard.py \
        --store ./var/events.jsonl

# Live Gateway — descomentar post-migración V2
# run_agent "live_gateway" python3 agents/live_gateway.py --store ./var/events.jsonl
