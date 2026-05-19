# FASE 1 — ROOT CAUSE BACKEND
## systemd unit
- Estado observado: `activating` en ejecución actual, pero con historial repetido de `Failed with result 'exit-code'` y `status=127`.
- Último error repetido en journal: `timeout: failed to run command 'cargo': No such file or directory`.
- Comando del unit: `ExecStart=/usr/bin/bash /root/2excamim/scripts/run_pipeline.sh`.

## Contadores de fallos
- Directorio presente: `/root/2excamim/var/.failures/`.
- No se encontraron archivos `*.count` en el momento de inspección.
- Kill switch no presente (`/root/2excamim/var/.kill_switch` no existe).

## Errores por agente
- confirmation_agent: OK (sin stacktrace; corre y reporta métricas).
- sizing_agent: OK (sin stacktrace; corre y reporta métricas).
- exit_agent: OK (sin stacktrace; corre y reporta métricas).
- shadow_live_agent: OK (sin stacktrace; corre y reporta métricas).
- drawdown_guard: OK por script real (`agents/drawdown_guard.py`). Nota: el comando `agents/drawdown_guard_agent.py` falla porque ese archivo no existe.

## Diagnóstico
Causa raíz principal: el backend se corta antes de completar por fallo de entorno en el pipeline (`cargo` no disponible para `market-watch`), y además la observabilidad actual en `run_agent()` oculta fallos de agentes al no propagar error.
