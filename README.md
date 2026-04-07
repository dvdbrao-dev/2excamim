# 2EXCAMIM Events v1

Crate Rust mínimo para el sistema de eventos v1 de 2EXCAMIM.

## Uso

```bash
cargo fmt
cargo check
cargo test
cargo run -- summary --store ./var/events.jsonl
cargo run -- signal sig-1 --store ./var/events.jsonl
cargo run -- decision dec-1 --store ./var/events.jsonl --json
```

Los eventos viven en `src/events/` y cada constructor `EventEnvelope::new_*`:

- genera `event_id` con UUID
- fija `schema_version = "v1"`
- fija `occurred_at = Utc::now()`
- calcula `idempotency_key`
- valida antes de devolver el evento

## Store local JSONL

El crate expone un store local append-only en `src/store/`:

```rust
use twoexcamim::events::{EventEnvelope, Linkage, Provenance, SignalGenerated, SignalSide, SourceKind};
use twoexcamim::store::{JsonlEventStore, StoredEvent};

let envelope = EventEnvelope::new_signal_generated(
    "signal-engine",
    Some("BTCUSDT".into()),
    Linkage::default(),
    Provenance {
        source_kind: SourceKind::Derived,
        source_ref: None,
        producer_run_id: Some("run-1".into()),
        actor: None,
        trace_id: Some("trace-1".into()),
        notes: None,
    },
    SignalGenerated {
        signal_id: "sig-1".into(),
        hypothesis_id: None,
        instrument: "BTCUSDT".into(),
        timeframe: "1h".into(),
        side: SignalSide::Long,
        strength: 0.8,
        rationale: None,
    },
)?
;

let stored = StoredEvent::try_from(envelope)?;
let store = JsonlEventStore::new("./var/events.jsonl")?;

store.append_event(&stored)?;
let all_events = store.read_all()?;
let replay = store.replay()?;
```

Características mínimas:

- formato JSONL append-only, una línea JSON por evento
- crea el fichero si no existe
- deduplica por `idempotency_key`
- mantiene orden de escritura
- soporta filtros simples en memoria por `event_type`, `correlation_id`, `signal_id` y `decision_id`

## Codecs v1

El crate expone rehidratación tipada desde `StoredEvent` en `src/codecs/`:

```rust
use twoexcamim::codecs::RehydratedEvent;
use twoexcamim::store::StoredEvent;

let stored: StoredEvent = /* leído del JSONL store */;
let typed = RehydratedEvent::try_from(stored)?;

match typed {
    RehydratedEvent::SignalGenerated(event) => {
        assert_eq!(event.event_type.as_str(), "signal.generated");
        assert_eq!(event.payload.signal_id, "sig-1");
    }
    _ => {}
}
```

La rehidratación:

- decide el payload por `event_type`
- deserializa `payload: serde_json::Value` al tipo concreto
- reconstruye `EventEnvelope<T>`
- vuelve a validar invariantes y payload

## Projections v1

El crate expone projections mínimas en `src/projections/` construidas desde `&[StoredEvent]`:

```rust
use twoexcamim::projections::{
    build_decision_projections, build_signal_projections, timeline_for_correlation_id,
};
use twoexcamim::store::StoredEvent;

let events: Vec<StoredEvent> = /* replay del store */;

let signal_views = build_signal_projections(&events)?;
let decision_views = build_decision_projections(&events)?;
let correlation_timeline = timeline_for_correlation_id(&events, "corr-1");
```

Incluye:

- timeline lineal sin reordenación adicional
- filtro por `correlation_id`, `signal_id` y `decision_id`
- `SignalProjection` mínima derivada de `signal.generated`, `signal.confirmed`, `veto.raised` y `decision.formed`
- `DecisionProjection` mínima derivada de `decision.formed`, `veto.raised` y `fill.received`

Las projections usan rehidratación tipada cuando hace falta y omiten eventos que no aportan al estado derivado.

## Query layer v1

El crate expone una capa de consultas mínima en `src/queries/` sobre `JsonlEventStore` + projections existentes:

