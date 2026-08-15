#!/usr/bin/env python3
"""Inject one frozen Responses output limit without retaining request bodies."""

from __future__ import annotations

import argparse
import hashlib
import http.client
import json
import pathlib
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


LISTEN_HOST = "127.0.0.1"
LISTEN_PORT = 12080
UPSTREAM_HOST = "192.168.68.71"
UPSTREAM_PORT = 11999
MAX_REQUEST_BYTES = 16 * 1024 * 1024
MAX_OUTPUT_TOKENS = 65536
EXPECTED_MODEL = "qwen3.8:27b-mlx"
LIMIT_HEADER = "X-ReviewGraphen-Max-Output-Tokens"
PROTOCOL = "reviewgraphen.responses-limit-shaper.v1"
HOP_BY_HOP = {
    "connection",
    "content-length",
    "host",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
}


def digest(body: bytes) -> str:
    return "sha256:" + hashlib.sha256(body).hexdigest()


class Server(ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self, log_path: pathlib.Path):
        super().__init__((LISTEN_HOST, LISTEN_PORT), Handler)
        self.log_path = log_path

    def record(self, value: dict[str, object]) -> None:
        line = json.dumps(value, separators=(",", ":"), sort_keys=True) + "\n"
        with self.log_path.open("a", encoding="utf-8") as stream:
            stream.write(line)
            stream.flush()


class Handler(BaseHTTPRequestHandler):
    server: Server
    protocol_version = "HTTP/1.1"

    def do_GET(self) -> None:
        if self.path == "/healthz":
            body = b'{"status":"ready"}\n'
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        if self.path.rstrip("/") == "/v1/models":
            self.forward(b"")
            return
        self.send_error(404)

    def do_POST(self) -> None:
        if self.path.rstrip("/") != "/v1/responses":
            self.send_error(404)
            return
        length = int(self.headers.get("Content-Length", "-1"))
        if length < 0 or length > MAX_REQUEST_BYTES:
            self.send_error(413)
            return
        body = self.rfile.read(length)
        if len(body) != length:
            self.send_error(400)
            return
        try:
            value = json.loads(body)
        except json.JSONDecodeError:
            self.send_error(400)
            return
        if not isinstance(value, dict) or value.get("model") != EXPECTED_MODEL:
            self.send_error(400)
            return
        supplied = self.headers.get(LIMIT_HEADER)
        if supplied != str(MAX_OUTPUT_TOKENS):
            self.send_error(400)
            return
        prior = value.get("max_output_tokens")
        if prior is not None and prior != MAX_OUTPUT_TOKENS:
            self.send_error(409)
            return
        value["max_output_tokens"] = MAX_OUTPUT_TOKENS
        rewritten = json.dumps(value, separators=(",", ":"), ensure_ascii=False).encode()
        started = time.monotonic()
        status = self.forward(rewritten)
        self.server.record(
            {
                "schema": "reviewgraphen.benchmark.responses_limit_transport.v1",
                "protocol": PROTOCOL,
                "model": EXPECTED_MODEL,
                "model_context_window": 262144,
                "max_output_tokens": MAX_OUTPUT_TOKENS,
                "original_request_bytes": len(body),
                "original_request_sha256": digest(body),
                "rewritten_request_bytes": len(rewritten),
                "rewritten_request_sha256": digest(rewritten),
                "upstream_status": status,
                "elapsed_seconds": round(time.monotonic() - started, 3),
            }
        )

    def forward(self, body: bytes) -> int:
        headers = {
            name: value
            for name, value in self.headers.items()
            if name.lower() not in HOP_BY_HOP and name.lower() != LIMIT_HEADER.lower()
        }
        connection = http.client.HTTPConnection(UPSTREAM_HOST, UPSTREAM_PORT, timeout=None)
        try:
            connection.request(self.command, self.path, body=body or None, headers=headers)
            response = connection.getresponse()
            self.send_response(response.status)
            for name, value in response.getheaders():
                if name.lower() not in HOP_BY_HOP:
                    self.send_header(name, value)
            self.send_header("Connection", "close")
            self.end_headers()
            while chunk := response.read(65536):
                self.wfile.write(chunk)
                self.wfile.flush()
            self.close_connection = True
            return response.status
        except (OSError, http.client.HTTPException) as error:
            self.send_error(502, explain=type(error).__name__)
            return 502
        finally:
            connection.close()

    def log_message(self, format: str, *args: object) -> None:
        return


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--log", required=True, type=pathlib.Path)
    args = parser.parse_args()
    if args.log.exists() or not args.log.parent.is_dir():
        raise SystemExit("log path must be fresh under an existing directory")
    Server(args.log).serve_forever()


if __name__ == "__main__":
    main()
