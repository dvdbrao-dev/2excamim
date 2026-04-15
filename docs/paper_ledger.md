# Paper Ledger Projection v1

The paper ledger is a projection over canonical events. It does not read backend state as source of truth.

Position lifecycle:

- `open`: buys have created a positive net share balance and no sell has reduced it.
- `partially_closed`: the position still has positive net shares, but at least one sell has reduced the position.
- `closed`: net shares are zero after sells have fully offset buys.

PnL rules:

- Buys increase net shares and remaining cost basis.
- Sells reduce net shares.
- Realized PnL is calculated with average cost:
  - `realized_pnl += closed_shares * (sell_price - average_entry_price)`
- Remaining cost basis is reduced by the average cost of the shares closed.
- Average entry is shown for open shares only.
- Unrealized PnL is included only when a mark is available from canonical fill data for the same market/outcome:
  - `unrealized_pnl = net_shares * (current_mark_price - average_entry_price)`

This is intentionally conservative and deterministic. It is sufficient for paper-trading observability, not a replacement for broker-grade accounting.
