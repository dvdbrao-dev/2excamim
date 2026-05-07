# Cross-Venue Market Matching (Research-Only)

Agent: `agents/cross_venue_matcher_candidate.py`

## Purpose

Provide a deterministic skeleton for future Polymarket/Kalshi market-pair research.
This component only scores potential market matches and emits research events.

## Scope

- No live trading
- No order placement
- No credentials
- No arbitrage execution
- No heavy ML dependency

## Matching primitives (v1)

- title normalization
- asset extraction (`BTC`, `ETH`, `SOL`)
- settlement-window extraction (`5m`, `15m`, `1h`, `1d`)
- strike extraction (simple numeric parsing)
- text similarity (`difflib.SequenceMatcher`)
- confidence scoring with explicit reasons and reject causes

## Output event

Event type:

- `cross_venue.market_match_scored`

Payload includes:

- `polymarket_market_id`
- `kalshi_market_id`
- `polymarket_title`
- `kalshi_title`
- `asset`
- `window`
- `strike`
- `confidence`
- `reasons`
- `rejected`
- `reject_reason`

## Matching is not edge

A high-confidence market title match is metadata alignment only.
It does not imply pricing inefficiency, tradable edge, or executable arbitrage.
Any strategy claims require separate spread/latency/liquidity/fee validation under shadow controls.

## Example

```bash
python3 agents/cross_venue_matcher_candidate.py \
  --polymarket-markets-json ./data/polymarket_markets.json \
  --kalshi-markets-json ./data/kalshi_markets.json \
  --output-jsonl ./var/events/cross_venue_matches.jsonl \
  --min-confidence 0.90
```
