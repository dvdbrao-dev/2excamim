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
