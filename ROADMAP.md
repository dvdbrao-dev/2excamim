# Roadmap de 2EXCAMIM

Este roadmap describe el estado real del sistema actual. No es una lista histórica de deseos: separa lo ya construido, el siguiente frente de trabajo y lo que se mantiene fuera de alcance por ahora.

2EXCAMIM sigue siendo un sistema offline y append-only orientado a investigación, confirmación, gobierno y paper execution controlada. El sistema todavía no debe considerarse una plataforma de trading live.

## 0. Pivot Estratégico — Abril 2026

Estado: aplicado.

### Qué cambió y por qué
El sistema operaba con hipótesis de forecasting (predecir mejor que el mercado).
Auditoría reveló problema crítico: 219 de 219 vetos causados por blend LLM
arrastrando p_final por debajo de 0.10. El LLM no aportaba criterio,
apagaba señales.

Nueva hipótesis: maker liquidity en NO-side de mercados con flujo retail
sesgado. No se predice el outcome, se cobra el spread acomodando flujo
desequilibrado. Ver STRATEGY.md.

### Cambios técnicos aplicados
- ALPHA = 0.0 (probability_agent.py)
- MAX_RESOLUTION_HOURS = 720
- Filtro price_too_extreme eliminado
- Filtro not_in_maker_target_range añadido (NO >= 0.85)
- var/ excluido de git tracking

### Próximas prioridades
1. Medir decisiones formadas con nueva config (24h)
2. Validar hipótesis con histórico de Polymarket
3. Cambiar sizing_agent a limit orders cuando decisiones > 20/día
4. Fills reales con tamaño mínimo cuando paper loop sea estable

### No se toca hasta validar edge bruto
- Arquitectura Rust/event-sourcing
- Live gateway
- Kelly sizing
- Confirmation agent thresholds

---

## 1. Research y Generación de Señales

Estado: implementado en versión funcional mínima.

Incluye:

- Ingesta e investigación inicial de mercados de predicción.
- Generación de señales desde outputs de research.
- Export de señales desde Python y handoff hacia Rust.
- Traducción contractual a eventos canónicos `signal.generated`.
- Validación de registros, deduplicación y reportes de ingestión.

Pendiente siguiente:

- Mejorar datasets históricos y baselines estadísticos.
- Mantener la frontera Python -> Rust simple y audititable.
- Evaluar cuándo una señal justifica materializar también `hypothesis.generated`.

Diferido / no prioritario:

- Convertir research en un servicio live.
- Acoplar el runtime Rust a internals Python.

## 2. Confirmación de Señales

Estado: implementado y operativo.

Incluye:

- `ConfirmationAgent` v1.
- `ConfirmationRunner`.
- Scorecards de confirmación.
- Persistencia de `signal.confirmed` para señales elegibles.
- Políticas de confirmación configurables desde JSON.
- Saltos explícitos por señales ya confirmadas, congeladas o fuera de política.
- Reportes de aceptación, rechazo, baja confianza, stale y overrides de política.

Pendiente siguiente:

- Consolidar perfiles de política por familia de señal cuando haya suficiente evidencia.
- Seguir reduciendo ambigüedad entre research score, confirmation score y readiness.

Diferido / no prioritario:

- Confirmación basada en red o feeds live dentro del runtime.
- Automatización always-on.

## 3. Evaluación Analítica y Validación

Estado: implementado y usado como base de gobierno.

Incluye:

- Medición ex-post de outcomes de señales confirmadas.
- Comparación entre señales confirmadas y señales generadas.
- Policy sweep sobre thresholds, horizontes y delta mínimo.
- Advisory classification por familia de señal.
- Validación walk-forward / era-based.
- Propuesta exportable de políticas de confirmación.

Pendiente siguiente:

- Aumentar cobertura de datos históricos para que las conclusiones sean menos frágiles.
- Mantener las reglas de clasificación explícitas y revisables.
- Usar los reportes analíticos como input para decisiones de promoción, no como ejecución automática.

Diferido / no prioritario:

- Optimización opaca o demasiado automática de parámetros.
- Entrenamiento o selección de políticas sin trazabilidad.

## 4. Readiness y Gobierno de Señales

Estado: implementado en primera versión.

Incluye:

- Materialización de readiness/governance para familias de señales.
- Estados explícitos: `experimental`, `candidate`, `promoted`, `frozen`.
- Evidencia resumida por familia, dirección y fuente cuando está disponible.
- Provenance y rationale resumidos.
- Export JSON en `var/readiness/confirmation_readiness.json`.
- CLI `materialize-confirmation-readiness`.

Pendiente siguiente:

- Usar readiness como input visible para humanos antes de ampliar automatización.
- Afinar reglas con más datos reales de outcomes y walk-forward.
- Alinear readiness, policy proposal y confirmation policy activa en un flujo operacional claro.

Diferido / no prioritario:

- Gobierno distribuido o dependiente de base de datos.
- Flujos de aprobación complejos antes de que el producto los necesite.

