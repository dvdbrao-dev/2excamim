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
