# Order Entity v1

`Order Entity v1` introduces the smallest local contractual form of `order`.

## Purpose

It exists to close the seam between:

- `decision.formed`
- `fill.received`

without introducing a full order lifecycle or execution runtime.

## Event

The entity is created by:

- `order.registered`

## Payload

`order.registered` carries only:

- `order_id`
- optional `decision_id`
- `instrument`
- `venue`

## Semantics

- `order_id` can originate externally, but after registration it becomes a local contractual handle.
- The event says the system recognizes the order; it does not say the order was submitted, accepted,
  amended, cancelled, or completed.
- The event helps queries interpret whether a fill is merely externally observed or is backed by a
  locally registered order linked to a decision.
- `order.submitted` now exists as the next minimal lifecycle step when local submission intent must
  be represented.

## Limits

- No lifecycle beyond registration and submission is defined in v1.
- Multiple orders for the same decision remain semantically limited until a fuller order model
  exists.
- `order_query_v1` provides a derived inspection surface for `order_id`, but it does not create a
  new persisted order state machine.