```rust
use twoexcamim::queries::QueryService;
use twoexcamim::store::JsonlEventStore;

let store = JsonlEventStore::new("./var/events.jsonl")?;
let queries = QueryService::new(&store);

let all_events = queries.all_events()?;
let signal = queries.signal_projection("sig-1")?;
let decision = queries.decision_projection("dec-1")?;
let signal_timeline = queries.timeline_for_signal("sig-1")?;
let correlation_timeline = queries.timeline_for_correlation("corr-1")?;
let confirmed = queries.confirmed_signals()?;
let with_fills = queries.decisions_with_fills()?;
```

Mantiene alcance pequeño:

- lee desde el store local JSONL existente
- reconstruye projections en memoria cuando hace falta
- no añade índices persistentes
- no añade caché
- no introduce red, async ni integración runtime

## Scenario Fixtures + Replay Harness v1

El crate expone fixtures en Rust puro y un harness mínimo en `src/scenarios/` para ejecutar escenarios end-to-end sobre store + codecs + projections + queries:

```rust
use twoexcamim::scenarios::{load_fixture_named, ReplayHarness};
use twoexcamim::store::JsonlEventStore;

let fixture = load_fixture_named("confirmed_then_filled_signal")?;
let store = JsonlEventStore::new("./var/scenario-events.jsonl")?;
let harness = ReplayHarness::new(&store);

let result = harness.run_fixture(&fixture)?;

assert_eq!(result.total_events, 4);
assert_eq!(result.confirmed_signals_count, 1);
assert_eq!(result.decisions_with_fills_count, 1);
```

Incluye fixtures mínimas:

- `confirmed_then_filled_signal`
- `vetoed_signal_without_fill`
- `decision_with_multiple_fills`

El harness:

- escribe eventos manteniendo su orden
- reutiliza la deduplicación por `idempotency_key` del store
- permite reejecutar la misma fixture sobre el mismo store sin duplicar eventos

## Observability Summary v1

El crate expone un resumen operativo mínimo en `src/observability/` derivado del store local y de las projections existentes:

```rust
use twoexcamim::observability::summary_from_store;
use twoexcamim::store::JsonlEventStore;

let store = JsonlEventStore::new("./var/events.jsonl")?;
let summary = summary_from_store(&store)?;

println!("{summary:?}");
```

El summary mantiene alcance pequeño:

- reutiliza `build_signal_projections` y `build_decision_projections`
- cuenta `fill.received`, quantity total y correlation IDs únicos
- agrega conteos por tipo de evento con `EventType::as_str()`
- no añade bus, red, runtime, base de datos ni async

## Application Services v1

El crate expone una capa mínima de servicios en `src/application/` para orquestar operaciones típicas sobre store + queries + observability + scenarios:

```rust
use twoexcamim::application::EventAppService;
use twoexcamim::store::JsonlEventStore;

let store = JsonlEventStore::new("./var/events.jsonl")?;
let app = EventAppService::new(&store);

let summary = app.current_summary()?;
println!("{summary:?}");
```

La capa application mantiene alcance pequeno:

- persiste `StoredEvent` o `EventEnvelope<T>` sobre el JSONL store existente
- expone projections y summary sin duplicar lógica

## Runtime Skeleton v1

El crate ya expone un runtime ejecutable minimo para inspeccion y evaluacion del estado actual del sistema:

```bash
cargo run -- [summary] --store ./var/events.jsonl
cargo run -- signal sig-1 --store ./var/events.jsonl
cargo run -- decision dec-1 --store ./var/events.jsonl
cargo run -- order ord-1 --store ./var/events.jsonl
cargo run -- fill fill-1 --store ./var/events.jsonl
cargo run -- signal sig-1 --store ./var/events.jsonl --json
```

Caracteristicas del runtime skeleton:

- binario CLI real con entrypoint en `src/main.rs`
- reutiliza `JsonlEventStore`, `QueryService` y capas semanticas existentes
- inspecciona `signal`, `decision`, `order` y `fill`
- expone readiness, governance, promotion policy, lineage y execution boundary segun aplique
- usa solo el log/store JSONL actual
- no introduce red, scheduler, resolver, gateway ni ejecucion live

