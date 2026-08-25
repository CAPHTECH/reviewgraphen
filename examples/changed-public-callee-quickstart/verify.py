#!/usr/bin/env python3
"""Read-only verifier for the checked-in provider-free quickstart."""

import argparse
import hashlib
import json
import os
import stat
import re
import sys


def fail(message):
    print(message, file=sys.stderr)
    raise SystemExit(1)


def regular_file(path):
    try:
        metadata = os.lstat(path)
    except OSError:
        fail(f"missing: {path}")
    if not stat.S_ISREG(metadata.st_mode):
        fail(f"not a regular file: {path}")


def canonical_json(path):
    regular_file(path)
    with open(path, "rb") as handle:
        content = handle.read()
    try:
        value = json.loads(content)
    except (UnicodeDecodeError, json.JSONDecodeError):
        fail(f"invalid JSON: {path}")
    encoded = json.dumps(
        value, ensure_ascii=False, separators=(",", ":"), sort_keys=True
    ).encode("utf-8")
    if content != encoded:
        fail(f"non-canonical JSON: {path}")
    return value


def sha256(content):
    return "sha256:" + hashlib.sha256(content).hexdigest()


def file_bytes(path):
    regular_file(path)
    with open(path, "rb") as handle:
        return handle.read()


def expected_directories(paths):
    directories = {""}
    for path in paths:
        parent = os.path.dirname(path)
        while parent:
            directories.add(parent)
            parent = os.path.dirname(parent)
    return directories


def observed_paths(root):
    observed = set()
    for current, directories, files in os.walk(root, followlinks=False):
        relative_current = os.path.relpath(current, root)
        relative_current = "" if relative_current == "." else relative_current
        for directory in directories:
            path = os.path.join(current, directory)
            metadata = os.lstat(path)
            if stat.S_ISLNK(metadata.st_mode) or not stat.S_ISDIR(metadata.st_mode):
                fail(f"non-directory artifact entry: {path}")
            observed.add(os.path.join(relative_current, directory))
        for filename in files:
            path = os.path.join(current, filename)
            metadata = os.lstat(path)
            if stat.S_ISLNK(metadata.st_mode) or not stat.S_ISREG(metadata.st_mode):
                fail(f"non-regular artifact entry: {path}")
            observed.add(os.path.join(relative_current, filename))
    return observed


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--expected", required=True)
    parser.add_argument("--output-root", required=True)
    arguments = parser.parse_args()

    expected = canonical_json(arguments.expected)
    required = {
        "schema",
        "request_sha256",
        "first_exit_code",
        "second_exit_code",
        "artifacts",
    }
    if set(expected) != required or expected["schema"] != "reviewgraphen.quickstart_expected_hashes.v1":
        fail("invalid expected hash contract")
    if expected["first_exit_code"] != 0 or expected["second_exit_code"] != 20:
        fail("invalid expected exit codes")
    artifacts = expected["artifacts"]
    if not isinstance(artifacts, dict) or list(artifacts) != sorted(artifacts):
        fail("artifact map is not sorted")
    hash_pattern = re.compile(r"sha256:[0-9a-f]{64}\Z")
    if not isinstance(expected["request_sha256"], str) or not hash_pattern.fullmatch(
        expected["request_sha256"]
    ):
        fail("invalid request hash")

    request_path = os.path.join(os.path.dirname(arguments.expected), "request.v3.json")
    if sha256(file_bytes(request_path)) != expected["request_sha256"]:
        fail("request hash mismatch")

    root_metadata = os.lstat(arguments.output_root)
    if stat.S_ISLNK(root_metadata.st_mode) or not stat.S_ISDIR(root_metadata.st_mode):
        fail("output root is not a directory")
    expected_paths = set(artifacts)
    expected_entries = expected_paths | expected_directories(expected_paths)
    if observed_paths(arguments.output_root) != expected_entries - {""}:
        fail("artifact path set mismatch")

    for relative_path, entry in artifacts.items():
        if set(entry) != {"byte_length", "sha256"}:
            fail(f"invalid expected artifact entry: {relative_path}")
        if (
            not isinstance(entry["byte_length"], int)
            or isinstance(entry["byte_length"], bool)
            or entry["byte_length"] < 0
            or not isinstance(entry["sha256"], str)
            or not hash_pattern.fullmatch(entry["sha256"])
        ):
            fail(f"invalid expected artifact value: {relative_path}")
        if os.path.isabs(relative_path) or ".." in relative_path.split("/"):
            fail(f"unsafe expected artifact path: {relative_path}")
        path = os.path.join(arguments.output_root, relative_path)
        content = file_bytes(path)
        if len(content) != entry["byte_length"] or sha256(content) != entry["sha256"]:
            fail(f"artifact hash mismatch: {relative_path}")
        if relative_path.endswith(".json"):
            canonical_json(path)


if __name__ == "__main__":
    main()
