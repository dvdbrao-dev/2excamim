# AGENT_CONTRACT

## Por qué existe este documento

El sistema tiene infraestructura completa para events, store, governance, projections y batch runner.
Sin embargo, todas las decisiones las toma un humano mediante CLI.

**Este documento define la interfaz mínima que cualquier agente debe implementar para participar en el sistema.**
No se introduce ninguna feature nueva de infraestructura hasta que al menos un agente esté operativo.

---

## Regla de bloqueo

> **`run batch` no materializa decisiones si ningún agente ha producido `signal.confirmed` en el store.**

Esto no requiere código nuevo. La governance ya exige `signal.confirmed` para que una señal sea `Eligible`.
El bloqueo es natural si no hay agente de confirmación activo.

---

## Interfaz mínima de un agente

Un agente es cualquier proceso que:

1. Lee el store JSONL en `./var/events.jsonl`
2. Evalúa el estado actual usando `cargo run -- inspect` o leyendo el JSONL directamente
3. Escribe exactamente un tipo de evento usando `cargo run -- [comando]`
4. Es idempotente: ejecutarlo dos veces no produce efectos distintos
5. Registra su identidad en `provenance.actor` y `provenance.producer_run_id`

No necesita ser Rust. Puede ser Python, bash, o cualquier proceso que llame a la CLI del runtime.

---

## Agentes requeridos por fase

### Fase actual: ACTIVA

| Agente | Evento que produce | Comando CLI | Estado |
|---|---|---|---|
| `confirmation-agent-v1` | `signal.confirmed` | `cargo run -- confirm signal` | ✅ ACTIVO |
| `probability-agent-v1` | `signal.confirmed` / `veto.raised` | `python3 agents/probability_agent.py` | ✅ ACTIVO |
| `veto-agent-v1` | `veto.raised` | `cargo run -- veto signal` | ✅ ACTIVO |
| `sizing-agent-v1` | `decision.formed` o `veto.raised` | `python3 agents/sizing_agent.py` | ✅ ACTIVO |

**Estos cinco agentes ya cubren el tramo mínimo de confirmación, probabilidad, veto, sizing y salida.**

### Fase siguiente (no empezar hasta resolver la anterior)

| Agente | Evento que produce | Estado |
|---|---|---|
| `exit-agent-v1` | `veto.raised` sobre decisiones activas | ✅ ACTIVO |

---

## Contrato mínimo de `confirmation-agent-v1`

Este es el primer agente. Debe existir antes de cualquier otra feature nueva.

### Input
```
./var/events.jsonl  — store local actual
```

### Lógica mínima aceptable (puede ser esta, literalmente)
```python
for signal in pending_unconfirmed_signals:
    if signal.strength >= 0.6:
        confirm(signal_id=signal.signal_id, confirmed_by="confirmation-agent-v1")
```

### Output
Evento `signal.confirmed` escrito en el store vía CLI:
```bash
cargo run -- confirm signal {signal_id} \
  --confirmed-by confirmation-agent-v1 \
  --store ./var/events.jsonl
```

### Provenance obligatoria
```json
{
  "source_kind": "Runtime",
  "actor": "confirmation-agent-v1",
  "producer_run_id": "{run_id_unico_por_ejecucion}"
}
```

### Criterio de aceptación
- Ejecutar el agente → `run batch --dry-run` muestra al menos una decisión `Eligible`
- Ejecutar el agente dos veces → mismo número de eventos (idempotencia)
- `cargo run -- inspect signal {id}` muestra `status: Eligible` en governance

---

## Registro de agentes activos

Actualizar esta tabla cuando un agente pase a producción:

| Agente | Versión | Lenguaje | Ruta | Estado | Fecha |
|---|---|---|---|---|---|
| confirmation-agent-v1 | v1 | Python | agents/confirmation_agent.py | ✅ ACTIVO | 2026-04-08 |
| probability-agent-v1 | v1 | Python | agents/probability_agent.py | ✅ ACTIVO | 2026-04-16 |
| veto-agent-v1 | v1 | Python | agents/veto_agent.py | ✅ ACTIVO | 2026-04-15 |
| sizing-agent-v1 | v1 | Python | agents/sizing_agent.py | ✅ ACTIVO | 2026-04-15 |
| exit-agent-v1 | v1 | Python | agents/exit_agent.py | ✅ ACTIVO | 2026-04-15 |

**Si esta tabla está vacía, el sistema no tiene agentes.**

---

## Regla para sesiones de desarrollo

Antes de añadir cualquier feature nueva de infraestructura, responde esta pregunta:

> ¿Qué agente concreto usará esta feature y cuándo?

Si la respuesta es "en el futuro" o "cuando esté todo listo", la feature no se implementa todavía.
