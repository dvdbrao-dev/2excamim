"""
Live Gateway Agent — Polymarket CLOB V2
Estado: INACTIVO hasta 22 abril 2026
Capital máximo esta fase: $50
Objetivo: medir fill rate real, no PnL
"""
import os
import json
import logging
from datetime import datetime, UTC

logger = logging.getLogger(__name__)

# Configuración
MAX_CAPITAL_USD = 50.0
MAX_ORDER_SIZE_USD = 10.0
MIN_SPREAD_TO_QUOTE = 0.008
TARGET_NO_FLOOR = 0.80
CLOB_HOST = "https://clob.polymarket.com"
CHAIN_ID = 137

def load_config():
    """Carga y valida variables de entorno requeridas."""
    required = [
        "EXCAMIM_PRIVATE_KEY",
        "POLY_API_KEY",
        "POLY_SECRET",
        "POLY_PASSPHRASE"
    ]
    missing = [k for k in required if not os.getenv(k)]
    if missing:
        raise EnvironmentError(
            f"Variables de entorno faltantes: {missing}. "
            f"Ver docs/LIVE_GATEWAY_SETUP.md"
        )
    return {k: os.getenv(k) for k in required}

def get_eligible_markets(events_path="var/events.jsonl"):
    """
    Lee decisiones formadas del store y devuelve mercados
    elegibles para colocar limit orders.
    Solo mercados con decision.formed y sin fill.received.
    """
    decisions = {}
    fills = set()

    with open(events_path) as f:
        for line in f:
            try:
                e = json.loads(line)
                t = e.get("event_type")
                if t == "decision.formed":
                    mid = e["payload"].get("market_id")
                    if mid:
                        decisions[mid] = e["payload"]
                elif t == "fill.received":
                    mid = e["payload"].get("market_id")
                    if mid:
                        fills.add(mid)
            except:
                pass

    eligible = {
        mid: payload
        for mid, payload in decisions.items()
        if mid not in fills
    }
    return eligible

def place_maker_order(market_id, side, price, size_usd, dry_run=True):
    """
    Coloca limit order en CLOB V2.
    dry_run=True por defecto — no ejecuta nada real.
    Cambiar a False solo después del 22 abril con capital real.
    """
    if dry_run:
        logger.info(
            f"[DRY RUN] Order: market={market_id[:16]} "
            f"side={side} price={price} size=${size_usd}"
        )
        return {"status": "dry_run", "market_id": market_id}

    # TODO: implementar con py-clob-client-v2 post-migración
    raise NotImplementedError(
        "Live orders activar solo después del 22 abril. "
        "Ver docs/LIVE_GATEWAY_SETUP.md"
    )

def run(dry_run=True):
    """Entry point del agente."""
    logger.info(f"Live Gateway iniciando — dry_run={dry_run}")

    if not dry_run:
        config = load_config()
        logger.info("Config cargada correctamente")

    eligible = get_eligible_markets()
    logger.info(f"Mercados elegibles: {len(eligible)}")

    for market_id, payload in eligible.items():
        logger.info(f"Procesando: {market_id[:16]}")
        place_maker_order(
            market_id=market_id,
            side="NO",
            price=payload.get("price", 0.90),
            size_usd=MAX_ORDER_SIZE_USD,
            dry_run=dry_run
        )

    logger.info("Live Gateway completado")

if __name__ == "__main__":
    logging.basicConfig(level=logging.INFO)
    run(dry_run=True)
