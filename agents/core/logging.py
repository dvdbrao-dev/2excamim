from __future__ import annotations

import json
from typing import Any


def emit_json(payload: dict[str, Any]) -> None:
    print(json.dumps(payload, separators=(",", ":"), sort_keys=True))