## 5. Runtime, Eventos y Persistencia

Estado: implementado como runtime offline modular.

Incluye:

- CLI Rust para inspección y ejecución de flujos offline.
- Modularización de parser, dispatch y renderers.
- Persistencia JSONL append-only.
- Proyecciones y consultas en memoria.
- Inspección de `signal`, `decision`, `order` y `fill`.
- Reportes texto y JSON.
- Batch runner básico para encadenar ingestión, materialización y resumen.

Pendiente siguiente:

- Un comando único `run-paper-pipeline` que ejecute el flujo paper completo de forma reproducible.
- Mantener la CLI como superficie clara para pruebas, demos y operación manual.
- Evitar que el runtime se convierta prematuramente en daemon.

Diferido / no prioritario:

- Base de datos.
- Scheduler always-on.
- Rediseño amplio del event model.

## 6. Decisiones, Órdenes y Observación de Ejecución

Estado: implementado en versión mínima y canónica.

Incluye:

- Materialización prudente de `decision.formed`.
- Materialización de `order.registered`.
- Persistencia de `order.submitted`.
- Observación canónica de `fill.received`.
- Detección de duplicados por idempotency key.
- Boundaries y readiness para decisiones, órdenes y fills.
- Resumen de ejecución por orden.

Pendiente siguiente:

- Hacer más explícita la cantidad objetivo de una orden cuando sea necesario.
- Mejorar reconciliación de fills y estados parciales sin introducir broker semantics prematuras.

Diferido / no prioritario:

- Accepted/rejected de venues reales.
- Gateway live.
- Semántica completa de broker o exchange.

## 7. Paper Execution y Adapter de Polymarket

Estado: prototipo implementado y subordinado a 2EXCAMIM.

Incluye:

- Adapter mínimo hacia `polymarket-paper-trader`.
- Boundary estrecho por CLI / fixture, sin importar internals Python en Rust.
- `PaperExecutionRequest`.
- Mapeo de trade/fill backend a resultado canónico.
- Persistencia de `fill.received` por la ruta canónica.
- Fixture mode para tests deterministas.
- Deduplicación de importaciones repetidas del mismo backend trade.

Pendiente siguiente:

- Mantener el adapter reemplazable.
- Aumentar cobertura de fixtures para casos parciales o inconsistentes.
- Definir mejor qué campos de backend son contractuales y cuáles son solo observación.

Diferido / no prioritario:

- Usar `polymarket-paper-trader` como sistema core.
- Multi-account orchestration.
- Dependencia fuerte de schema interno del backend.

## 8. Paper Decision Runner

Estado: implementado como primer loop paper end-to-end.

Incluye:

- Carga de señales confirmadas elegibles.
- Skip de señales ya ejecutadas.
- Mapeo simple de señal confirmada a paper order.
- Persistencia de `order.registered` y `order.submitted`.
- Llamada al adapter paper.
- Persistencia de `fill.received`.
- Reporte con señales vistas, requests enviados, fills persistidos, duplicados y skips.

Pendiente siguiente:

- Crear `run-paper-pipeline` como comando operativo único que agrupe confirmación, decisión paper, riesgo, ejecución fixture/backend y ledger.
- Mejorar reglas de sizing y filtros sin convertirlo en motor live.
- Mantener idempotencia y reproducibilidad como requisitos centrales.

Diferido / no prioritario:

- Ejecución live.
- Orquestación multi-cuenta.
- Scheduler continuo.

## 9. Paper Ledger y Riesgo

Estado: implementado en primera versión.

Incluye:

- Proyección canónica de paper ledger desde eventos.
- Vistas de órdenes, fills y posiciones abiertas.
- Exposición por mercado y outcome.
- Spend / proceeds acumulados cuando son derivables.
- CLI `show-paper-ledger`.
- `PaperRiskGuard` antes de ejecución paper.
- Límites configurables:
  - máximo de posiciones abiertas
  - exposición nocional total
  - exposición por mercado
  - máximo de órdenes por mercado
  - bloqueo opcional de duplicado mismo mercado / mismo outcome
- Reporte de bloqueos por razón.

Pendiente siguiente:

- Enriquecer ciclo de vida de posiciones paper.
- Proyección de PnL realizada y no realizada.
- Cierres parciales y completos más claros.
- Métricas de exposición útiles para intervención humana.

Diferido / no prioritario:

- Risk engine institucional.
- Margen, liquidación o collateral real.
- Dependencia del backend como source of truth.

## 10. Dashboard / Control Room v1

Estado: siguiente frente importante, no implementado aún.

Rationale:

2EXCAMIM ya puede generar, confirmar, gobernar y ejecutar en paper de forma mínima. El siguiente salto de utilidad no es más automatización, sino visibilidad operacional. Un fundador o operador debe poder entender rápidamente qué está haciendo el sistema, qué está bloqueado, qué está en paper, qué familias están promovidas o congeladas y dónde hay riesgo acumulado.

Incluye esperado:

