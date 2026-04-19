import json
import requests

GAMMA_BASE = "https://gamma-api.polymarket.com"
CLOB_BASE = "https://clob.polymarket.com"


def parse_json_or_list(value):
    if isinstance(value, list):
        return value
    if isinstance(value, str):
        try:
            parsed = json.loads(value)
            if isinstance(parsed, list):
                return parsed
        except json.JSONDecodeError:
            return []
    return []


def parse_float(value, default=0.0):
    try:
        return float(value)
    except (TypeError, ValueError):
        return default


def get_active_markets():
    response = requests.get(
        f"{GAMMA_BASE}/markets",
        params={"active": "true", "limit": 200},
        timeout=10,
    )
    response.raise_for_status()
    payload = response.json()
    if isinstance(payload, list):
        return payload
    if isinstance(payload, dict):
        for key in ("data", "markets", "results"):
            value = payload.get(key)
            if isinstance(value, list):
                return value
    return []


def get_spread(token_id):
    response = requests.get(
        f"{CLOB_BASE}/book",
        params={"token_id": token_id},
        timeout=10,
    )
    response.raise_for_status()
    book = response.json()
    bids = book.get("bids", [])
    asks = book.get("asks", [])
    if not bids or not asks:
        return None
    best_bid = float(bids[0]["price"])
    best_ask = float(asks[0]["price"])
    return {
        "spread": round(best_ask - best_bid, 4),
        "best_bid": best_bid,
        "best_ask": best_ask,
    }


rows = []
for market in get_active_markets():
    try:
        outcome_prices = parse_json_or_list(market.get("outcomePrices"))
        if len(outcome_prices) < 2:
            continue
        midpoint_no = parse_float(outcome_prices[1], default=-1.0)
        if midpoint_no < 0.85:
            continue

        volume = parse_float(market.get("volumeNum"), default=0.0)
        if volume <= 10000:
            continue
        volume_24h = parse_float(market.get("volume24hr"), default=0.0)

        clob_token_ids = parse_json_or_list(market.get("clobTokenIds"))
        if not clob_token_ids:
            continue
        token_id = str(clob_token_ids[0])

        spread = get_spread(token_id)
        if spread is None:
            continue

        rows.append(
            {
                "question": str(market.get("question", ""))[:60],
                "midpoint_no": round(midpoint_no, 4),
                "volume": round(volume, 2),
                "volume_24h": round(volume_24h, 2),
                "spread": spread["spread"],
                "best_bid": spread["best_bid"],
                "best_ask": spread["best_ask"],
            }
        )
    except Exception:
        continue

rows.sort(key=lambda row: row["volume_24h"], reverse=True)
for row in rows[:10]:
    print(json.dumps(row))
