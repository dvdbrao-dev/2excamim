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

## Event Envelope (external candidates)
- Implementación base: `agents/core/event_envelope.py`.
- Validación estricta de campos requeridos y formato (`UUID`, timestamp ISO-8601, payload/provenance objeto).
- Escritura JSONL de una línea por evento con append idempotente sobre `agents/core/event_store.py`.
- Tipos iniciales habilitados:
  - `external_repo.audit_recorded`
  - `market_slot.discovered`
  - `market_snapshot.observed`
  - `oracle_lag.observed`
  - `candidate_signal.scored`
  - `shadow_fill.simulated`
  - `strategy_round.scored`
  - `candidate_strategy.evaluated`
  - `data_gap.detected`
  - `feed_health.checked`

### Slot Discovery Candidate (Phase 1)
- Agente: `agents/market_slot_discovery_candidate.py`
- Rol: discovery determinista de slots esperados BTC/ETH/SOL en ventanas 5m/15m.
- Salida: eventos `market_slot.discovered` en `./var/events/external_candidates.jsonl` (por defecto).
- Operación opcional: `bash scripts/run_slot_discovery_candidate.sh` (no obligatorio en pipeline principal).

### Research Snapshot Collector Candidate (Phase 2)
- Agente: `agents/research_collector_candidate.py`
- Rol: recolectar snapshots research (`spot`, `oracle`, `orderbook`, health/gaps) para slots candidatos.
- Modo seguro: `--mock` / `--dry-run`, sin credenciales ni ejecución live.
- Operación opcional: `bash scripts/run_research_collector_candidate.sh` (no obligatorio en pipeline principal).

### Oracle Lag Signal Candidate (Phase 3)
- Agente: `agents/oracle_lag_signal_candidate.py`
- Rol: puntuar señales candidatas cuando `spot/oracle` parece adelantarse al ajuste de `book mid`.
- Filtros duros: stale feed, spread alto, snapshots insuficientes, banda de precio y proximidad a fin de slot.
- Salida: `oracle_lag.observed` + `candidate_signal.scored` en modo shadow-only no ejecutable.
- Operación opcional: `bash scripts/run_oracle_lag_signal_candidate.sh`.

### Shadow Execution Simulator (Phase 4)
- Agente: `agents/shadow_execution_simulator.py`
- Rol: convertir `candidate_signal.scored` en `shadow_fill.simulated` y `strategy_round.scored` sin órdenes reales.
- Modelo conservador: fee, slippage, latencia y probabilidad de fill explícita.
- Operación opcional: `bash scripts/run_shadow_execution_simulator.sh`.

### External Candidate Scorecard
- Agente: `agents/external_candidate_scorecard.py`
- Rol: consolidar métricas de `candidate_signal.scored`, `shadow_fill.simulated` y `strategy_round.scored`.
- Resultado: evento `candidate_strategy.evaluated` con `status` efectivo y `suggested_status`.
- Regla de seguridad: no auto-promoción por defecto (solo sugerencia de promoción).
- Operación opcional: `bash scripts/run_external_candidate_scorecard.sh`.

### Offline Backtest Harness (Phase 5)
- Agente: `agents/backtest_external_candidate.py`
- Rol: replay offline determinista de snapshots/candidatos para `oracle_lag_v1`.
- Salidas:
  - `var/events/external_backtest.jsonl`
  - `reports/external_candidates/oracle_lag_v1_backtest.md`
- Operación opcional: `bash scripts/run_external_candidate_backtest.sh`.

### External Candidate Reporting
- Agente: `agents/external_candidate_report.py`
- Rol: generar observabilidad Markdown por estrategia, asset/window y run para candidatos externos.
- Salida por defecto: `reports/external_candidates/external_candidate_report_<RUN_ID>.md`.
- Operación opcional: `bash scripts/generate_external_candidate_report.sh`.
- Documentación: `docs/reports/external_candidate_reports.md`.

## Arranque rápido
```bash
bash scripts/run_pipeline.sh
```

### Pipeline por defecto vs external candidates
- Por defecto, el pipeline principal no cambia: `EXTERNAL_EDGE_CANDIDATES_ENABLED=0`.
- Para habilitar la cadena externa en modo seguro/mock:

```bash
EXTERNAL_EDGE_CANDIDATES_ENABLED=1 EXTERNAL_EDGE_MOCK_MODE=1 bash scripts/run_pipeline.sh
```

- Para ejecutar solo la cadena externa (sin backtest):

```bash
EXTERNAL_EDGE_MOCK_MODE=1 bash scripts/run_external_edge_candidates.sh
```

- Para deshabilitar explícitamente los externos:

```bash
EXTERNAL_EDGE_CANDIDATES_ENABLED=0 bash scripts/run_pipeline.sh
```

Ver configuración detallada en `docs/config/external_edge_candidates.md`.

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
