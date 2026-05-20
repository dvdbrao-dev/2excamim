# POST-MORTEM — Pipeline backend muerto 16-abr → 18-may 2026

## Resumen ejecutivo
El backend paper dejó de operar de forma autónoma por una combinación de fallo en `market_watch` y falta de visibilidad operativa en el pipeline. Durante el gap, se siguieron acumulando snapshots/eventos parciales sin continuidad de decisiones ejecutables. La recuperación se centró en observabilidad, clarificación de orquestación y tolerancia operativa en el pipeline.

## Causa raíz
`run_pipeline.sh` dependía de `market_watch` en condiciones de entorno inestable y el mecanismo de fallos acumulados activaba `kill_switch`, frenando ejecuciones posteriores; además, los errores quedaban poco visibles para operación diaria.

## Por qué tardamos 33 días en darnos cuenta
- `run_agent()` seguía en modo tolerante (no aborta), lo que ocultó gravedad real sin un health report explícito.
- Faltaba consolidación diaria de estado de fallos/stacktrace por agente en el reporte operativo.
- Existía ambigüedad de orquestación (cron vs systemd), reduciendo trazabilidad de cuál camino era el oficial.

## Cambios aplicados
- `1ad8fb6` — `audit: fase 1 root cause backend muerto desde 16-abr`
- `95265ba` — `fix: pipeline runtime — resolve cargo path for systemd runs`
- `726ad92` — `feat: observabilidad pipeline — last_error + health check`
- `2657db7` — `ops: clarificar orquestación (cron vs systemd)`
- `bf92025` — `fix: run_pipeline — continue core agents when market_watch fails`
- `35bb883` — `fix: market_watch_agent — disable by default on DNS outage`

## Verificación
- Decisiones formadas antes del fix: 8 (todas 16-abr long_yes)
- Decisiones formadas después del fix: 8
- Distribución side post-fix: 0 long_yes / 0 long_no
- Próximo evento esperado: 2026-05-20T16:00:00Z

## Recomendaciones futuras
- Rehabilitar `market_watch` solo con DNS/HTTPS estable hacia `gamma-api.polymarket.com` y monitoreo activo de latencia/error-rate.
- Añadir alerta explícita cuando `decision.formed` no crece por >6h continuas.
- Mantener una única orquestación oficial documentada y auditable (cron actualmente).
- Ejecutar revisión semanal de `var/.failures/*.last_error` y de `kill_switch`.
