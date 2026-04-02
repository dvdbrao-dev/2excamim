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
