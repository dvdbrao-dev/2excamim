# Shadow Execution Simulator

## Fill Assumptions
- Simula fills sobre `candidate_signal.scored` sin enviar órdenes.
- Usa probabilidad de fill conservadora basada en `spread`, `depth` y `latency`.
- Aplica slippage adverso siempre.
- Modela fee aproximada de taker para penalizar optimismo.

## Why Conservative
El objetivo es evitar falsos positivos. Si una señal sobrevive costos/fricción en este modelo, merece pasar a evaluación más estricta.

## Limitations
- No modela matching real por nivel de cola.
- No modela latencia de red real ni microburst.
- `mock_resolution` solo sirve para pruebas rápidas de PnL.

## Tuning
- `--fee-rate-bps`: subir para mayor prudencia.
- `--slippage-bps`: subir para escenarios líquidos adversos.
- `--latency-ms`: subir para degradar fill probability.
- `--max-notional-usdc`: limitar exposición por señal.
