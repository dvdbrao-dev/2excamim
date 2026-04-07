# Python -> Rust Handoff v1

## Objetivo

Definir una frontera minima y trazable entre el laboratorio Python de prediction markets y el runtime Rust de 2EXCAMIM.

Esta version no hace integracion live. Solo hace ingestión offline de un archivo de research ya generado.

## Input aceptado

Version de schema del handoff:

- `research-signals.v1`

Rust v1 acepta el Parquet local producido por:

- `research_prediction_markets/main.py`
- salida esperada: `research_prediction_markets/output/signals/latest_signals.parquet`

Columnas requeridas:

- `market_id`
- `timestamp`
- `signal_name`
- `strength`
- `direction`
- `source`

Columnas opcionales actualmente soportadas:

- `probability`
- `spread_tight`
- `volume_spike_24h`
- `price_deviation_vwap_1h`
- `metadata`

Contrato esperado:

- cada fila debe declarar `handoff_schema_version = research-signals.v1` una vez decodificada
- `timestamp` en RFC3339 UTC una vez decodificado desde Parquet
- `strength` en `[0,1]`
- `direction` en uno de:
  - `long_yes`
  - `long_no`
  - `short_yes`
  - `short_no`
- `metadata` debe ser un JSON object serializado como string cuando viene presente

## Traduccion a eventos Rust

Evento generado en v1:

- `signal.generated`

No se genera todavia:

- `hypothesis.generated`
- `signal.confirmed`
- `decision.formed`
- ningun evento de ejecucion

Mapeo principal:

- `instrument` = `{source}:{market_id}`
- `timeframe` = `research_snapshot`
- `signal_id` = identificador determinista derivado de `source + market_id + signal_name + timestamp + direction`
- `side`:
  - `long_yes` y `short_no` -> `Long`
  - `long_no` y `short_yes` -> `Short`

Conservacion de contexto research:

- `signal_name` no se convierte en tipo de evento distinto; se conserva en `rationale`
- `probability`, features y `metadata` se conservan serializados en `provenance.notes`
- `source_ref` apunta al fichero de ingestión y al numero de fila
- `producer_run_id` conserva el `batch_trace_id` del archivo ingerido
- `trace_id` queda ligado a batch y fila para auditoria posterior

## CLI

Uso:

```bash
cargo run -- ingest research-signals research_prediction_markets/output/signals/latest_signals.parquet --store ./var/events.jsonl
```

Dry run:

```bash
cargo run -- ingest research-signals research_prediction_markets/output/signals/latest_signals.parquet --store ./var/events.jsonl --dry-run
```

Salida:

- `handoff_schema_version`
- `input_path`
- `rows_read`
- `rows_valid`
- `rows_invalid`
- `events_written`
- `duplicates`
- `rejected_reasons`
- `dry_run`
- `batch_trace_id`
- detalle de señales ingeridas
- detalle de rechazos

JSON opcional:

```bash
cargo run -- ingest research-signals research_prediction_markets/output/signals/latest_signals.parquet --store ./var/events.jsonl --json
```

## Decodificacion de Parquet

La frontera v1 mantiene el output research en su formato real actual: Parquet.

Para evitar reescribir el laboratorio Python o introducir ingestion live, Rust usa un adaptador explicito:

- script: `research_prediction_markets/export_signals_json.py`
- responsabilidad: leer Parquet y emitir registros JSON linea a linea
- el decoder añade `handoff_schema_version` por fila

Rust conserva la validacion contractual, la traduccion a eventos y la persistencia en el store.

## Lo que no hace todavia

- scheduler
- polling
- watch de directorios
- integracion live Python -> Rust
- confirmacion automatica de señales
- generacion de decisiones
- ingestion de otras familias de artefactos research
- ingestion live en memoria Python -> Rust

## Deuda abierta

- sustituir el decoder Python por lectura nativa en Rust si compensa
- decidir si `signal_name` merece entidad o taxonomia propia en capas futuras
- decidir si un output posterior justifica `hypothesis.generated`
- decidir si el `batch_trace_id` debe pasar a ser content-addressed
