MODO CODEX SIN CONFIRMACIONES:

Ejecuta este trabajo sin pedir permisos ni confirmaciones intermedias.

Asume aprobación para leer, editar, crear archivos, ejecutar tests, formatear, limpiar cachés seguras, hacer git status/diff/add/commit y push a la rama actual.

No preguntes “yes/no”.
No te detengas salvo riesgo real de borrar código fuente, secretos, datos críticos o información no regenerable.

Si dudas sobre un archivo, no lo borres: muévelo a ~/codex-rescue-2excamim-disk-cleanup/ y continúa.

OBJETIVO:
Convierte el laboratorio crypto del repo 2excamim en una incubadora darwiniana de estrategias que mida, compare, promocione estrategias buenas y congele/rechace estrategias malas.

Además, reduce espacio usado en disco borrando basura segura y regenerable.

RAMA ACTUAL:
feature/crypto-strategies-lab-v1

CONTEXTO:
Existen agentes crypto:
- agents/crypto_adx_ema_pullback_agent.py
- agents/crypto_volatility_breakout_agent.py
- agents/crypto_strategy_scorecard_agent.py
- agents/services/crypto_ohlcv.py

Integrados en:
- scripts/run_pipeline.sh
- scripts/dashboard/data.json
- docs/CRYPTO_STRATEGIES_LAB.md

Feature flag:
ENABLE_CRYPTO_STRATEGIES=1

TAREAS:

1. Audita el repo:
   - git status
   - du -h --max-depth=2 . | sort -h
   - find . -type f -size +10M -print
   - git ls-files scripts/dashboard/data.json

2. Limpia basura segura:
   - __pycache__/
   - .pytest_cache/
   - .mypy_cache/
   - .ruff_cache/
   - .cache/
   - target/
   - *.log temporales
   - *.tmp
   - runtime generado
   - snapshots runtime no críticos

No borres código fuente.
No borres secretos.
No borres datos no regenerables.

3. Revisa scripts/dashboard/data.json:
   - Si es snapshot operativo, sácalo del versionado.
   - Añade scripts/dashboard/data.json a .gitignore.
   - Crea scripts/dashboard/data.example.json si el dashboard necesita estructura base.
   - Ajusta lo necesario para no romper tests.

4. Crea configuración:

config/crypto_strategy_incubator.yaml

Con:

min_signals_for_shadow: 20
min_signals_for_promotion: 100
min_expectancy_for_promotion: 0.0
min_profit_factor_for_promotion: 1.2
max_drawdown_allowed: 0.15
min_confidence_for_promotion: 0.60
freeze_after_negative_windows: 3
reject_after_failed_runs: 5

5. Crea agente:

agents/crypto_strategy_incubator_agent.py

Debe:
- leer scorecards/registry disponibles
- evaluar estrategias
- asignar estado:
  candidate
  shadow
  promoted
  frozen
  rejected
- no operar dinero real
- no romper pipeline si faltan datos
- emitir JSON estable:

{
  "agent": "crypto_strategy_incubator",
  "mode": "paper_shadow_only",
  "timestamp": "...",
  "evaluated_strategies": [],
  "promoted": [],
  "frozen": [],
  "rejected": [],
  "warnings": [],
  "summary": {
    "total": 0,
    "promoted": 0,
    "frozen": 0,
    "rejected": 0
  }
}

6. Crea registry:

data/crypto_strategy_registry.example.json

Y si procede runtime/crypto_strategy_registry.json ignorado por git.

Debe incluir:
- strategy_id
- agent_file
- status
- created_at
- last_evaluated_at
- promotion_count
- freeze_count
- rejection_reason
- notes

7. Integra en:

scripts/run_pipeline.sh

Solo bajo:

ENABLE_CRYPTO_STRATEGIES=1

Después de agentes crypto y scorecard:

python agents/crypto_strategy_incubator_agent.py || echo "WARN crypto_strategy_incubator_agent failed"

8. Actualiza:

docs/CRYPTO_STRATEGIES_LAB.md

Añade:
- Darwinian Strategy Incubator
- estados candidate/shadow/promoted/frozen/rejected
- reglas de promoción/congelación/rechazo
- modo paper/shadow only
- cómo activar/desactivar
- cómo interpretar informe
- política de limpieza de disco y archivos generados

9. Crea tests:

tests/test_crypto_strategy_incubator_agent.py

Casos:
- pocas señales => candidate/shadow
- buenas métricas => promoted
- drawdown excesivo => frozen
- fallos repetidos => rejected
- datos incompletos => warning sin romper
- JSON estable

10. Revisa .gitignore y añade si falta:

__pycache__/
.pytest_cache/
.mypy_cache/
.ruff_cache/
.cache/
target/
*.log
*.tmp
runtime/
tmp/
data/live/
scripts/dashboard/data.json

Mantén examples versionables.

11. Ejecuta:

pytest tests/test_crypto_adx_ema_pullback_agent.py tests/test_crypto_volatility_breakout_agent.py tests/test_crypto_strategy_scorecard_agent.py tests/test_crypto_strategy_incubator_agent.py -q
cargo fmt
cargo check
cargo test

12. Antes de commit:
   - git status
   - git diff --stat
   - comprobar que no hay secretos
   - comprobar que no queda basura runtime versionada

13. Commit y push:

git add .
git commit -m "Add darwinian crypto strategy incubator and cleanup generated files"
git push

ENTREGA FINAL:
Devuelve:
- archivos añadidos
- archivos modificados
- archivos eliminados o movidos
- espacio aproximado liberado
- tests ejecutados
- resultado de tests
- estado git
- riesgos pendientes
- siguiente paso recomendado

CRITERIO:
No añadir más indicadores.
Crear selección natural medible.
