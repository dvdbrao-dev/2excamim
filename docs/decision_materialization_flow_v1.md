# Decision Materialization Flow v1

## Objetivo

Evaluar el log actual y decidir que `signal` puede materializarse como `decision.formed` sin introducir orquestacion live.

## Entrada

- store JSONL actual
- signal query layer ya existente
- readiness, governance y promotion policy ya existentes

## Comando

```bash
cargo run -- materialize decisions --store ./var/events.jsonl --dry-run
```

Persistencia:

```bash
cargo run -- materialize decisions --store ./var/events.jsonl
```

JSON opcional:

```bash
cargo run -- materialize decisions --store ./var/events.jsonl --dry-run --json
```

## Regla de decision en v1

Una `signal` se considera materializable solo cuando:

- existe en projections
- `signal_promotion_policy` devuelve `Eligible`
- `next_step` es `FormDecision`
- no tiene ya `decision_ids` downstream en la projection

Clasificacion de salida:

- `Eligible`: materializable en `dry-run`
- `Materialized`: persistida como `decision.formed`
- `Skipped`: debil, congelada o ya materializada
- `Blocked`: vetada o bloqueada por semantica upstream
- `Inconsistent`: contradicciones visibles en el log

## Persistencia v1

Sin `--dry-run`, el flow persiste `decision.formed` solo para casos claramente elegibles.

Semantica usada:

- `decision_id` determinista: `decision-{signal_id}`
- `action`: `Enter`
- `side`: heredado de `signal.generated`
- `instrument`: heredado de `signal.generated`
- `parent_event_id`: `signal.generated` mas reciente del timeline de la señal
- `correlation_id`: heredado de la señal si existe

Provenance:

- `produced_by`: `runtime.decision_materialization`
- `actor`: `decision_materialization_flow_v1`
- `producer_run_id`: batch trace del run

## Lo que no hace todavia

- materializacion automatica de orders
- ejecucion real
- gateway
- resolver
- scheduler
- orquestacion permanente

## Limites

- el flow no fuerza write-time global policy
- la decision materializada sigue siendo minima y prudente
- no introduce scorecards ni ranking avanzado entre señales
