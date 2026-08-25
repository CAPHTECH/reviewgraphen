"""Restricted canonical JSON and stable identifiers."""
import base64
import hashlib
import json
from typing import Any

MAX_INTEGER = (2 ** 53) - 1


class CanonicalError(ValueError):
    pass


def _validate(value: Any) -> None:
    if value is None or isinstance(value, bool):
        return
    if isinstance(value, int):
        if not -MAX_INTEGER <= value <= MAX_INTEGER:
            raise CanonicalError("integer_out_of_range")
        return
    if isinstance(value, float):
        raise CanonicalError("float_forbidden")
    if isinstance(value, str):
        if "\x00" in value or any(0xD800 <= ord(c) <= 0xDFFF for c in value):
            raise CanonicalError("invalid_string_codepoint")
        return
    if isinstance(value, list):
        for item in value:
            _validate(item)
        return
    if isinstance(value, dict):
        for key, item in value.items():
            if not isinstance(key, str) or not key.isascii():
                raise CanonicalError("object_key_not_ascii")
            _validate(item)
        return
    raise CanonicalError("unsupported_json_value")


def canonical_bytes(value: Any) -> bytes:
    _validate(value)
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"), allow_nan=False).encode("utf-8")


def sha256_bytes(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def hash_json(value: Any) -> str:
    return sha256_bytes(canonical_bytes(value))


def stable_id(kind: str, value: Any) -> str:
    if not kind or ":" in kind:
        raise CanonicalError("invalid_stable_id_kind")
    return kind + ":" + hash_json(value)


def canonical_b64_decode(value: str) -> bytes:
    if not isinstance(value, str) or len(value) % 4:
        raise CanonicalError("base64_invalid")
    try:
        decoded = base64.b64decode(value.encode("ascii"), validate=True)
    except (UnicodeEncodeError, ValueError) as error:
        raise CanonicalError("base64_invalid") from error
    if base64.b64encode(decoded).decode("ascii") != value:
        raise CanonicalError("base64_invalid")
    return decoded


def canonical_b64_encode(data: bytes) -> str:
    return base64.b64encode(data).decode("ascii")


def parse_json_bytes(data: bytes) -> Any:
    def no_duplicates(pairs):
        out = {}
        for key, value in pairs:
            if key in out:
                raise CanonicalError("duplicate_json_key")
            out[key] = value
        return out
    try:
        value = json.loads(data.decode("utf-8"), object_pairs_hook=no_duplicates,
                           parse_float=lambda _: (_ for _ in ()).throw(CanonicalError("float_forbidden")))
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise CanonicalError("malformed_json") from error
    _validate(value)
    return value
