# Architecture

## System Layers

2EXCAMIM separa responsabilidades de forma explícita:

- Rust: runtime operativo, eventos, consultas, proyecciones y servicios del sistema
- Python: laboratorio de research offline para experimentación controlada

### Python Research Layer - Prediction Markets

Ubicación:

`research_prediction_markets/`

Responsabilidades:

- ingestión de datos de prediction markets
- cálculo de features experimentales
- generación de signal candidates
- export estructurado para consumo posterior

No hace:

- ejecución
- risk management
- event orchestration
- decision making final

Encaje doctrinal:

- Python = research / experimentation / offline feature generation
- Rust = runtime operativo / eventos / decisiones / ejecución

Interfaz actual con el sistema:

- salida en Parquet
- preparado para futura traducción a eventos Rust
