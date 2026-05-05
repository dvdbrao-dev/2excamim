# Maker Strategy Validation Protocol

## Hipótesis a validar

El sistema puede proveer liquidez en el lado NO de mercados binarios Polymarket
(midpoint NO >= 0.85) cobrando el spread, sin necesitar forecasting.

## Pre-condiciones para validar

1. Datos históricos: mercados cerrados con NO >= 0.80, volumen >= 50k USDC.
2. Modelo de fill conservador (ver `scripts/maker_fill_simulator.py`).
3. Thresholds pre-commiteados antes de ver resultados.

## Criterios pre-commiteados (NO se modifican después de ver datos)

| Criterio | Threshold | Fuente |
|----------|-----------|--------|
| Meses con edge neto > 0 | >= 60% | `scripts/maker_backtester.py:VALIDATION_MONTHLY_PASS_RATE` |
| T-stat bootstrap | >= 1.5 | `scripts/maker_backtester.py:VALIDATION_MIN_T_STAT` |
| Edge = expected_edge_per_fill - fees - adverse_selection | > 0 | por mes |

## Cómo ejecutar la validación

```bash
# Paso 1: ingestar histórico (solo una vez, slice congelado durante análisis)
python3 agents/services/polymarket_orderbook_history.py \
    --output-dir data/orderbook \
    --min-no-midpoint 0.80 \
    --min-volume 50000

# Paso 2: correr backtester
python3 scripts/maker_backtester.py \
    --data-dir data/orderbook \
    --quote-price 0.85 \
    --quote-size 50 \
    --output-json reports/maker_backtest_$(date +%Y%m%d).json
```

## Modelo de fill (conservador por diseño)

El simulador asume:
- Queue position pessimista: 20% de la liquidez al nivel de precio.
- Solo fill si volumen 1h > $500 y fill_probability > 5%.
- Adverse selection activa siempre que sigma_1h disponible.
- Sin fills si NO midpoint < quote_price (mercado movió en contra).
- Drift negativo de 0.1%/hora para simular deterioro de posición.

**Principio**: si el modelo conservador muestra edge → credibilidad alta. Si no → estrategia queda como hipótesis no probada.

## Interpretación de veredictos

| Veredicto | Significado | Acción |
|-----------|-------------|--------|
| PASS | Edge demostrado con datos conservadores | Autorización condicional para paper trading |
| INCONCLUSIVE | Un criterio pasa, otro no | Más datos; no escalar tamaño |
| FAIL | Ningún criterio pasa | Pivot maker queda como hipótesis. STRATEGY.md se actualiza |

## Invariantes del proceso

1. El slice de datos histórico NO se actualiza durante el análisis.
2. Los thresholds están en código, no en CLI args.
3. La validación es idempotente: mismos datos → mismo veredicto.
4. Si FAIL: `STRATEGY.md` se actualiza para reflejar que la hipótesis maker no está validada con datos disponibles.

## Estado actual

Validación pendiente de datos históricos suficientes (ejecutar paso 1 con red disponible).
