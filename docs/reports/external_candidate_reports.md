# External Candidate Reports

`agents/external_candidate_report.py` genera un reporte Markdown de observabilidad para candidatos externos en modo shadow/research.

## Objetivo

Consolidar trazabilidad operativa por:

- estrategia
- asset/window
- run/source

sin introducir ejecución live ni motor alterno.

## Entradas

- JSONL de eventos externos (por defecto `./var/events/external_candidates.jsonl`).

Eventos usados:

- `candidate_signal.scored`
- `shadow_fill.simulated`
- `strategy_round.scored`
- `candidate_strategy.evaluated`
- `feed_health.checked`
- `data_gap.detected`

## Salida

- Reporte Markdown en `reports/external_candidates/`.
- Nombre por defecto: `external_candidate_report_<RUN_ID>.md`.

## Secciones del reporte

- Summary
- Signal counts
- Rejection reasons
- Fill simulation assumptions
- PnL gross/net (si existe)
- Feed health
- Data gaps
- Governance status
- Next action

## Uso

```bash
bash scripts/generate_external_candidate_report.sh
```

Con filtro de estrategia y run id fijo:

```bash
EXTERNAL_REPORT_STRATEGY_VERSION=oracle_lag_v1 EXTERNAL_REPORT_RUN_ID=manual_01 \
  bash scripts/generate_external_candidate_report.sh
```

Salida directa a path específico:

```bash
python3 agents/external_candidate_report.py \
  --input-jsonl ./var/events/external_candidates.jsonl \
  --output-report ./reports/external_candidates/oracle_lag_v1_report.md \
  --strategy-version oracle_lag_v1
```
