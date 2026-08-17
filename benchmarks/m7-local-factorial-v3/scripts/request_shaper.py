#!/usr/bin/env python3
"""Inject one frozen Responses output limit and retain every provider response."""

from __future__ import annotations

import argparse
import gzip
import hashlib
import http.client
import json
import pathlib
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


LISTEN_HOST = "127.0.0.1"
LISTEN_PORT = 12080
UPSTREAM_HOST = "192.168.68.71"
UPSTREAM_PORT = 11999
MAX_REQUEST_BYTES = 16 * 1024 * 1024
MAX_RESPONSE_BYTES_FOR_METRICS = 32 * 1024 * 1024
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


def digest_file(path: pathlib.Path) -> str:
    value = hashlib.sha256()
    with path.open("rb") as stream:
        while chunk := stream.read(1024 * 1024):
            value.update(chunk)
    return "sha256:" + value.hexdigest()


def completed_metrics(body: bytes) -> dict[str, int | str | None]:
    observed: dict[str, object] | None = None
    reasoning_delta_events = 0
    reasoning_delta_bytes = 0
    output_text_delta_events = 0
    output_text_delta_bytes = 0
    for line in body.splitlines():
        if not line.startswith(b"data:"):
            continue
        payload = line[5:].strip()
        if not payload or payload == b"[DONE]":
            continue
        try:
            event = json.loads(payload)
        except json.JSONDecodeError:
            continue
        if not isinstance(event, dict):
            continue
        event_type = event.get("type")
        delta = event.get("delta")
        if isinstance(event_type, str) and isinstance(delta, str):
            if event_type.startswith("response.reasoning_"):
                reasoning_delta_events += 1
                reasoning_delta_bytes += len(delta.encode("utf-8"))
            elif event_type == "response.output_text.delta":
                output_text_delta_events += 1
                output_text_delta_bytes += len(delta.encode("utf-8"))
        candidates = [event, event.get("response")]
        for candidate in candidates:
            if isinstance(candidate, dict) and isinstance(candidate.get("usage"), dict):
                observed = candidate["usage"]
    if observed is None:
        return {
            "provider_input_tokens": None,
            "cached_input_tokens": None,
            "provider_output_tokens": None,
            "provider_reported_reasoning_tokens": None,
            "thinking_tokens": None,
            "provider_non_reasoning_output_tokens": None,
            "final_content_tokens": None,
            "token_attribution": "unavailable_without_completed_usage",
            "reasoning_delta_events": reasoning_delta_events,
            "reasoning_delta_utf8_bytes": reasoning_delta_bytes,
            "output_text_delta_events": output_text_delta_events,
            "output_text_delta_utf8_bytes": output_text_delta_bytes,
        }
    input_tokens = observed.get("input_tokens")
    input_details = observed.get("input_tokens_details")
    cached_tokens = input_details.get("cached_tokens") if isinstance(input_details, dict) else None
    output_tokens = observed.get("output_tokens")
    details = observed.get("output_tokens_details")
    reasoning_tokens = details.get("reasoning_tokens") if isinstance(details, dict) else None
    input_tokens = input_tokens if isinstance(input_tokens, int) and input_tokens >= 0 else None
    cached_tokens = cached_tokens if isinstance(cached_tokens, int) and cached_tokens >= 0 else None
    output_tokens = output_tokens if isinstance(output_tokens, int) and output_tokens >= 0 else None
    reasoning_tokens = (
        reasoning_tokens if isinstance(reasoning_tokens, int) and reasoning_tokens >= 0 else None
    )
    provider_non_reasoning_tokens = (
        output_tokens - reasoning_tokens
        if output_tokens is not None
        and reasoning_tokens is not None
        and output_tokens >= reasoning_tokens
        else None
    )
    thinking_tokens = None
    final_content_tokens = None
    token_attribution = "ambiguous_mixed_or_absent_content_events"
    if output_tokens is not None and reasoning_delta_events > 0 and output_text_delta_events == 0:
        thinking_tokens = output_tokens
        final_content_tokens = 0
        token_attribution = "all_provider_output_attributed_to_reasoning_events"
    elif output_tokens is not None and output_text_delta_events > 0 and reasoning_delta_events == 0:
        thinking_tokens = 0
        final_content_tokens = output_tokens
        token_attribution = "all_provider_output_attributed_to_output_text_events"
    return {
        "provider_input_tokens": input_tokens,
        "cached_input_tokens": cached_tokens,
        "provider_output_tokens": output_tokens,
        "provider_reported_reasoning_tokens": reasoning_tokens,
        "thinking_tokens": thinking_tokens,
        "provider_non_reasoning_output_tokens": provider_non_reasoning_tokens,
        "final_content_tokens": final_content_tokens,
        "token_attribution": token_attribution,
        "reasoning_delta_events": reasoning_delta_events,
        "reasoning_delta_utf8_bytes": reasoning_delta_bytes,
        "output_text_delta_events": output_text_delta_events,
        "output_text_delta_utf8_bytes": output_text_delta_bytes,
    }


