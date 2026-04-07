# ADR-008: Python Rust Handoff v1

## Estado

Aprobado

## Contexto

El laboratorio Python ya produce un artefacto real y util: `latest_signals.parquet`.

El runtime Rust ya tiene store, queries y CLI, pero todavia no tenia una frontera profesional para incorporar ese output offline al sistema de eventos.

## Decision

Se introduce un handoff boundary v1 con estas decisiones:

- el input aceptado es el Parquet local generado por `research_prediction_markets`
- la ingestión es offline y manual via CLI
- el unico evento generado en v1 es `signal.generated`
- `hypothesis.generated` se pospone porque el output actual no la sustenta con claridad contractual
- la decodificacion de Parquet se hace con un script Python explicito y pequeno, sin reescribir el laboratorio ni introducir runtime live
- Rust conserva la validacion, la traduccion semantica y la persistencia contractual

## Consecuencias

Positivas:

- se cierra la primera frontera Python -> Rust con alcance pequeno
- el sistema gana trazabilidad desde output research hasta evento persistido
- la deduplicacion sigue delegada al store/event model actual

Negativas:

- la lectura de Parquet depende de Python en v1
- el handoff todavia no cubre otras familias de artefactos research

## No decidido todavia

- lectura nativa de Parquet en Rust
- versionado explicito del schema de handoff
- promocion de `signal_name` o `hypothesis` a contrato mas fuerte
