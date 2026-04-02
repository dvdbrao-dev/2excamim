# 2EXCAMIM Events v1

Crate Rust mínimo para el sistema de eventos v1 de 2EXCAMIM.

## Uso

```bash
cargo fmt
cargo check
cargo test
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