class Server(ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self, log_path: pathlib.Path, capture_dir: pathlib.Path):
        super().__init__((LISTEN_HOST, LISTEN_PORT), Handler)
        self.log_path = log_path
        self.capture_dir = capture_dir
        self.capture_lock = threading.Lock()
        self.capture_sequence = 0
        self.failure_lock = threading.Lock()
        self.upstream_failure_seen = False

    def next_capture(self) -> tuple[int, pathlib.Path, pathlib.Path]:
        with self.capture_lock:
            self.capture_sequence += 1
            sequence = self.capture_sequence
        stem = self.capture_dir / f"response-{sequence:06d}"
        return sequence, stem.with_suffix(".sse.partial"), stem.with_suffix(".sse.gz")

    def record(self, value: dict[str, object]) -> None:
        line = json.dumps(value, separators=(",", ":"), sort_keys=True) + "\n"
        with self.log_path.open("a", encoding="utf-8") as stream:
            stream.write(line)
            stream.flush()

    def may_forward(self) -> bool:
        with self.failure_lock:
            return not self.upstream_failure_seen

    def mark_upstream_failure(self) -> None:
        with self.failure_lock:
            self.upstream_failure_seen = True


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
        if not self.server.may_forward():
            self.send_error(503, explain="prior upstream failure; retry not forwarded")
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
        sequence, partial_path, capture_path = self.server.next_capture()
        observation = self.forward(rewritten, partial_path, capture_path)
        if int(observation["status"]) >= 500:
            self.server.mark_upstream_failure()
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
                "upstream_status": observation["status"],
                "upstream_response_bytes": observation["response_bytes"],
                "upstream_response_sha256": observation["response_sha256"],
                "raw_response_artifact": capture_path.name,
                "raw_response_compressed_bytes": capture_path.stat().st_size,
                "raw_response_compressed_sha256": digest_file(capture_path),
                "response_complete": observation["response_complete"],
                "request_sequence": sequence,
                "elapsed_seconds": round(time.monotonic() - started, 3),
                **observation["usage"],
            }
        )

    def forward(
        self, body: bytes, partial_path: pathlib.Path, capture_path: pathlib.Path
    ) -> dict[str, object]:
        headers = {
            name: value
            for name, value in self.headers.items()
            if name.lower() not in HOP_BY_HOP and name.lower() != LIMIT_HEADER.lower()
        }
        connection = http.client.HTTPConnection(UPSTREAM_HOST, UPSTREAM_PORT, timeout=None)
        response_body = bytearray()
        response_hash = hashlib.sha256()
        response_bytes = 0
        response_complete = False
        response_status = 502
        client_connected = True
        try:
            connection.request(self.command, self.path, body=body or None, headers=headers)
            response = connection.getresponse()
            response_status = response.status
            self.send_response(response.status)
            for name, value in response.getheaders():
                if name.lower() not in HOP_BY_HOP:
                    self.send_header(name, value)
            self.send_header("Connection", "close")
            self.end_headers()
            with partial_path.open("xb") as archive:
                while chunk := response.read(65536):
                    response_bytes += len(chunk)
                    response_hash.update(chunk)
                    archive.write(chunk)
                    archive.flush()
                    if len(response_body) + len(chunk) <= MAX_RESPONSE_BYTES_FOR_METRICS:
                        response_body.extend(chunk)
                    if client_connected:
                        try:
                            self.wfile.write(chunk)
                            self.wfile.flush()
                        except (BrokenPipeError, ConnectionResetError):
                            # A disconnected consumer must not make the provider
                            # response disappear from the audit record.
                            client_connected = False
            with partial_path.open("rb") as source, capture_path.open("xb") as compressed:
                with gzip.GzipFile(fileobj=compressed, mode="wb", mtime=0) as archive:
                    while chunk := source.read(1024 * 1024):
                        archive.write(chunk)
            partial_path.unlink()
            response_complete = True
            self.close_connection = True
            usage = (
                completed_metrics(bytes(response_body))
                if response_bytes == len(response_body)
                else completed_metrics(b"")
            )
            return {
                "status": response_status,
                "response_bytes": response_bytes,
                "response_sha256": "sha256:" + response_hash.hexdigest(),
                "response_complete": response_complete,
                "usage": usage,
            }
        except (OSError, http.client.HTTPException) as error:
            if not capture_path.exists():
                if not partial_path.exists():
                    partial_path.touch(mode=0o600, exist_ok=False)
                with partial_path.open("rb") as source, capture_path.open("xb") as compressed:
                    with gzip.GzipFile(fileobj=compressed, mode="wb", mtime=0) as archive:
                        while chunk := source.read(1024 * 1024):
                            archive.write(chunk)
                partial_path.unlink()
            if client_connected:
                try:
                    self.send_error(502, explain=type(error).__name__)
                except (BrokenPipeError, ConnectionResetError):
                    pass
            return {
                "status": response_status,
                "response_bytes": response_bytes,
                "response_sha256": "sha256:" + response_hash.hexdigest(),
                "response_complete": response_complete,
                "usage": completed_metrics(b""),
            }
        finally:
            connection.close()

    def log_message(self, format: str, *args: object) -> None:
        return


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--log", required=True, type=pathlib.Path)
    parser.add_argument("--capture-dir", required=True, type=pathlib.Path)
    args = parser.parse_args()
    if args.log.exists() or not args.log.parent.is_dir():
        raise SystemExit("log path must be fresh under an existing directory")
    if args.capture_dir.exists() or not args.capture_dir.parent.is_dir():
        raise SystemExit("capture directory must be fresh under an existing directory")
    args.capture_dir.mkdir(mode=0o700)
    args.log.touch(mode=0o600, exist_ok=False)
    Server(args.log, args.capture_dir).serve_forever()


if __name__ == "__main__":
    main()
