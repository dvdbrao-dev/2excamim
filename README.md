# 2excamim

Sistema de trading en paper para mercados de predicción (Polymarket).
Event-sourcing en Rust + agentes Python. Operativo desde abril 2026.

## Estado actual
Pipeline completo corriendo 24/7 via systemd timer cada 5 minutos.
Estrategia activa: maker liquidity en NO-side (ver STRATEGY.md).
LLM blend desactivado. Universo ampliado a mercados con NO >= 0.85.

## Arquitectura del pipeline
market-watch → signal_agent → scoring_agent → confirmation_agent
→ probability_agent → veto_agent → sizing_agent → exit_agent

## Fuentes de señales

### Polymarket (activo)
- Estrategia: maker liquidity, mercados con midpoint NO >= 0.85
- Estrategia legacy opcional: `legacy_gap_to_half` (desactivada por defecto)
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
| signal_agent | Python | Genera `signal.generated` por umbrales extremos con `signal_type` explícito |
| scoring_agent | Python | Filtra mercados por gap >= 0.07 |
| confirmation_agent | Python | Confirma solo con >=2 checks independientes (strength + calidad de mercado) |
| probability_agent | Python | LLM advisory por defecto y salida utilizable solo con contexto suficiente |
| veto_agent | Python | Veta signals confirmadas con probabilidad fuera de rango configurable `[floor, ceiling]` |
| sizing_agent | Python | Kelly sizing sobre signals elegibles |
| exit_agent | Python | Tres triggers de salida: target, volumen, decay |
| STRATEGY.md | Doc | Hipótesis de edge activa y cambios aplicados |

## Stack
- Runtime: Rust (event-sourcing, JSONL store)
- Agentes: Python 3.12
- Datos: Polymarket Gamma API
- LLM: gpt-4o-mini (OpenAI) via MixMCP pattern
- Infra: Hetzner VPS Ubuntu 24.04, systemd timer

## External Edge Candidate Program (V1)
- Objetivo: convertir research externo en hipótesis internas, no importar bots completos.
- Prioridad: `gengar_polymarket_bot` (hipótesis de oracle lag) y `polyrec` (harness de snapshots/backtests).
- Utilidad candidata: `Polymarket-Market-Finder` para discovery determinista de slots 5m/15m.
- Restricciones: candidate/shadow-only, sin live trading por defecto, sin credenciales, sin órdenes reales.
- Contrato: todo componente nuevo debe emitir eventos JSONL con `event_type`, `event_id`, `timestamp`, `idempotency_key`, `aggregate_key`, `provenance`, `payload`.

## Arranque rápido
```bash
bash scripts/run_pipeline.sh
```

## Replay y medición mínima

### Replay/backtest mínimo (histórico de eventos)
```bash
python3 scripts/replay_backtest.py --store ./var/events.jsonl
python3 scripts/replay_backtest.py --store ./var/events.jsonl --json
```

Qué mide:
- reconstrucción mínima de señales/confirmaciones/vetoes/decisiones/fills desde JSONL
- `pnl_gross`, `pnl_net`, costes (`fees + slippage + penalización de baja liquidez`), `trades`, `win_rate`
- `exposure_mean` y `max_drawdown` (sobre PnL neto realizado)

Qué no mide todavía:
- mark-to-market completo de posiciones abiertas
- microestructura real de ejecución (solo modelo de coste configurable en bps)
- atribución causal perfecta cuando faltan `linkage` en eventos históricos

### Reporte operativo por agente/estrategia (UMN mínimo)
```bash
python3 scripts/umn_report.py --store ./var/events.jsonl
python3 scripts/umn_report.py --store ./var/events.jsonl --json
```

Qué entrega:
- tabla por agente y por `signal_type/strategy` con `pnl_net`, `trades`, `costs`, `risk_proxy_drawdown`
- métrica compuesta simple `umn_score` para comparación estable
- clasificación pragmática: `contributor`, `neutral`, `negative`, `insufficient_data`

## Variables de entorno requeridas