- Vista de estado del sistema:
  - señales generadas
  - señales confirmadas
  - familias por readiness
  - decisiones paper recientes
  - fills paper recientes
  - posiciones abiertas
  - exposición por mercado y outcome
- Panel de riesgo:
  - límites activos
  - bloqueos recientes
  - razones de bloqueo
  - mercados con mayor exposición
- Panel de investigación:
  - advisory por familia
  - resultados walk-forward
  - políticas propuestas vs política activa
- Panel de operación:
  - últimos comandos o runs
  - conteos por fase
  - errores accionables

Pendiente siguiente:

- Definir si la primera versión será estática, CLI-rendered, o una UI local simple.
- Priorizar lectura y control humano sobre automatización.
- Mantener el dashboard como consumidor de eventos/proyecciones, no como nuevo source of truth.

Diferido / no prioritario:

- Trading controls live.
- Edición compleja de políticas desde UI.
- Multiusuario, permisos o deployment cloud.

## 11. Live Gateway / Ejecución Real

Estado: intencionalmente diferido.

## 12. External Edge Candidates V1

Estado: planificado (candidate/shadow-first, sin implementación de agentes en esta fase).

### Phase 0: event contract and docs
- Formalizar frontera documental para edge externo derivado.
- Exigir envelope mínimo por evento (`event_type`, `event_id`, `timestamp`, `idempotency_key`, `aggregate_key`, `provenance`, `payload`).
- Mantener Rust como única capa contractual de persistencia/event-sourcing.
- Estado: en progreso (utilidad base y tests iniciales agregados en `agents/core/event_envelope.py`).

### Phase 1: slot discovery candidate
- Crear candidato de discovery determinista para slots cripto 5m/15m.
- Base de investigación: patrones de `Polymarket-Market-Finder` sin importar repo completo.
- Salida prevista: universo de mercados candidato trazable y reproducible.
- Estado: en progreso (`agents/market_slot_discovery_candidate.py` + script opcional + tests).

### Phase 2: research snapshot collector
- Construir collector research-only de snapshots (oracle/spot/orderbook).
- Base de investigación: `polyrec` como referencia de harness.
- Persistencia append-only en JSONL, sin rediseñar store actual.
- Estado: en progreso (`agents/research_collector_candidate.py` con adapters mock, features puros y eventos de health/gap).

### Phase 3: oracle lag candidate scorer
- Evaluar hipótesis de lag entre oracle y precio spot.
- Base de investigación: `gengar_polymarket_bot` como señal de hipótesis, no como bot ejecutable.
- Producir scoring candidato auditable para promoción posterior.
- Estado: en progreso (`agents/oracle_lag_signal_candidate.py` con scoring determinista + filtros conservadores).

### Phase 4: shadow execution and scorecard
- Ejecutar en modo shadow/paper con scorecard explícito.
- Medir precisión, estabilidad, drawdown proxy y sensibilidad a costos.
- No habilitar live execution en esta fase.
- Estado: en progreso (`agents/shadow_execution_simulator.py` con fill assumptions conservadores y round scoring).
- Estado scorecard externo: en progreso (`agents/external_candidate_scorecard.py` con evaluación candidate/promoted/frozen/rejected y promoción solo sugerida).

### Phase 5: offline backtest harness
- Consolidar harness offline para replay/backtest de candidatos externos.
- Reusar pipeline y gobernanza actual (candidate/promoted/frozen/rejected).
- Mantener reproducibilidad e idempotencia como requisito central.
- Estado: en progreso (`agents/backtest_external_candidate.py` + reporte markdown + event log de backtest).

### Phase 6: optional cross-venue matcher research
- Investigación opcional offline de matching Kalshi/Polymarket.
- Sin integración operativa ni ejecución cross-venue.
- Activar solo cuando fases 1-5 tengan evidencia suficiente.

Incluye hoy:

- Nada live.
- Solo paper execution controlada y observable.

Pendiente siguiente:

- Considerar live gateway solo si el paper loop demuestra estabilidad, trazabilidad y valor.
- Antes de live, exigir:
  - readiness confiable
  - ledger paper robusto
  - PnL y reconciliación suficientes
  - dashboard operativo
  - límites de riesgo claros
  - revisión humana explícita

Diferido / no prioritario:

- Trading real.
- Gateway de ejecución live.
- Scheduler always-on.
- MCP como boundary productivo.
- Broad backend coupling.
- Multi-account orchestration.

## Prioridades Actuales

Estado: activo.

Pendiente siguiente:

1. Implementar `run-paper-pipeline` como flujo end-to-end reproducible.
2. Enriquecer paper ledger con ciclo de vida de posiciones y PnL.
3. Diseñar Dashboard / Control Room v1 como capa de observabilidad humana.
4. Evaluar live gateway solo después de que paper execution y control room sean confiables.

Diferido / no prioritario:

- Acelerar hacia live trading antes de que el sistema sea observable y gobernable.
- Convertir prototipos de backend en dependencias centrales.
- Ampliar infraestructura antes de que el flujo paper sea claro.
