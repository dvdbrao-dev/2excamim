#!/usr/bin/env python3
import http.server, json, subprocess, time, os
from pathlib import Path

BASE = Path(__file__).parent
DEBOUNCE = 30
last_refresh = 0

class Handler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=str(BASE), **kwargs)

    def do_GET(self):
        global last_refresh
        if self.path == "/api/refresh":
            now = time.time()
            if now - last_refresh > DEBOUNCE:
                subprocess.run(["bash", str(BASE / "refresh.sh")])
                last_refresh = now
            data = (BASE / "data.json").read_text()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Access-Control-Allow-Origin", "*")
            self.end_headers()
            self.wfile.write(data.encode())
        else:
            super().do_GET()

    def log_message(self, fmt, *args):
        pass

if __name__ == "__main__":
    os.chdir(BASE)
    port = int(os.environ.get("DASHBOARD_PORT", "8765"))
    print(f"EXCAMIM Dashboard → http://0.0.0.0:{port}")
    http.server.HTTPServer(("0.0.0.0", port), Handler).serve_forever()
