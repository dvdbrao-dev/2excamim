import json, pathlib, re, sys

pathlib.Path("var/market-watch/raw").mkdir(parents=True, exist_ok=True)


def repair_jsonl(path: pathlib.Path) -> int:
    if not path.exists():
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("")
        return 0
    lines = path.read_bytes().decode("utf-8", errors="replace").splitlines()
    valid = []
    for l in lines:
        if not l.strip():
            continue
        try:
            cleaned = re.sub(r"[\x00-\x1f\x7f]", "", l)
            json.loads(cleaned)
            valid.append(cleaned)
        except:
            pass
    path.write_text("\n".join(valid) + "\n", encoding="utf-8")
    return len(valid)


files = [
    "var/market-watch/snapshots.jsonl",
    "var/market-watch/raw/polymarket-discovery.jsonl",
    "var/market_snapshots.jsonl",
    "var/events.jsonl",
]

for f in files:
    p = pathlib.Path(f)
    n = repair_jsonl(p)
    pathlib.Path("var/market-watch/raw").mkdir(parents=True, exist_ok=True)
    print(f"repaired {f}: {n} valid lines")
