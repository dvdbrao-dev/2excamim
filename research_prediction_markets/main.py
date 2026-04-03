from __future__ import annotations

import json
import sys
from pathlib import Path

import pandas as pd

if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from research_prediction_markets.features.mvp_features import compute_mvp_features
from research_prediction_markets.ingestion.ingest import fetch_all_markets, fetch_all_trades
from research_prediction_markets.signals.mvp_signals import generate_mvp_signals

OUTPUT_COLUMNS = [
    "market_id",
    "timestamp",
    "signal_name",
    "strength",
    "direction",
    "probability",
    "spread_tight",
    "volume_spike_24h",
    "price_deviation_vwap_1h",
    "source",
    "metadata",
]


def build_signals_dataframe() -> pd.DataFrame:
    markets = fetch_all_markets()
    trades = fetch_all_trades()
    features = compute_mvp_features(markets, trades)
    signals = generate_mvp_signals(markets, features)

    market_map = {market.market_id: market for market in markets}
    feature_values: dict[str, dict[str, float]] = {}
    for feature in features:
        feature_values.setdefault(feature.market_id, {})[feature.feature_name] = feature.value

    rows = []
    for signal in signals:
        market = market_map.get(signal.market_id)
        if market is None:
            continue
        values = feature_values.get(signal.market_id, {})
        rows.append(
            {
                "market_id": signal.market_id,
                "timestamp": signal.timestamp,
                "signal_name": signal.signal_name,
                "strength": signal.strength,
                "direction": signal.direction,
                "probability": market.probability,
                "spread_tight": values.get("spread_tight"),
                "volume_spike_24h": values.get("volume_spike_24h"),
                "price_deviation_vwap_1h": values.get("price_deviation_vwap_1h"),
                "source": market.source,
                "metadata": json.dumps(signal.metadata, sort_keys=True, ensure_ascii=False),
            }
        )

    return pd.DataFrame(rows, columns=OUTPUT_COLUMNS)


def write_latest_signals_parquet(dataframe: pd.DataFrame) -> Path:
    output_path = Path(__file__).resolve().parent / "output" / "signals" / "latest_signals.parquet"
    output_path.parent.mkdir(parents=True, exist_ok=True)
    try:
        dataframe.to_parquet(output_path, index=False)
    except ImportError as exc:
        # Pandas needs an optional parquet engine; keep the failure explicit.
        raise RuntimeError("Parquet support requires pyarrow or fastparquet in the runtime environment") from exc
    return output_path


def main() -> Path:
    dataframe = build_signals_dataframe()
    if dataframe.empty:
        dataframe = pd.DataFrame(columns=OUTPUT_COLUMNS)
    return write_latest_signals_parquet(dataframe)


if __name__ == "__main__":
    result_path = main()
    print(result_path)
