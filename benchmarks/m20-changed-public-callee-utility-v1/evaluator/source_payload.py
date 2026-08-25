"""Exact source excerpt extraction and payload closure."""
from typing import Any
from .canonical import sha256_bytes, stable_id


def extract(file_bytes: bytes, start_line: int, end_line: int) -> dict:
    if start_line < 1 or end_line < start_line:
        return {"status": "obstruction", "obstruction_kind": "span_invalid"}
    try:
        file_bytes.decode("utf-8", "strict")
    except UnicodeDecodeError:
        return {"status": "obstruction", "obstruction_kind": "non_utf8_source"}
    starts = [0]
    for index, byte in enumerate(file_bytes):
        if byte == 10 and index + 1 < len(file_bytes):
            starts.append(index + 1)
    line_count = 0 if not file_bytes else len(starts)
    if start_line > line_count or end_line > line_count:
        return {"status": "obstruction", "obstruction_kind": "span_out_of_bounds"}
    start = starts[start_line - 1]
    end = file_bytes.find(b"\n", starts[end_line - 1])
    end = len(file_bytes) if end < 0 else end + 1
    excerpt = file_bytes[start:end]
    text = excerpt.decode("utf-8", "strict")
    digest = sha256_bytes(excerpt)
    return {"status": "payload", "bytes": excerpt, "text": text, "byte_length": len(excerpt), "sha256": digest,
            "payload_id": stable_id("source-payload", {"sha256": digest, "byte_length": len(excerpt)})}


def payload_record(extracted: dict) -> dict:
    if extracted.get("status") != "payload":
        raise ValueError("not_payload")
    return {"payload_id": extracted["payload_id"], "encoding": "utf-8", "media_type": "text/x-rust; charset=utf-8",
            "byte_length": extracted["byte_length"], "sha256": extracted["sha256"], "text": extracted["text"]}


def source_record(role: str, snapshot_side: str, path: str, start_line: int, end_line: int, extracted: dict) -> dict:
    payload = payload_record(extracted)
    body = {"role": role, "snapshot_side": snapshot_side, "path": path,
            "range": {"start_line": start_line, "end_line": end_line}, "payload_id": payload["payload_id"],
            "bytes": payload["byte_length"], "sha256": payload["sha256"]}
    return {"source_id": stable_id("source", body), **body}


def validate_payload_closure(packet: dict) -> list[str]:
    failures = []
    payloads = packet.get("payloads")
    inventory = packet.get("source_inventory", {})
    if not isinstance(payloads, list):
        return ["payload_missing"]
    by_id = {}
    for payload in payloads:
        try:
            data = payload["text"].encode("utf-8", "strict")
            expected = stable_id("source-payload", {"sha256": sha256_bytes(data), "byte_length": len(data)})
            if payload.get("sha256") != sha256_bytes(data) or payload.get("byte_length") != len(data) or payload.get("payload_id") != expected:
                failures.append("payload_hash_mismatch")
            if payload.get("encoding") != "utf-8" or payload.get("media_type") != "text/x-rust; charset=utf-8" or len(data) < 1:
                failures.append("source_payload_closure_invalid")
            by_id[payload.get("payload_id")] = payload
        except (KeyError, UnicodeError, AttributeError):
            failures.append("source_payload_closure_invalid")
    used = set()
    for source in inventory.get("admitted_sources", []):
        payload = by_id.get(source.get("payload_id"))
        if payload is None:
            failures.append("payload_missing")
            continue
        used.add(source["payload_id"])
        if source.get("bytes") != payload.get("byte_length") or source.get("sha256") != payload.get("sha256"):
            failures.append("payload_length_mismatch")
        try:
            line_range = source["range"]
            if set(line_range) != {"start_line", "end_line"} or line_range["start_line"] < 1 or line_range["end_line"] < line_range["start_line"]:
                failures.append("source_payload_closure_invalid")
            expected = stable_id("source", {"role": source["role"], "snapshot_side": source["snapshot_side"], "path": source["path"],
                                              "range": line_range, "payload_id": source["payload_id"], "bytes": source["bytes"], "sha256": source["sha256"]})
            if source.get("source_id") != expected or source["role"] not in {"changed", "context", "support"} or source["snapshot_side"] not in {"base", "head"}:
                failures.append("source_payload_closure_invalid")
        except (KeyError, TypeError):
            failures.append("source_payload_closure_invalid")
    if set(by_id) != used:
        failures.append("source_payload_closure_invalid")
    return sorted(set(failures))
