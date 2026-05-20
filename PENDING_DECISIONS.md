# Pending Decisions

## 2026-05-20 — Definición exacta de fases 2–6
Pregunta: ¿Cuál es el contenido exacto del plan original para Fases 2, 3, 4, 5 y 6?

Contexto observado en historial:
- Fase 1 ya existe: `1ad8fb6` (`audit: fase 1 root cause backend muerto desde 16-abr`) con `audit_fase1.md`.
- Hay commits posteriores que parecen cubrir parte del plan:
  - `95265ba` (`fix: pipeline runtime — resolve cargo path for systemd runs`)
  - `726ad92` (`feat: observabilidad pipeline — last_error + health check`)
  - `2657db7` (`ops: clarificar orquestación (cron vs systemd)`)
  - `bf92025` (`fix: run_pipeline — continue core agents when market_watch fails`)

Sin la definición explícita de fases 2–6, continuar implicaría adivinar alcance/entregables.

## 2026-05-20 — market_watch desactivado por API externa inestable
Hecho aplicado: `scripts/run_pipeline.sh` deja `MARKET_WATCH_ENABLED=0` por defecto.

Motivo:
- Ejecución de `market-watch` falla con error de red externa:
  `Dns Failed: resolve dns name 'gamma-api.polymarket.com:443'`.
- Según regla de fase 2.3, ante API externa caída/inestable se documenta y se
  deja el agente desactivado con comentario explícito.

Decisión pendiente:
- ¿Cuándo reactivar `market_watch` en este host? Propuesta: habilitar con
  `MARKET_WATCH_ENABLED=1` solo tras validar DNS/salida HTTPS estable hacia
  `gamma-api.polymarket.com`.
