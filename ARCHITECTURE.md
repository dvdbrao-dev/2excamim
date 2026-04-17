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
- preparado para futura traduccion a eventos Rust

## Runtime Rust actual

El runtime operativo ya tiene un esqueleto ejecutable minimo:

- CLI de inspeccion sobre el store JSONL append-only
- consultas sobre `signal`, `decision`, `order` y `fill`
- reutilizacion directa de projections, observability y query layer

No incluye todavia:

- integracion automatica Python -> Rust
- resolver
- scheduler real
- ejecucion live

Siguiente integracion mayor:

- traducir el output estructurado del research Python a eventos Rust consumibles por el runtime

## Handoff Python -> Rust v1

La frontera minima actual entre research y runtime es:

- Python produce `latest_signals.parquet`
- un adaptador explicito decodifica ese Parquet a registros JSON
- Rust valida esos registros, los traduce y persiste `signal.generated`

Propiedades:

- offline
- local
- trazable por fichero y fila
- sin scheduler
- sin live ingestion
- sin red

Limite actual:

- el handoff no confirma señales ni forma decisiones
- el runtime no consume todavia otras salidas del laboratorio Python

## Crypto feed v1

Flujo operativo añadido para señales cripto:

```text
Binance API (gratis)
    ↓
crypto_price_agent.py → crypto.signal.generated
    ↓
crypto_market_matcher.py → crypto.market.matched
    ↓
confirmation_agent.py → signal.confirmed
    ↓
sizing_agent.py → decision.formed (cap 3%)
```

Propiedades:

- live price feed con klines de 1 minuto
- sin autenticacion
- sin coste de API
- acoplamiento a Polymarket via snapshots JSONL
- confirmacion y sizing reusan el store append-only existente
