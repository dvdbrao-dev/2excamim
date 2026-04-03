# Prediction Markets Research (MVP)

## Objetivo

Generar señales experimentales para 2EXCAMIM sin afectar el runtime.

## Ubicación

`research_prediction_markets/`

## Inputs

- Kalshi markets
- Polymarket markets
- Polymarket trades

## Features MVP

- spread_tight
- volume_spike_24h
- price_deviation_vwap_1h

## Signals MVP

- tight_spread_momentum
- vwap_reversion

## Output

Parquet:

`research_prediction_markets/output/signals/latest_signals.parquet`

## Rol dentro de 2EXCAMIM

Este módulo pertenece al laboratorio Python.
No ejecuta operaciones ni decide por sí mismo.
Su trabajo es producir candidatos de señal para futura integración con Rust.

## Limitaciones actuales

- baseline de volumen simplificado
- sin histórico persistido de 7 días
- sin backtesting serio todavía
- sin integración automática con el runtime
- sin streaming ni tiempo real real

## Ejecución mínima

```bash
cd research_prediction_markets
python3 -m venv .venv
source .venv/bin/activate
pip install requests pandas pyarrow
python main.py
```
