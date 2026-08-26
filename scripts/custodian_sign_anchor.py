#!/usr/bin/env python3
"""Custodian-only Ed25519 signer for the external m20 Stage 0 anchor."""
from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
import stat
from datetime import datetime, timezone
from pathlib import Path

from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey


def canonical(value: object) -> bytes:
    return json.dumps(value, ensure_ascii=False, allow_nan=False, sort_keys=True, separators=(",", ":")).encode("utf-8")


def sha256(raw: bytes) -> str:
    return "sha256:" + hashlib.sha256(raw).hexdigest()


def pinned_hash(value: str) -> str:
    if len(value) != 71 or not value.startswith("sha256:") or any(character not in "0123456789abcdef" for character in value[7:]):
        raise ValueError("sha256_pin_invalid")
    return value


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--private-key", required=True)
    parser.add_argument("--stage0-root", required=True)
    parser.add_argument("--stage0-manifest-sha256", required=True)
    parser.add_argument("--preregistration-freeze-sha256", required=True)
    parser.add_argument("--preregistration-sha256", required=True)
    parser.add_argument("--signed-at-utc")
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    private_path=Path(args.private_key); root=Path(args.stage0_root); output=Path(args.output)
    status=private_path.lstat()
    if private_path.is_symlink() or not stat.S_ISREG(status.st_mode) or stat.S_IMODE(status.st_mode)!=0o600: raise ValueError("custodian_private_key_not_0600_regular")
    if not root.is_absolute() or not output.is_absolute() or output.exists() or output.is_symlink() or not output.parent.is_dir(): raise ValueError("custodian_anchor_path_invalid")
    key=serialization.load_pem_private_key(private_path.read_bytes(),password=None)
    if not isinstance(key,Ed25519PrivateKey): raise ValueError("custodian_private_key_not_ed25519")
    signed_at=args.signed_at_utc or datetime.now(timezone.utc).replace(microsecond=0).strftime("%Y-%m-%dT%H:%M:%SZ")
    datetime.strptime(signed_at,"%Y-%m-%dT%H:%M:%SZ")
    body={"schema":"m20.custodian-stage0-anchor.v2","experiment_id":"m20-changed-public-callee-utility-v1","stage":"stage0","stage0_root":str(root),"stage0_artifact_manifest_sha256":pinned_hash(args.stage0_manifest_sha256),"signed_at_utc":signed_at,"preregistration_freeze_sha256":pinned_hash(args.preregistration_freeze_sha256),"preregistration_sha256":pinned_hash(args.preregistration_sha256)}
    anchor={**body,"custodian_ed25519_signature_base64":base64.b64encode(key.sign(canonical(body))).decode("ascii")}
    descriptor=os.open(output,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o600)
    try:
        with os.fdopen(descriptor,"wb") as handle: handle.write(canonical(anchor)); handle.flush(); os.fsync(handle.fileno())
    except Exception:
        try: output.unlink()
        except OSError: pass
        raise
    public=key.public_key().public_bytes(serialization.Encoding.Raw,serialization.PublicFormat.Raw)
    print(canonical({"schema":"m20.custodian-anchor-signing-result.v1","anchor_sha256":sha256(output.read_bytes()),"custodian_ed25519_public_key_base64":base64.b64encode(public).decode("ascii"),"custodian_ed25519_public_key_sha256":sha256(public)}).decode("utf-8"))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
