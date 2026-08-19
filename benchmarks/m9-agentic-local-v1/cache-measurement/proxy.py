#!/usr/bin/env python3
"""Logging pass-through proxy: records exactly what Claude Code sends.

Listens on 127.0.0.1:11998, forwards verbatim to the cch proxy on
192.168.68.71:11999, and writes each outbound request body to
/tmp/m9cache/requests/NNN.json plus a timing line.

Nothing is modified. The point is to diff consecutive turns' request
prefixes byte-for-byte and find the first divergence.
"""

import http.server
import json
import time
import urllib.request
from pathlib import Path

UPSTREAM = "http://192.168.68.71:11999"
OUT = Path("/tmp/m9cache/requests")
OUT.mkdir(parents=True, exist_ok=True)
LOG = Path("/tmp/m9cache/proxy.log")
counter = {"n": 0}


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass

    def _proxy(self, method):
        length = int(self.headers.get("Content-Length") or 0)
        body = self.rfile.read(length) if length else b""
        counter["n"] += 1
        index = counter["n"]
        if body:
            (OUT / f"{index:03d}.json").write_bytes(body)

        headers = {
            k: v
            for k, v in self.headers.items()
            if k.lower() not in ("host", "content-length", "connection", "accept-encoding")
        }
        req = urllib.request.Request(
            UPSTREAM + self.path, data=body or None, headers=headers, method=method
        )
        t0 = time.monotonic()
        try:
            with urllib.request.urlopen(req, timeout=3600) as r:
                payload = r.read()
                status = r.status
                ctype = r.headers.get("Content-Type", "application/json")
        except Exception as e:  # noqa: BLE001
            payload = json.dumps({"error": str(e)}).encode()
            status = 502
            ctype = "application/json"
        dt = time.monotonic() - t0
        with LOG.open("a") as fh:
            fh.write(
                f"{index:03d} {method} {self.path} bytes_in={len(body)} "
                f"status={status} elapsed={dt:.2f}s\n"
            )
        (OUT / f"{index:03d}.resp").write_bytes(payload[:200000])

        self.send_response(status)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def do_POST(self):
        self._proxy("POST")

    def do_GET(self):
        self._proxy("GET")


if __name__ == "__main__":
    http.server.ThreadingHTTPServer(("127.0.0.1", 11998), Handler).serve_forever()
