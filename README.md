# 2excamim

Sistema de trading en paper para mercados de predicción (Polymarket).
Event-sourcing en Rust + agentes Python. Operativo desde abril 2026.

## Estado actual
Pipeline completo corriendo 24/7 via systemd timer cada 5 minutos.

## Arquitectura del pipeline
market-watch → signal_agent → scoring_agent → confirmation_agent
→ probability_agent → veto_agent → sizing_agent → exit_agent

## Fuentes de señales

### Polymarket (activo)
- Estrategia: VWAP reversion + MixMCP LLM
- Mercados: 100 activos filtrados

### Binance Price Feed (activo)
- Estrategia: momentum 15min → mercados cripto Polymarket
- Símbolos: BTC/USDT, ETH/USDT, SOL/USDT
- Umbral: ±2% en 15 minutos
- Coste: $0 (API pública)

## Agentes activos

| Agente | Lenguaje | Función |
|---|---|---|
| market-watch | Rust | Descarga snapshots de gamma-api.polymarket.com |
| signal_agent | Python | Genera signals por gap de precio vs 0.5 |
| scoring_agent | Python | Filtra mercados por gap >= 0.07 |
| confirmation_agent | Python | Confirma signals con strength >= 0.6 |
| probability_agent | Python | Estima probabilidad via gpt-4o-mini + MixMCP |
| veto_agent | Python | Veta signals con probabilidad fuera de [0.10, 0.90] |
| sizing_agent | Python | Kelly sizing sobre signals elegibles |
| exit_agent | Python | Tres triggers de salida: target, volumen, decay |

## Stack
- Runtime: Rust (event-sourcing, JSONL store)
- Agentes: Python 3.12
- Datos: Polymarket Gamma API
- LLM: gpt-4o-mini (OpenAI) via MixMCP pattern
- Infra: Hetzner VPS Ubuntu 24.04, systemd timer

## Arranque rápido
```bash
bash scripts/run_pipeline.sh
```

## Variables de entorno requeridas
