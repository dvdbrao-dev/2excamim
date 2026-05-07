# External Candidate Scorecard

## Purpose
`agents/external_candidate_scorecard.py` agrega señales/fills/rounds de candidatos externos y evalúa estado de gobernanza:
- `candidate`
- `promoted` (solo sugerido)
- `frozen`
- `rejected`

## Safety Rule
Por defecto no promueve automáticamente. Si el desempeño es fuerte, marca `suggested_status=promoted` pero mantiene `status=candidate`.

## Threshold Table
| Metric | Default |
|---|---:|
| `min_signals` | 200 |
| `min_resolved_rounds` | 100 |
| `max_drawdown_usdc` | 50.0 |
| `min_net_expectancy_bps` | 3.0 |

## Governance Logic
- muestra insuficiente: `candidate`
- expectativa negativa con muestra suficiente: `rejected`
- problemas severos de calidad de datos: `frozen`
- positivos pero sin muestra suficiente: `candidate`
- positivos con muestra suficiente: `suggested_status=promoted` y `status=candidate`
