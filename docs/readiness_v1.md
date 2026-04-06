# Readiness v1

`Readiness v1` is a query-only interpretation layer derived from the existing append-only event
log. It does not add new events, runtime execution, or scheduler behavior.

## Scope

The layer answers the current operational standing of:

- `signal`
- `decision`
- `fill`

using typed statuses plus explicit reasons.

## Semantics

### Signal

- `GeneratedUnconfirmed`: a `signal.generated` exists and no applicable veto is present.
- `ReadyForDecision`: a generated signal also has at least one confirmation and no applicable veto.
- `BlockedByVeto`: a signal-scoped veto exists.
- `Inconsistent`: the log exposes contradictions or impossible sequencing that the current model can
  detect, such as confirmation without generation.

### Decision

- `Ready`: a `decision.formed` exists, links to a signal, and that signal is readiness-healthy.
- `FormedButUpstreamWeak`: a decision exists but the current upstream evidence is not strong enough
  to call it healthy.
- `Blocked`: a decision-scoped veto exists or the upstream signal is blocked.
- `Inconsistent`: the log exposes contradictions, such as instrument mismatches or upstream signal
  inconsistency.

`Decision lineage / promotion boundary v1` refines this by exposing:

- traced upstream signal references
- traced hypothesis references when available
- applicable vetoes
- downstream fill observations
- explicit reasons for `supported`, `weak`, `blocked`, or `inconsistent`

### Fill

- `ReceivedWithSufficientReferences`: a `fill.received` exists and the referenced decision is
  readiness-healthy under the current model.
- `ReceivedButUpstreamInsufficient`: a fill exists but the current model cannot justify strong
  upstream support.
- `Inconsistent`: the log exposes contradictions, ambiguity, or mismatched downstream references.

## Known limits

- No first-class `order` entity exists yet, so fill sufficiency is still bounded by that absence.
- Historical enforcement is not applied at write time in this slice; readiness reports on the log as
  found.
- Readiness reasons are intentionally explicit so weak or insufficient outcomes can be traced back
  to model limits instead of hidden behind booleans.
