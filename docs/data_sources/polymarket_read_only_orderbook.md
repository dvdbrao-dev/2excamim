# Polymarket Read-Only Orderbook

## No-live guarantee
Esta integración usa solo endpoints públicos read-only de Gamma y CLOB. No usa wallet, firma, `POLY_API_KEY`, órdenes, cancelaciones ni endpoints autenticados.

## Endpoints públicos usados
- Metadata/discovery: `https://gamma-api.polymarket.com/markets`
- Orderbook: `https://clob.polymarket.com/book?token_id=...`

## Por qué importa resolver token_id
El CLOB orderbook se consulta por `token_id` de outcome. Un slug correcto sin `token_id` correcto produce `data_gap.detected`.

## Riesgo de matching de metadata
El lookup intenta slug exacto; si falla, usa fallback conservador (asset + window + cercanía temporal). Si la confianza es baja o ambigua, rechaza con error estructurado (`metadata_low_confidence`, `metadata_ambiguous_fallback`).

## Qué cuenta como orderbook usable
Se considera usable cuando existe al menos un outcome con:
- `best_bid` y `best_ask`
- `mid_price`
- `spread_bps`
- `depth_top_n` e `imbalance_top_n`

Si falla, no se inventan valores y se registra `data_gap.detected` con errores como `http_404`, `timeout`, `dns`, `parse_error`.

## Smoke
```bash
bash scripts/run_polymarket_orderbook_smoke.sh
STRICT=1 bash scripts/run_polymarket_orderbook_smoke.sh
```

Salida por defecto: `var/events/polymarket_orderbook_smoke.jsonl`.

## Cómo leer STRICT=1
`STRICT=1` falla solo cuando:
- no hay snapshots reales de spot Binance, o
- metadata/orderbook estaba habilitado pero `orderbook_observed_count=0`.

Si falla por mapeo inexistente a mercado activo, revisar `data_gap.detected` y mejorar discovery/fallback Gamma.

## Riesgos conocidos
- cambios de schema o endpoint Gamma/CLOB
- slug incorrecto
- baja liquidez o book vacío
- libros stale
- rate limits

## Limitación de edge
Tener metadata + orderbook read-only no prueba edge. Solo habilita mejor observabilidad para hipótesis `oracle_lag_v1`.
