# Shadow Live & Drawdown Circuit Breaker

## Por qué existe

El PnL paper miente: ejecuta stops y takes al precio exacto sin slippage, sin
queue, sin partial fills. Shadow live corre en paralelo simulando fills
realistas y cuantifica el "impuesto del optimismo del paper".

El drawdown guard es autónomo: si el equity cae más allá de los thresholds
pre-commiteados, toca el kill switch sin intervención humana.

## Arquitectura

```
decision.formed
    └─► shadow_live_agent.py → shadow.fill.received (aggregate_key: shadow:...)
                                      │
fill.received (paper/live)            │
    │                                 │
    └──────────────────┬──────────────┘
                       │
                equity_curve.py (scope=paper | shadow)
                       │
                drawdown_guard.py
                  ├─ rolling_drawdown(7d) → kill switch if > threshold
                  └─ rolling_drawdown(30d) → permanent kill if > threshold
```

## Thresholds pre-commiteados (config/risk_limits.yaml)

| Trigger | Threshold | Acción |
|---------|-----------|--------|
| Shadow DD 7d | >= 5% | Kill switch temporal |
| Paper DD 7d | >= 8% | Kill switch temporal |
| DD 30d (paper o shadow) | >= 15% | Kill switch permanente |

## Kill switch

- Temporal: `var/.kill_switch` — eliminar manualmente para reanudar pipeline
- Permanente: `var/.kill_switch.permanent` — requiere:
  1. Commit documentando causa raíz
  2. Reset manual del archivo
  3. Timestamp del incidente en `runtime/kill_switch_reason.json`

## Shadow live: cómo funciona

1. Lee `decision.formed` aún no shadow-procesados
2. Para cada decisión, aplica slippage adverso al snapshot del mercado:
   - Polymarket: midpoint + 30bps (config `polymarket_slippage_bps`)
   - Crypto: midpoint + 5bps (config `crypto_slippage_bps`)
3. Emite `shadow.fill.received` con `aggregate_key: shadow:<original>`
4. Idempotente por `decision_id`

## Reporte semanal

```bash
python3 scripts/shadow_vs_paper_report.py --output-dir reports
```

Genera `reports/shadow_vs_paper_YYYY-WNN.md`. Si divergencia > 30% del paper
PnL, emite warning: el modelo de fills paper necesita más slippage.

## Invariantes

- Shadow live no toca dinero real ni envía orders reales.
- Downstream agents (scorecard, governance) pueden filtrar por `aggregate_key.startswith("shadow:")`.
- Permanent kill solo se levanta con PR humano + evidencia de causa raíz.
