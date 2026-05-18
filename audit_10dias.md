# AUDITORÍA EXCAMIM — 10 DÍAS
Fecha: 2026-05-18T11:30:00Z

## 1. VEREDICTO GENERAL

El sistema estuvo **PARCIALMENTE ACTIVO**: el market-watch corre cada 5 min y el dashboard está levantado, pero el pipeline de trading **no ejecutó entre 2026-04-17 y 2026-05-17 (~31 días de gap)**. Reactividad confirmada solo el 2026-04-16 (pruebas) y 2026-05-18 (hoy). No se cerraron fills en el ledger paper en ningún momento.

---

## 2. ACTIVIDAD REAL

| Métrica | Valor |
|---|---|
| Eventos en `events.jsonl` | 305 |
| Días con actividad de pipeline | 2 (2026-04-16 y 2026-05-18) |
| Gap sin pipeline | 2026-04-17 → 2026-05-17 (31 días) |
| Snapshots market-watch | ~3.000 (todos de 2026-05-18) |
| Trades paper ejecutados (fills) | 0 |
| Último evento | `signal.generated` @ 2026-05-18T11:16Z |
| Frecuencia de ciclos market-watch | Cada 5 min (timer activo) |
| Frecuencia del pipeline de señales | No automatizado — corre solo al lanzar `run_pipeline.sh` |

**Breakdown de eventos (305 total):**

| Tipo | Count |
|---|---|
| `signal.generated` | 105 |
| `market.scored` | 99 |
| `signal.confirmed` | 86 |
| `decision.formed` | 8 |
| `veto.raised` | 4 |

---

## 3. CONFIRMATION AGENT

**Estado: IMPLEMENTADO y funcional**

Evidencia:
- Checkpoint activo: `var/.checkpoints/confirmation-agent-v1-413018d2c31b.json` (offset 495.499 bytes)
- 86 eventos `signal.confirmed` producidos por `confirmation-agent-v1`
- También presentes: `probability-agent-v1`, `shadow-live-agent-v1`, `sizing-agent-v1`

---

## 4. BUGS CONOCIDOS — ¿SIGUEN PRESENTES?

### Kelly vetando NO-side bets: **SÍ — CONFIRMADO**

Evidencia directa:

| Side | Señales generadas (unique) | Confirmadas |
|---|---|---|
| `long_yes` | 72 | 48 (67%) |
| `long_no` | 35 | **0 (0%)** |

Root cause identificado: la **fuerza de señal** (`strength`) para `long_no` es estructuralmente baja. Para mercados con `midpoint ≈ 0.6`, el gap de 0.5 es solo 0.105 → `strength ≈ 0.21`, muy por debajo del `DEFAULT_THRESHOLD = 0.6` del confirmation-agent. Los mercados YES extremos (midpoint 0.13–0.16) tienen gap ~0.36 → `strength ≈ 0.72`.

Esto no es un bug de Kelly: Kelly maneja correctamente `long_no` (invierte probabilidades en `sizing_agent.py:367`). El problema está en la generación de señales: la función de strength favorece sistemáticamente YES bets sobre mercados con precio muy bajo.

### LLM probability blend degradando señal: **PRESENTE pero benigno**

- `ALPHA = 0.0` → `p_final = midpoint` (LLM completamente ignorado en el blend)
- 4 vetos `probability_out_of_bounds` para señales England `long_yes` donde `p_final ∈ [0.074, 0.092]`, fuera del rango `[0.10, 0.90]`
- El blend no degrada señal porque ALPHA=0.0, pero tampoco aporta información del LLM

### O(n) JSONL reads por ciclo: **MITIGADO**

- `read_jsonl_since()` en `agents/core/event_store.py:97` usa `handle.seek(safe_offset)` con byte offset guardado en checkpoint
- Complejidad es O(bytes_nuevos), no O(total_eventos)
- Con 305 eventos y ~85 KB el impacto es mínimo de todas formas

---

## 5. PERFORMANCE PAPER TRADING

| Métrica | Valor |
|---|---|
| Total decisions formadas | 8 |
| Fills persistidos en ledger | **0** |
| PnL neto paper | **$0.00** |
| Open positions | 0 |
| Closed positions | 0 |
| Distribución YES vs NO | 100% YES / 0% NO |
| Rango de precios operados (YES) | midpoint 0.124–0.172 (mercados ~85–87% probabilidad NO) |
| Kelly fraction usada | ~50 USDC / 1000 USDC bankroll (5%) |
| UMN promedio | no calculado (0 fills) |

Las 8 decisions formadas NO generaron órdenes en el ledger. El `latest_pipeline.json` confirma: `signals_seen=0, execution_requests_sent=0, fills_persisted=0`. El pipeline corrió en modo dry-run implícito o la ejecución de órdenes no está cableada al ledger paper.

---

## 6. ESTADO DE AGENTES

