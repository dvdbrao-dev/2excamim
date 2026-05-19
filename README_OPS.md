# EXCAMIM Ops

## Orquestación oficial del pipeline paper
- Orquestador oficial: `cron` del usuario `root`.
- Frecuencia oficial: `*/30 * * * * cd /root/2excamim && bash scripts/run_pipeline.sh >> /root/2excamim/logs/cron.log 2>&1`.
- `twoexcamim-paper-pipeline.service` y `twoexcamim-paper-pipeline.timer` quedan deshabilitados para evitar doble ejecución y estados conflictivos.

## Health check
- Script operativo: `scripts/pipeline_health.sh`.
- El reporte diario (`scripts/daily_health.sh`) incluye su salida.