Pendiente mayor despues de este slice:

- traduccion del output Python de research a eventos Rust del sistema
- integracion Python -> Rust sobre una interfaz estructurada y estable

## Python -> Rust Handoff v1

El runtime ya puede ingerir offline el output real del laboratorio Python y traducirlo a eventos del sistema:

```bash
cargo run -- ingest research-signals research_prediction_markets/output/signals/latest_signals.parquet --store ./var/events.jsonl
```

Dry run:

```bash
cargo run -- ingest research-signals research_prediction_markets/output/signals/latest_signals.parquet --store ./var/events.jsonl --dry-run
```

JSON opcional:

```bash
cargo run -- ingest research-signals research_prediction_markets/output/signals/latest_signals.parquet --store ./var/events.jsonl --json
```

Alcance de esta version:

- lee el Parquet actual del lab Python
- valida shape minimo por fila
- traduce a `signal.generated`
- persiste en el JSONL store existente
- soporta `--dry-run`
- reporta `rows_read`, `rows_valid`, `rows_invalid`, `events_written`, `duplicates`, `rejected_reasons` y `batch_trace_id`

No hace todavia:

- integracion live
- scheduler
- gateway
- resolver
- otras familias de eventos research

Contrato y decisiones:

- [`docs/python_rust_handoff_v1.md`](/root/2excamim/docs/python_rust_handoff_v1.md)
- [`docs/ADR-008-python-rust-handoff-v1.md`](/root/2excamim/docs/ADR-008-python-rust-handoff-v1.md)

## Decision Materialization Flow v1

El runtime ya puede evaluar señales del log y decidir si pueden materializarse como `decision.formed`:

```bash
cargo run -- materialize decisions --store ./var/events.jsonl --dry-run
```

Persistencia prudente:

```bash
cargo run -- materialize decisions --store ./var/events.jsonl
```

El flow:

- inspecciona las señales visibles en projections
- reutiliza readiness, governance y signal promotion policy
- marca cada señal como `Eligible`, `Skipped`, `Blocked` o `Inconsistent`
- persiste `decision.formed` solo para casos claramente elegibles cuando no se usa `--dry-run`

Contrato:

- [`docs/decision_materialization_flow_v1.md`](/root/2excamim/docs/decision_materialization_flow_v1.md)

## Runtime Operations UX v1

La CLI ya expone verbos operativos mas coherentes:

```bash
cargo run -- summary
cargo run -- inspect signal sig-1
cargo run -- policy signal sig-1
cargo run -- ingest research-signals research_prediction_markets/output/signals/latest_signals.parquet --dry-run
cargo run -- materialize decisions --dry-run
```

Notas operativas:

- `--help` devuelve salida util y exit code `0`
- `--json` produce salida estructurada y tambien errores JSON por `stderr`
- exit codes:
  - `0` exito
  - `1` error runtime
  - `2` uso invalido
  - `3` entidad no encontrada

Detalle:

- [`docs/runtime_operations_ux_v1.md`](/root/2excamim/docs/runtime_operations_ux_v1.md)
- permite reejecutar fixtures conocidas por nombre
- no introduce bus, red, runtime, base de datos ni async

## Prediction Markets Research (Python Lab)

2EXCAMIM incluye un laboratorio cuantitativo en Python ubicado en:

`research_prediction_markets/`

Propósito:

- explorar señales experimentales para prediction markets
- trabajar en modo offline y paper-first
- producir outputs estructurados para consumo posterior por Rust

Flujo:

1. ingestión de Kalshi y Polymarket
2. cálculo de features MVP
3. generación de señales MVP
4. export a Parquet

Output actual:

`research_prediction_markets/output/signals/latest_signals.parquet`

Restricciones:

- no ejecuta trades
- no toma decisiones finales
- no sustituye el runtime Rust
- no contiene lógica de riesgo ni orquestación

### Ejecución mínima

```bash
cd research_prediction_markets
python3 -m venv .venv
source .venv/bin/activate
pip install requests pandas pyarrow
python main.py
```
