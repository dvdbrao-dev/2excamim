# Crypto Strategies Lab v1

Este laboratorio añade dos estrategias OHLCV en paper trading sin tocar ejecución real.

## Estrategias

### 1) ADX + EMA Pullback v1
- `strategy_id`: `crypto_adx_ema_pullback_v1`
- Símbolos: `BTCUSDT`, `ETHUSDT`, `SOLUSDT`
- Timeframes: `1h`, `4h`
- Indicadores: `EMA21`, `EMA50`, `ADX14`, `RSI14`, `ATR14`
- Regla LONG:
  - `EMA21 > EMA50`
  - `ADX14 > 25`
  - Precio cerca de `EMA21` (pullback)
  - `RSI14` recupera/cruza 50 desde abajo
- Stop loss: `entry - 1.5 * ATR14`
- Take profit: `entry + 2 * (entry - stop_loss)`

### 2) Volatility Breakout v1
- `strategy_id`: `crypto_volatility_breakout_v1`
- Símbolos: `BTCUSDT`, `ETHUSDT`, `SOLUSDT`
- Timeframe: `1h`
- Indicadores: `Bollinger Bands(20,2)`, `ATR14`, `ATR SMA50`, `MACD(12,26,9)`, `Volume SMA20`
- Regla LONG:
  - `close > upper_band`
  - `ATR14 > ATR_SMA50`
  - `MACD histogram > 0`
  - `volume > volume_SMA20`
- Anti-FOMO:
  - No entrar si tamaño vela de ruptura `> 2.5 * ATR14`
- Stop loss: `entry - 1.5 * ATR14`
- Take profit: `entry + 2 * (entry - stop_loss)`

## Eventos generados

Formato compatible en `var/events.jsonl`:
- `event_type`: `crypto.signal.generated`
- `schema_version`: `v1`
- `produced_by`: `runtime.agent.crypto_strategy`
- `aggregate_key`: `crypto:<SYMBOL>:<TIMEFRAME>`

Payload principal:
- `signal_id` (determinista)
- `strategy_id`
- `symbol`
- `timeframe`
- `side` (`LONG`)
- `entry_price`
- `stop_loss`
- `take_profit`
- `risk_reward`
- `indicators`
- `reason`
- `generated_at`
- `candle_close_time`

Idempotencia:
- `idempotency_key` incluye `strategy_id + symbol + timeframe + candle_close_time`
- No se duplican señales de la misma vela cerrada.

## Datos OHLCV

- Fuente pública Binance: `GET /api/v3/klines`
- Sin API key.
- Cache local: `var/crypto_ohlcv/` por estrategia/símbolo/timeframe.

## Ejecución

Agentes nuevos:
- `python3 agents/crypto_adx_ema_pullback_agent.py --store ./var/events.jsonl --cache-dir ./var/crypto_ohlcv`
- `python3 agents/crypto_volatility_breakout_agent.py --store ./var/events.jsonl --cache-dir ./var/crypto_ohlcv`
- `python3 agents/crypto_strategy_scorecard_agent.py --store ./var/events.jsonl --json`

Pipeline:
- `scripts/run_pipeline.sh` incluye estos agentes con guardia:
- `ENABLE_CRYPTO_STRATEGIES=1 bash scripts/run_pipeline.sh`

## Scorecard

`agents/crypto_strategy_scorecard_agent.py` agrega por `strategy_id`:
- `signals_generated`
- `decisions_formed`
- `vetoes`
- `fills`
- `open_positions`
- `pnl`
- `recommended_state`: `KEEP_IN_PAPER | FREEZE | PROMOTE_CANDIDATE | KILL`

Interpretación rápida:
- `KEEP_IN_PAPER`: muestra todavía insuficiente o neutral
- `FREEZE`: mala tracción (ej. muchas señales sin fills o PnL negativo con fills)
- `PROMOTE_CANDIDATE`: señales + fills + PnL positivo consistente
- `KILL`: veto estructural dominante

## Darwinian Strategy Incubator

`agents/crypto_strategy_incubator_agent.py` añade una capa de gobernanza para evitar que estrategias sin edge contaminen el laboratorio.

Modo operativo:
- `paper_shadow_only` (sin ejecución real, sin llaves, sin órdenes live)

Estados de estrategia:
- `candidate`: estrategia nueva o con muestra insuficiente.
- `shadow`: ya genera muestra útil y sigue en observación.
- `promoted`: supera umbrales mínimos de calidad y consistencia.
- `frozen`: degradación temporal (drawdown o ventanas negativas) hasta nueva evidencia.
- `rejected`: fallo repetido estructural; no debe seguir en rotación activa.

Reglas iniciales (configurables):
- Archivo: `config/crypto_strategy_incubator.yaml`
- `min_signals_for_shadow: 20`
- `min_signals_for_promotion: 100`
- `min_expectancy_for_promotion: 0.0`
- `min_profit_factor_for_promotion: 1.2`
- `max_drawdown_allowed: 0.15`
- `min_confidence_for_promotion: 0.60`
- `freeze_after_negative_windows: 3`
- `reject_after_failed_runs: 5`

Registro y trazabilidad:
- Registry runtime: `runtime/crypto_strategy_registry.json`
- Ejemplo versionado: `data/crypto_strategy_registry.example.json`
- Cada evaluación actualiza estado, contadores y timestamps; no borra estrategias automáticamente.

Salida JSON del incubador:
- `agent`, `mode`, `timestamp`
- `evaluated_strategies`
- `promoted`, `frozen`, `rejected`
- `warnings`
- `summary` con totales

Interpretación rápida:
- `promoted` creciente con drawdown controlado => edge potencial.
- `frozen` recurrente => revisar régimen/parametrización.
- `rejected` => sacar de la incubadora activa y re-diseñar antes de reintroducir.

## Evitar overfitting

- No optimizar parámetros en un único tramo temporal.
- Comparar por ventanas (walk-forward) y por símbolo.
- Congelar reglas antes de medir para evitar tuning retrospectivo.
- Separar análisis exploratorio de evaluación final.
- Mantener baseline (pipeline actual Polymarket) y medir uplift incremental.

## Seguridad operativa

- Solo paper trading.
- No ejecución de dinero real.
- No cambios en claves privadas ni credenciales.

## Activación/Desactivación

- Activo solo con `ENABLE_CRYPTO_STRATEGIES=1`.
- Integrado en `scripts/run_pipeline.sh` después de señales crypto + scorecard.
- Falla controlada: `python3 agents/crypto_strategy_incubator_agent.py --json || echo "WARN crypto_strategy_incubator_agent failed"`.

## Disk Cleanup And Generated Files Policy

No versionar artefactos regenerables o runtime:
- `target/`
- `__pycache__/`, `.pytest_cache/`, `.mypy_cache/`, `.ruff_cache/`, `.cache/`
- `*.log`, `*.tmp`
- `runtime/`, `tmp/`
- snapshots y data runtime de `var/`
- `scripts/dashboard/data.json` (snapshot operativo)

Mantener versionados solo ejemplos/base reproducible:
- `scripts/dashboard/data.example.json`
- `data/crypto_strategy_registry.example.json`
