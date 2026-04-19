# Análisis de Mercado — 19 Abril 2026

## Hallazgos clave

### Universo real de mercados binarios CLOB
- 34 mercados binarios no-negRisk con volumen CLOB > 50k USDC
- Spreads reales: 0.001 a 0.024 (hay makers activos)
- Volumen diario por mercado: $3k-$10k (bajo pero operable)

### Mercados objetivo confirmados (NO >= 0.85, binarios, CLOB)
| Mercado | NO price | Spread | Vol 24h |
|---------|----------|--------|---------|
| Trump resign by Dec 2026 | 0.935 | 0.010 | $10,464 |
| Zelenskyy out by end 2026 | 0.835 | 0.010 | $4,611 |

### Lo que descartamos hoy
- negRisk multi-outcome: spreads 0.001, makers profesionales, 
  no competible para nuestro tamaño
- Binarios NO >= 0.85 con spread 0.998: libros vacíos, 
  solo YES en AMM, sin flujo CLOB real

### Conclusión
La hipótesis maker es viable en binarios políticos/eventos 
con NO >= 0.80, spread >= 0.010 y vol_24h >= $3k.
Universo actual: ~5-10 mercados simultáneos.
Edge esperado por trade: 0.010 (spread completo si fills ambos lados)

## Próximo paso
Validar fill rate real con órdenes mínimas post-migración V2.
Sin live gateway no hay más que aprender en paper para estos mercados.

---
Commit: "docs: market analysis — binary CLOB maker opportunity confirmed"
