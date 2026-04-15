
## ⚠️ PRÓXIMO SLICE OBLIGATORIO: Confirmation Agent v1

**Estado: NO INICIADO — BLOQUEANTE**

Este slice es el prerequisito de todo lo que sigue.
No se implementa ningún slice de infraestructura nuevo hasta que este esté completo.

### Qué hay que hacer

Crear `agents/confirmation_agent.py` (o `.sh`, o `.rs`) que:

1. Lea señales pendientes de confirmación desde `./var/events.jsonl`
2. Aplique criterio mínimo: `strength >= 0.6` → confirmar
3. Llame a la CLI para escribir `signal.confirmed`
4. Sea idempotente

### Criterio de done

```bash
python agents/confirmation_agent.py
cargo run -- run batch --research-signals research_prediction_markets/output/signals/latest_signals.parquet --store ./var/events.jsonl --dry-run
```

El batch debe mostrar al menos una decisión `Eligible`. Si no, el agente no funciona.

### Por qué es bloqueante

Sin `signal.confirmed` en el store, governance no marca ninguna señal como `Eligible`.
Sin señales `Eligible`, `materialize decisions` no materializa nada.
El sistema está completo pero paralizado. Este agente es el que lo desbloquea.

Ver contrato completo en `AGENT_CONTRACT.md`.

---

## Slices de infraestructura pendientes (congelados hasta Confirmation Agent v1)

Los siguientes slices están identificados pero **congelados** hasta que el agente de confirmación esté operativo:

- Veto Agent v1
- Sizing Agent v1
- Scheduler / daemon
- Gateway real
- Live ingestion
