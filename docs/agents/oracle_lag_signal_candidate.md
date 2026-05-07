# Oracle Lag Signal Candidate Agent

## Hypothesis
Cuando el spot/oracle se mueve más rápido que el ajuste del precio de mercado (mid del libro), puede existir una ventana corta de desalineación explotable.

## Why It May Be Real
- feeds externos (spot/oracle) pueden reaccionar antes que algunos mercados de predicción
- libros con baja profundidad ajustan más lento
- ventanas cortas (5m/15m) pueden exponer fricciones microestructurales

## Why It May Disappear
- arbitraje competitivo
- mejoras del market making
- cambios en liquidez/fees/spread
- degradación de calidad de datos

## No-Live Guarantee
- no envía órdenes
- no usa claves
- no ejecuta en venues
- emite solo `oracle_lag.observed` y `candidate_signal.scored`
- payload marcado como `governance_state=candidate`, `promoted=false`, `executable=false`

## Minimum Evidence Before Promotion
- estabilidad del edge en múltiples slots/activos
- scorecard con drawdown controlado
- sensibilidad robusta a spread/costos
- consistencia out-of-sample
- revisión humana explícita
