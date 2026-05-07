# ADR: Externally Derived Edge Candidates V1

## Status
Accepted

## Context
2EXCAMIM opera como sistema paper/shadow-first con runtime Rust event-sourced y agentes Python pequeños. Se evaluaron repos externos que contienen ideas útiles, pero también riesgos de scope creep, ejecución live y acoplamiento técnico excesivo.

## Decision

### 1) No importar bots externos wholesale
- Se permite derivar hipótesis, utilidades y patrones.
- No se permite integrar bots completos con su propia lógica de ejecución/riesgo como subsistema interno.
- Cada componente debe adaptarse al contrato de eventos de 2EXCAMIM.

### 2) Solo hipótesis y utilidades en V1
- V1 se limita a discovery, snapshot research y scoring candidato.
- No se habilita trading live, ni creación de órdenes reales, ni manejo de credenciales.

### 3) Candidate/shadow-first obligatorio
- Todo nuevo componente inicia en estado candidate/shadow/research-only.
- Promoción depende de scorecard y gobernanza (`candidate`/`promoted`/`frozen`/`rejected`).

### 4) Prioridad: oracle lag + research harness
- `gengar_polymarket_bot` se usa como insumo de hipótesis de oracle lag.
- `polyrec` se usa como referencia para harness de snapshots y backtests.
- Se prioriza porque aporta evidencia medible sin romper límites de seguridad operacional.

### 5) Cross-venue diferido
- Matching Kalshi/Polymarket queda en investigación opcional offline.
- Se difiere hasta validar fases internas de edge en Polymarket shadow/paper.

## Consequences

### Positivas
- Preserva límites de seguridad y alcance.
- Mantiene trazabilidad vía event-sourcing JSONL.
- Reduce riesgo de introducir ejecución live accidental.

### Negativas
- Menor velocidad de integración frente a copiar bots.
- Requiere trabajo adicional de adaptación contractual.

## Scope boundaries reaffirmed
- Sin `async` en Rust runtime.
- Sin base de datos nueva.
- Sin dependencia de red nueva en el core Rust.
- Sin ampliación de alcance sin confirmación explícita.
