from __future__ import annotations

import json
import math
import sys
from pathlib import Path

import pandas as pd

REQUIRED_COLUMNS = {
    "market_id",
    "timestamp",
    "signal_name",
    "strength",
    "direction",
    "source",
}


def normalize_value(value):
    if pd.isna(value):
        return None
    if hasattr(value, "isoformat"):
        return value.isoformat().replace("+00:00", "Z")
    if isinstance(value, float) and (math.isinf(value) or math.isnan(value)):
        return None
    return value


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: export_signals_json.py <signals.parquet>", file=sys.stderr)
        return 2

    input_path = Path(sys.argv[1])
    if not input_path.exists():
        print(f"input file {input_path} does not exist", file=sys.stderr)
        return 1

    dataframe = pd.read_parquet(input_path)
    missing_columns = sorted(REQUIRED_COLUMNS.difference(dataframe.columns))
    if missing_columns:
        print(
            f"missing required columns: {', '.join(missing_columns)}",
            file=sys.stderr,
        )
        return 1

    for row_number, (_, row) in enumerate(dataframe.iterrows(), start=1):
        record = {column: normalize_value(value) for column, value in row.items()}
        record["row_number"] = row_number
        print(json.dumps(record, ensure_ascii=True, sort_keys=True))

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
