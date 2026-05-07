from __future__ import annotations

import json
import time
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass
from typing import Any


@dataclass(frozen=True)
class HttpResult:
    ok: bool
    status_code: int | None
    data: Any | None
    error: str | None
    latency_ms: int
    url: str


class HttpJsonClient:
    def __init__(self, timeout_sec: float = 5.0, max_retries: int = 2) -> None:
        self.timeout_sec = max(0.1, timeout_sec)
        self.max_retries = max(0, max_retries)

    def get_json(self, url: str, params: dict[str, str] | None = None) -> HttpResult:
        query = urllib.parse.urlencode(params or {})
        final_url = f"{url}?{query}" if query else url
        last_error = "unknown_error"
        start = time.perf_counter()

        for attempt in range(self.max_retries + 1):
            try:
                req = urllib.request.Request(final_url, headers={"Accept": "application/json"}, method="GET")
                with urllib.request.urlopen(req, timeout=self.timeout_sec) as response:
                    status_code = getattr(response, "status", None)
                    body = response.read()
                    data = json.loads(body.decode("utf-8"))
                    latency = int((time.perf_counter() - start) * 1000)
                    return HttpResult(True, status_code, data, None, latency, final_url)
            except urllib.error.HTTPError as err:
                last_error = f"http_error:{err.code}"
                if 400 <= err.code < 500:
                    break
            except urllib.error.URLError as err:
                last_error = f"url_error:{err.reason}"
            except TimeoutError:
                last_error = "timeout"
            except json.JSONDecodeError:
                last_error = "invalid_json"
            except Exception as err:  # pragma: no cover
                last_error = f"unexpected:{type(err).__name__}"

            if attempt < self.max_retries:
                time.sleep(min(0.25 * (attempt + 1), 1.0))

        latency = int((time.perf_counter() - start) * 1000)
        return HttpResult(False, None, None, last_error, latency, final_url)
