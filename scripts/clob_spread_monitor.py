import requests, json, time
from datetime import datetime

TEST_MARKETS = [
    "102936224134271070189104847090829839924697394514566827387181305960175107677216",
    "81662326158871781857247725348568394697379926716334270967994039975048021832777"
]

BASE = "https://clob-v2.polymarket.com"

def get_spread(token_id):
    r = requests.get(f"{BASE}/book", params={"token_id": token_id}, timeout=5)
    book = r.json()
    bids = book.get("bids", [])
    asks = book.get("asks", [])
    if not bids or not asks:
        return None
    best_bid = float(bids[0]["price"])
    best_ask = float(asks[0]["price"])
    return {
        "token_id": token_id[:8],
        "best_bid": best_bid,
        "best_ask": best_ask,
        "spread": round(best_ask - best_bid, 4),
        "midpoint": round((best_bid + best_ask) / 2, 4),
        "ts": datetime.utcnow().isoformat()
    }

for token_id in TEST_MARKETS:
    try:
        s = get_spread(token_id)
        if s:
            print(json.dumps(s))
    except Exception as e:
        print(f"Error {token_id[:8]}: {e}")
