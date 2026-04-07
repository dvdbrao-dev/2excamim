# Order Query v1

`Order Query v1` is a query-only report centered on `order_id`.

It exists to answer, with a small typed surface:

- whether the order is only registered
- whether it has been locally submitted
- whether fills have been observed
- whether the current local lifecycle is weak or inconsistent

## Report shape

`OrderLifecycleReport` exposes:

- `order_id`
- `status`
- `reasons`
- `decision_refs`
- `observed_fill_ids`
- `venue_refs`
- `notes`

## Status semantics

- `Registered`: a local order entity exists, but no local submission is yet visible.
- `Submitted`: the order is locally registered and submitted, but no fill is yet visible.
- `ObservedWithFills`: registration, submission, and downstream fills are all visible coherently.
- `Weak`: the order is partially observable, but local lifecycle support is incomplete.
- `Inconsistent`: the current log exposes real contradictions, such as submission without
  registration or conflicting venue references.

## Integration

- The report derives directly from `order.registered`, `order.submitted`, and `fill.received`.
- When exactly one `decision_id` is traceable, the report also adds note-level context from
  decision execution boundary and governance.
- The report does not replace `execution_boundary_v1`; it gives a complementary order-first view.
