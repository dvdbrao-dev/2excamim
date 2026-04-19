# Estrategia activa: Maker Liquidity en NO-side

## Hipótesis central
El flujo retail en Polymarket está sistemáticamente sesgado hacia YES en 
mercados con baja probabilidad real. Un maker que provee liquidez en el 
lado NO de estos mercados cobra el spread sin necesitar forecasting.

## Por qué el forecasting no funciona
- El midpoint de Polymarket agrega información de traders reales con dinero real
- GPT-4o-mini no tiene información privilegiada sobre eventos binarios near-term
- El blend LLM/mercado degradaba la estimación de p_win que alimenta Kelly
- Evidencia: 219 de 219 vetos fueron causados por p_final < 0.10 por el blend LLM

## Cambios aplicados (Abril 2026)
- ALPHA = 0.0 en probability_agent.py (LLM desactivado como estimador)
- MAX_RESOLUTION_HOURS = 720 (universo ampliado a 30 días)
- Eliminado filtro price_too_extreme
- Añadido filtro not_in_maker_target_range (objetivo: NO >= 0.85)

## Universo de mercados objetivo
- Mercados con midpoint YES <= 0.15 (NO >= 0.85)
- Volumen mínimo: 50.000 USDC
- Horizonte: 4h a 720h hasta resolución
- Categorías con más flujo retail sesgado: Sports, Pop-Culture, Crypto

## Lo que NO se toca hasta validar edge
- Arquitectura Rust/event-sourcing
- Paper ledger y PaperRiskGuard
- Confirmation agent y sus thresholds
- Live gateway (sigue diferido)
- Kelly sizing (hasta tener fills reales)

## Próximas validaciones pendientes
1. Análisis histórico: dado midpoint_NO >= 0.85, tasa real de resolución NO
   API: https://gamma-api.polymarket.com/markets?closed=true&active=false
2. Medir decisiones formadas con nueva configuración tras 24h de pipeline
3. Activar fills paper reales cuando decisiones > 20/día

## Migración V2

### Ventana táctica post-22 abril
- Orderbook wipe total: cero queue priority heredado
- Gap de liquidez estimado: 24-72h mientras makers API-only migran manualmente
- Fee taker en rango NO >= 0.85: mínima (p*(1-p) <= 0.1275)
- Makers 0% fees en V2 confirma estrategia

### Preparación completada
- py-clob-client-v2 instalado
- Spread monitor en scripts/clob_spread_monitor.py
- Pendiente: registrar Builder Code en polymarket.com/settings?tab=builder
