"""Polymarket orderbook history ingester.

Downloads closed/open markets with NO midpoint >= 0.80 from the Polymarket
gamma API and stores them as JSONL partitioned by month under data/orderbook/.

Usage:
    python agents/services/polymarket_orderbook_history.py [--output-dir PATH]
                                                           [--min-no-midpoint 0.80]
                                                           [--min-volume 50000]
"""
from __future__ import annotations

import argparse
import json
import sys
import urllib.request
import urllib.error
import urllib.parse
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

GAMMA_BASE = "https://gamma-api.polymarket.com"
CLOB_BASE = "https://clob.polymarket.com"
DEFAULT_OUTPUT_DIR = Path("data/orderbook")
DEFAULT_MIN_NO_MIDPOINT = 0.80
DEFAULT_MIN_VOLUME = 50_000.0
DEFAULT_PAGE_SIZE = 100


def _get_json(url: str, timeout: int = 15) -> Any:
    req = urllib.request.Request(url, headers={"User-Agent": "maker-backtester/1.0"})
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        return json.loads(resp.read().decode("utf-8"))


def fetch_closed_markets(
    min_no_midpoint: float = DEFAULT_MIN_NO_MIDPOINT,
    min_volume: float = DEFAULT_MIN_VOLUME,
    page_size: int = DEFAULT_PAGE_SIZE,
    max_pages: int = 20,
) -> list[dict[str, Any]]:
    """Fetch closed markets from gamma API where NO midpoint >= min_no_midpoint."""
    results: list[dict[str, Any]] = []
    offset = 0

    for _ in range(max_pages):
        params = urllib.parse.urlencode({
            "closed": "true",
            "active": "false",
            "limit": page_size,
            "offset": offset,
        })
        url = f"{GAMMA_BASE}/markets?{params}"
        try:
            page = _get_json(url)
        except (urllib.error.URLError, json.JSONDecodeError, OSError):
            break

        if not isinstance(page, list):
            break

        for market in page:
            if not isinstance(market, dict):
                continue
            volume = float(market.get("volumeNum", 0) or market.get("volume", 0) or 0)
            if volume < min_volume:
                continue
            # Extract YES midpoint (market price for YES token)
            yes_mid = _extract_yes_midpoint(market)
            if yes_mid is None:
                continue
            no_mid = 1.0 - yes_mid
            if no_mid < min_no_midpoint:
                continue
            results.append(_normalize_market(market, yes_mid, no_mid))

        if len(page) < page_size:
            break
        offset += page_size

    return results


def fetch_clob_orderbook(token_id: str) -> dict[str, Any] | None:
    """Fetch current orderbook snapshot from CLOB API for a token."""
    url = f"{CLOB_BASE}/book?token_id={token_id}"
    try:
        data = _get_json(url, timeout=10)
        if isinstance(data, dict):
            return data
    except (urllib.error.URLError, json.JSONDecodeError, OSError):
        pass
    return None


def _extract_yes_midpoint(market: dict[str, Any]) -> float | None:
    """Extract YES token midpoint from a gamma market dict."""
    # Try best_bid / best_ask fields
    best_bid = market.get("bestBid") or market.get("best_bid")
    best_ask = market.get("bestAsk") or market.get("best_ask")
    if best_bid is not None and best_ask is not None:
        try:
            mid = (float(best_bid) + float(best_ask)) / 2.0
            if 0.0 < mid < 1.0:
                return mid
        except (ValueError, TypeError):
            pass
    # Try last_trade_price
    last_price = market.get("lastTradePrice") or market.get("last_trade_price")
    if last_price is not None:
        try:
            p = float(last_price)
            if 0.0 < p < 1.0:
                return p
        except (ValueError, TypeError):
            pass
    # Try outcome_prices for binary markets
    outcome_prices = market.get("outcomePrices")
    if isinstance(outcome_prices, list) and len(outcome_prices) >= 2:
        try:
            yes_price = float(outcome_prices[0])
            if 0.0 <= yes_price <= 1.0:
                return yes_price
        except (ValueError, TypeError):
            pass
    # Try tokens
    tokens = market.get("tokens")
    if isinstance(tokens, list):
        for token in tokens:
            if isinstance(token, dict) and token.get("outcome", "").upper() == "YES":
                try:
                    p = float(token.get("price", 0))
                    if 0.0 < p < 1.0:
                        return p
                except (ValueError, TypeError):
                    pass
    return None


