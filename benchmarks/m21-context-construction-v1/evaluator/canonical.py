import hashlib
import json
from typing import Any


class CanonicalError(ValueError):
    pass


def _check(value: Any) -> None:
    if value is None or isinstance(value, (bool, str)):
        if isinstance(value, str) and ("\x00" in value or any(0xD800 <= ord(c) <= 0xDFFF for c in value)):
            raise CanonicalError("invalid_string")
        return
    if isinstance(value, int) and -(2**53 - 1) <= value <= 2**53 - 1:
        return
    if isinstance(value, float):
        raise CanonicalError("float_forbidden")
    if isinstance(value, list):
        for item in value: _check(item)
        return
    if isinstance(value, dict) and all(isinstance(k, str) and k.isascii() for k in value):
        for item in value.values(): _check(item)
        return
    raise CanonicalError("unsupported_json_value")


def canonical_bytes(value: Any) -> bytes:
    _check(value)
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()


def sha256(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def hash_json(value: Any) -> str:
    return sha256(canonical_bytes(value))


def stable_id(kind: str, *parts: str) -> str:
    if not kind or ":" in kind: raise CanonicalError("invalid_id_kind")
    body = "\x00".join(parts).encode()
    return kind + ":" + hashlib.sha256(body).hexdigest()


def load(path):
    def pairs(rows):
        out = {}
        for key, value in rows:
            if key in out: raise CanonicalError("duplicate_json_key")
            out[key] = value
        return out
    return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=pairs,
                      parse_float=lambda _: (_ for _ in ()).throw(CanonicalError("float_forbidden")))


def parse_json_bytes(data: bytes):
    try:
        text = data.decode("utf-8", "strict")
    except UnicodeDecodeError as error:
        raise CanonicalError("malformed_json") from error
    def pairs(rows):
        out = {}
        for key, value in rows:
            if key in out: raise CanonicalError("duplicate_json_key")
            out[key] = value
        return out
    try:
        value = json.loads(text, object_pairs_hook=pairs,
                           parse_float=lambda _: (_ for _ in ()).throw(CanonicalError("float_forbidden")))
    except json.JSONDecodeError as error:
        raise CanonicalError("malformed_json") from error
    _check(value)
    return value


def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(canonical_bytes(value))
