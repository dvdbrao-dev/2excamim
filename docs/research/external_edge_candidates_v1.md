# External Edge Candidates V1

## Objetivo
Definir un programa controlado para derivar edge desde repositorios externos sin importar bots completos ni abrir superficies live en 2EXCAMIM.

## Reglas de integración
- No importar repos externos wholesale.
- No crear trading live ni órdenes reales por defecto.
- No introducir claves privadas ni credenciales.
- Todo componente nuevo empieza como `candidate`/`shadow`/`research-only`.
- Rust conserva el boundary contractual de event-sourcing.

## Fuentes externas auditadas

### Alta prioridad
1. `JLowo/gengar_polymarket_bot`
- Uso permitido: hipótesis de oracle lag para mercados BTC 5m en Polymarket.
- Uso prohibido: copiar lógica de ejecución o cualquier camino de órdenes live.

2. `txbabaxyz/polyrec`
- Uso permitido: diseño de harness de snapshots oracle/spot/orderbook y estructura de backtests.
- Uso prohibido: importar framework completo sin adaptación contractual a 2EXCAMIM.

### Candidate / Utility
3. `handiko/Polymarket-Market-Finder`
- Uso permitido: discovery determinista de slots 5m/15m para mercados cripto.
- Uso prohibido: acoplar scraping o heurísticas sin trazabilidad de salida.

### Research only (deferred)
4. `KaustubhPatange/polymarket-trade-engine`
- Solo patrones de logging/state/simulation.

5. `RichardFeynmanEnthusiast/kalshi-polymarket-market-matching`
- Solo matching offline cross-venue a futuro.

6. `PaulieB14/polymarket-subgraph-analytics`
- Solo research futuro de liquidez/orderflow.

## Envelope contractual requerido
Todo output candidato que quiera entrar al event log debe emitirse como evento con:
- `event_type`
- `event_id`
- `timestamp`
- `idempotency_key`
- `aggregate_key`
- `provenance`
- `payload`

## Plan por fases
- Phase 0: event contract and docs.
- Phase 1: slot discovery candidate.
- Phase 2: research snapshot collector.
- Phase 3: oracle lag candidate scorer.
- Phase 4: shadow execution and scorecard.
- Phase 5: offline backtest harness.
- Phase 6: optional cross-venue matcher research.

## Entregables de esta iteración
- Documentación y ADR aprobando dirección de trabajo.
- Sin implementación de agentes ni cambios de ejecución.