def _normalize_market(
    market: dict[str, Any], yes_mid: float, no_mid: float
) -> dict[str, Any]:
    market_id = market.get("conditionId") or market.get("id") or ""
    question = market.get("question") or market.get("title") or ""
    volume = float(market.get("volumeNum", 0) or market.get("volume", 0) or 0)
    end_date = market.get("endDate") or market.get("end_date") or ""
    resolution = _parse_resolution(market)

    # Extract NO token ID for CLOB access
    no_token_id: str | None = None
    tokens = market.get("tokens")
    if isinstance(tokens, list):
        for token in tokens:
            if isinstance(token, dict) and token.get("outcome", "").upper() == "NO":
                no_token_id = token.get("token_id") or token.get("tokenId")
                break

    return {
        "market_id": market_id,
        "question": question,
        "yes_midpoint": round(yes_mid, 6),
        "no_midpoint": round(no_mid, 6),
        "volume_usdc": round(volume, 2),
        "end_date": end_date,
        "resolution": resolution,
        "no_token_id": no_token_id,
        "ingested_at": datetime.now(timezone.utc).isoformat(),
    }


def _parse_resolution(market: dict[str, Any]) -> str | None:
    outcome = market.get("outcome") or market.get("winner") or market.get("resolution")
    if isinstance(outcome, str):
        o = outcome.lower()
        if o in ("yes", "true", "1"):
            return "YES"
        if o in ("no", "false", "0"):
            return "NO"
    outcome_prices = market.get("outcomePrices")
    if isinstance(outcome_prices, list) and len(outcome_prices) >= 2:
        try:
            yes_price = float(outcome_prices[0])
            if yes_price >= 0.95:
                return "YES"
            if yes_price <= 0.05:
                return "NO"
        except (ValueError, TypeError):
            pass
    return None


def _month_key(market: dict[str, Any]) -> str:
    """Return YYYY-MM for partitioning. Uses ingested_at."""
    ts = market.get("ingested_at", "")
    if ts:
        try:
            dt = datetime.fromisoformat(ts.replace("Z", "+00:00"))
            return dt.strftime("%Y-%m")
        except ValueError:
            pass
    return datetime.now(timezone.utc).strftime("%Y-%m")


def _already_ingested(output_dir: Path, market_id: str, month: str) -> bool:
    """Check if market_id already exists in the monthly JSONL file."""
    fpath = output_dir / f"{month}.jsonl"
    if not fpath.exists():
        return False
    for line in fpath.read_text(encoding="utf-8").splitlines():
        if not line.strip():
            continue
        try:
            obj = json.loads(line)
            if obj.get("market_id") == market_id:
                return True
        except json.JSONDecodeError:
            pass
    return False


def ingest_markets(
    markets: list[dict[str, Any]],
    output_dir: Path,
) -> dict[str, int]:
    """Write markets to monthly JSONL files. Returns {ingested: N, skipped: N}."""
    output_dir.mkdir(parents=True, exist_ok=True)
    ingested = skipped = 0

    for market in markets:
        market_id = market.get("market_id", "")
        month = _month_key(market)
        if _already_ingested(output_dir, market_id, month):
            skipped += 1
            continue
        fpath = output_dir / f"{month}.jsonl"
        with fpath.open("a", encoding="utf-8") as fh:
            fh.write(json.dumps(market) + "\n")
        ingested += 1

    return {"ingested": ingested, "skipped": skipped}


def load_markets_from_dir(
    output_dir: Path,
    min_no_midpoint: float = DEFAULT_MIN_NO_MIDPOINT,
) -> list[dict[str, Any]]:
    """Load all ingested markets from the JSONL directory."""
    markets: list[dict[str, Any]] = []
    if not output_dir.exists():
        return markets
    for fpath in sorted(output_dir.glob("*.jsonl")):
        for line in fpath.read_text(encoding="utf-8").splitlines():
            line = line.strip()
            if not line:
                continue
            try:
                market = json.loads(line)
                if market.get("no_midpoint", 0) >= min_no_midpoint:
                    markets.append(market)
            except json.JSONDecodeError:
                pass
    return markets


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Ingest Polymarket orderbook history.")
    parser.add_argument("--output-dir", default=str(DEFAULT_OUTPUT_DIR), dest="output_dir")
    parser.add_argument("--min-no-midpoint", type=float, default=DEFAULT_MIN_NO_MIDPOINT, dest="min_no_midpoint")
    parser.add_argument("--min-volume", type=float, default=DEFAULT_MIN_VOLUME, dest="min_volume")
    parser.add_argument("--max-pages", type=int, default=20, dest="max_pages")
    parser.add_argument("--dry-run", action="store_true", dest="dry_run")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    output_dir = Path(args.output_dir)

    print(f"[orderbook_history] Fetching closed markets NO>={args.min_no_midpoint} vol>={args.min_volume}")
    markets = fetch_closed_markets(
        min_no_midpoint=args.min_no_midpoint,
        min_volume=args.min_volume,
        max_pages=args.max_pages,
    )
    print(f"[orderbook_history] Fetched {len(markets)} qualifying markets")

    if args.dry_run:
        print("[orderbook_history] Dry run — not writing")
        return 0

    stats = ingest_markets(markets, output_dir)
    print(f"[orderbook_history] Ingested {stats['ingested']}, skipped {stats['skipped']} (already present)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
