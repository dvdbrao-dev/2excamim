# Runtime Operations UX v1

## Objetivo

Hacer la CLI del runtime mas coherente para operacion manual y futura automatizacion, sin introducir un runner live.

## Verbos canónicos

- `summary`
- `inspect`
- `policy`
- `ingest`
- `materialize`
- `materialize orders`
- `submit orders`
- `run batch`

Formas principales:

```bash
cargo run -- summary
cargo run -- inspect signal sig-1
cargo run -- policy signal sig-1
cargo run -- ingest research-signals ./signals.parquet --dry-run
cargo run -- materialize decisions --dry-run
cargo run -- materialize orders --dry-run
cargo run -- submit orders --dry-run
cargo run -- run batch --research-signals ./signals.parquet --dry-run
```

## Alias heredados

Siguen aceptandose por compatibilidad:

- `signal <id>`
- `decision <id>`
- `order <id>`
- `fill <id>`

## Salida

- texto estructurado por defecto
- `--json` donde aplica a inspeccion, policy, ingest, materialization y batch run
- errores en JSON por `stderr` si el comando se invoca con `--json`

## Exit codes

- `0`: exito o `--help`
- `1`: error operativo/runtime
- `2`: error de uso/argumentos
- `3`: entidad no encontrada

## Limites

- no hay parser externo ni framework CLI dedicado
- no hay scheduler ni live runner
- no hay shell interactiva
- el batch runner actual solo encadena ingest, materialize y summary