| Agente | Estado | Checkpoint | Última evaluación |
|---|---|---|---|
| `confirmation-agent-v1` | Activo | offset 495KB | 2026-05-18 |
| `probability-agent-v1` | Activo | presente | 2026-05-18 |
| `shadow-live-agent-v1` | Activo | presente | 2026-05-18 |
| `sizing-agent-v1` | Activo | presente | 2026-05-18 |
| `crypto_adx_ema_pullback_v1` | `candidate` | Registry activo | 2026-05-18T10:44Z |
| `crypto_volatility_breakout_v1` | `candidate` | Registry heredado | (Apr 28, sin evaluar) |
| `excamim-dashboard` | Running | — | Apr 28 (2 semanas sin restart) |
| `market-watch.timer` | Waiting (cada 5 min) | — | activo |
| `excamim-daily.timer` | Waiting | — | FALLÓ hoy 08:00 |

---

## 7. ERRORES Y WARNINGS CRÍTICOS

1. **`excamim-daily.service` FALLA diariamente a las 08:00** (exit code 1, ~1 segundo de ejecución). El script corre OK a mano. Probable causa: `telegram_agent.py` falla con credenciales vacías en el contexto systemd (no hay `EnvironmentFile` en el `.service`).

2. **4× `veto.raised` probability_out_of_bounds** — todos para England `long_yes` en 2026-04-16, señales repetidas del mismo market_id. El probability-agent veta correctamente señales donde `p_final < 0.10`.

3. **Gap de 31 días sin pipeline** (2026-04-17 → 2026-05-17). El pipeline no tiene scheduling automático; solo corre con `run_pipeline.sh` manual.

4. **`latest_pipeline.json` muestra `signals_seen=0`** — el stage `load_existing_signals` no ve señales en el contexto del pipeline principal. Las señales de hoy aparecen en `events.jsonl` pero no llegan al resumen del pipeline.

5. **`snapshots.jsonl` no tiene histórico** — solo datos de 2026-05-18. El archivo fue vaciado o rotado. No hay rollup diario del market-watch histórico.

---

## 8. CAMBIOS EN CÓDIGO (últimos 10 días)

Solo 2 commits en los últimos 10 días:

```
ce13325 audit: 10 días paper + fixes preventivos (cron tmp, journald 200M)
ff51618 audit: 10 días paper + fixes preventivos (cron tmp, journald 200M)
```

Fixes aplicados (incluidos en estos commits):
- Cron cada 6h para limpiar `.tmp` huérfanos en `var/market-watch`
- Límite de journald a 200M (prevención de llenado de disco)

Desarrollo reciente (pre-ventana, en los últimos 20 commits):
- +7.432 líneas, 63 ficheros: agentes `shadow_live`, `oracle_lag_signal_candidate`, `research_collector_candidate`, `market_slot_discovery_candidate`, `external_candidate_backtest`, `cross_venue_matcher_candidate`
- Infraestructura: `drawdown_guard`, `shadow_execution_simulator`, `maker_backtester`, `maker_fill_simulator`
- 10 nuevos ficheros de tests
- Docs completos: `ARCHITECTURE.md`, `CHANGELOG.md`, `EVENT_CATALOG.md`, `ROADMAP.md`

---

## 9. DIAGNÓSTICO FINAL

- **El pipeline NO tiene scheduling automático.** El `market-watch.timer` recoge snapshots cada 5 min, pero el pipeline de señales+confirmación+sizing solo corre cuando se lanza `run_pipeline.sh` manualmente. En los últimos 10 días solo se ejecutó hoy.

- **0 fills en 10 días.** Las 8 decisions formadas (Kelly aprobado, 50 USDC) no llegaron a órdenes ejecutadas en el ledger. La capa de ejecución paper (`shadow_live_agent`) no está conectada al loop principal.

- **Bug NO-side bloquea el 100% de long_no signals.** La función de strength en `signal_agent.py` produce ~0.21 para NO bets en mercados de 60% implícito vs threshold 0.6. El sistema opera efectivamente como si solo hubiera YES bets.

- **`excamim-daily` falla en producción** por ausencia de env vars en contexto systemd. Sin daily reports funcionales, el monitoreo activo es ciego.

- **Mucho código nuevo bien estructurado** (shadow live, oracle lag, external candidates, tests) pero **sin integración** al pipeline principal. Los agentes candidatos llevan semanas en estado `candidate` sin evaluación real de fills.

---

## 10. PRÓXIMOS PASOS RECOMENDADOS

1. **[CRÍTICO] Automatizar el pipeline** — añadir timer systemd o crontab `*/30 * * * * cd /root/2excamim && bash scripts/run_pipeline.sh >> var/pipeline.log 2>&1`. Sin esto el sistema no opera autónomamente.

2. **[ALTO] Fix bug NO-side strength** — en `signal_agent.py:219`, normalizar la strength para `long_no` usando `p_no = 1 - midpoint` y calcular el gap desde la perspectiva NO. Alternativamente bajar el confirmation threshold para señales NO a ~0.15 (rango real observado). Esto desbloqueará el 35% de señales actualmente descartadas.

3. **[ALTO] Fix `excamim-daily` env vars** — añadir `EnvironmentFile=/root/2excamim/.env` al `[Service]` de `excamim-daily.service` para que las credenciales Telegram estén disponibles en el contexto systemd. Actualmente cada daily report falla silenciosamente.
