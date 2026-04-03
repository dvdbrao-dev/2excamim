# Repository Guidelines

## Proyecto y alcance
Este repositorio es `2EXCAMIM` y el lenguaje actual es Rust. Mantén el alcance pequeño y explícito: no amplíes requisitos, capas o responsabilidades sin pedir confirmación primero. No tocar código ni comportamiento de `EXCAMIM` legado como parte de cambios en este crate.

## Estructura del repositorio
El código vive en `src/` y está dividido por responsabilidad: `events/`, `store/`, `codecs/`, `projections/`, `queries/`, `scenarios/`, `observability/` y `application/`. Los tests de integración viven en `tests/` con archivos por área, por ejemplo `tests/store_tests.rs` o `tests/queries_tests.rs`. Reutiliza módulos existentes antes de crear lógica nueva o duplicada.

## Restricciones técnicas
Reglas duras para este repo:

- No usar `async`.
- No meter red.
- No meter base de datos.
- No ampliar alcance sin pedirlo.
- Mantener estilo sobrio y dependencias mínimas.

La persistencia actual es JSONL append-only y las proyecciones/consultas se resuelven en memoria. Preserva ese modelo salvo instrucción explícita.

## Comandos obligatorios
Antes de dar cualquier tarea por terminada, ejecuta siempre desde la raíz:

- `cargo fmt`
- `cargo check`
- `cargo test`

Si alguno falla, no cierres el trabajo como completado.

## Estilo de código
Sigue `rustfmt` y el estilo normal de Rust: `snake_case` para funciones, módulos y tests; `CamelCase` para tipos. Prefiere validación explícita, errores tipados y funciones pequeñas. Antes de añadir una abstracción nueva, verifica si la lógica ya existe en `src/`.

## Git y cambios grandes
Haz checkpoints de Git antes y después de tareas grandes. Usa commits pequeños y claros, siguiendo el patrón del historial reciente, por ejemplo: `feat: add observability summary v1`.
