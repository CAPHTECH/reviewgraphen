"""Hash-verifying, fixed-command Git object reader."""; import difflib; import hashlib; import os; import subprocess; from pathlib import Path; from .canonical import stable_id
class PreflightError(ValueError):
    def __init__(self, code: str, detail: str = "repository"): self.code, self.detail = code, detail; super().__init__(code)
def valid_path(path: str) -> bool: return isinstance(path, str) and bool(path) and path.isascii() is not None and "\x00" not in path and "\\" not in path and not path.startswith("/") and all(part not in {"", ".", ".."} for part in path.split("/"))
def production_rust(path: str) -> bool:
    if not valid_path(path) or not path.split("/")[-1].endswith(".rs"): return False
    parts, base = path.split("/"), path.split("/")[-1]
    if any(part in {"third_party", "vendor", "vendored", "generated", "target", "benches", "tests", "example", "examples", "doc", "docs"} for part in parts): return False
    return not base.endswith((".generated.rs", "_test.rs", "_tests.rs")) and base not in {"test.rs", "tests.rs"}
class TreeSnapshot(dict[str, tuple[str, str]]):
    def __init__(self, entries: dict[str, tuple[str, str]], ignored_symlink_count: int):
        super().__init__(entries); self.ignored_symlink_count = ignored_symlink_count
class GitRepository:
    GIT = Path("/usr/bin/git")
    def __init__(self, root: str, allow_list: list[str]):
        if not isinstance(root, str) or not isinstance(allow_list, list) or not allow_list: raise PreflightError("repository_not_allowed")
        candidate = Path(root)
        if candidate.is_symlink() or not candidate.is_absolute() or not candidate.is_dir(): raise PreflightError("repository_not_allowed")
        self.root = candidate.resolve(strict=True); allowed = []
        for item in allow_list:
            path = Path(item)
            if path.is_symlink() or not path.is_absolute() or not path.is_dir(): raise PreflightError("repository_not_allowed")
            allowed.append(path.resolve(strict=True))
        if self.root not in allowed or not self.GIT.is_file() or self.GIT.is_symlink(): raise PreflightError("repository_not_allowed")
        self.env = {"GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_CONFIG_COUNT": "0", "LC_ALL": "C", "PATH": ""}; result = subprocess.run([str(self.GIT), "rev-parse", "--show-object-format"], cwd=self.root, env=self.env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE, shell=False, timeout=10, check=False)
        if result.returncode or result.stderr or result.stdout not in {b"sha1\n", b"sha256\n"}: raise PreflightError("object_format_invalid")
        self.object_format = result.stdout[:-1].decode("ascii"); self.oid_bytes = 20 if self.object_format == "sha1" else 32; self._cache: dict[str, tuple[str, bytes]] = {}
    def object(self, oid: str, expected_type: str | None = None) -> tuple[str, bytes]:
        if not isinstance(oid, str) or len(oid) != self.oid_bytes * 2 or any(c not in "0123456789abcdef" for c in oid): raise PreflightError("object_oid_invalid")
        if oid not in self._cache:
            result = subprocess.run([str(self.GIT), "cat-file", "--batch"], input=(oid + "\n").encode(), cwd=self.root, env=self.env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, shell=False, timeout=10, check=False)
            if result.returncode or result.stderr: raise PreflightError("object_read_failed")
            first = result.stdout.find(b"\n")
            if first < 0: raise PreflightError("object_framing_invalid")
            header, framed = result.stdout[:first].split(b" "), result.stdout[first + 1:]
            if len(header) != 3 or header[0] != oid.encode("ascii") or header[1] not in {b"commit", b"tree", b"blob"}: raise PreflightError("object_framing_invalid")
            size_raw = header[2]
            if not size_raw or any(byte < 0x30 or byte > 0x39 for byte in size_raw) or len(size_raw) > 1 and size_raw.startswith(b"0"): raise PreflightError("object_framing_invalid")
            size = int(size_raw)
            if size < 0 or len(framed) != size + 1 or framed[-1:] != b"\n": raise PreflightError("object_framing_invalid")
            content, kind = framed[:-1], header[1].decode("ascii"); digest = hashlib.new(self.object_format, header[1] + b" " + str(size).encode() + b"\0" + content).hexdigest()
            if digest != oid: raise PreflightError("object_hash_mismatch")
            self._cache[oid] = (kind, content)
        kind, content = self._cache[oid]
        if expected_type is not None and kind != expected_type: raise PreflightError("object_type_invalid")
        return kind, content
    def commit(self, oid: str) -> tuple[str, list[str]]:
        content = self.object(oid, "commit")[1]; header = content.split(b"\n\n", 1)[0].splitlines()
        trees = [line for line in header if line.startswith(b"tree ")]
        if len(trees) != 1 or not header or header[0] != trees[0]: raise PreflightError("commit_invalid")
        try: tree = header[0][5:].decode("ascii"); parents = [line[7:].decode("ascii") for line in header if line.startswith(b"parent ")]
        except UnicodeError as error: raise PreflightError("commit_invalid") from error
        for value in [tree, *parents]:
            if len(value) != self.oid_bytes * 2 or any(c not in "0123456789abcdef" for c in value): raise PreflightError("commit_invalid")
        return tree, parents
    def tree(self, oid: str) -> TreeSnapshot:
        output: dict[str, tuple[str, str]] = {}; ignored_symlink_count = 0
        def visit(tree_oid: str, prefix: str) -> None:
            nonlocal ignored_symlink_count
            content, index = self.object(tree_oid, "tree")[1], 0; local = set()
            while index < len(content):
                space, nul = content.find(b" ", index), content.find(b"\0", index)
                if space <= index or nul <= space or nul + 1 + self.oid_bytes > len(content): raise PreflightError("tree_framing_invalid")
                mode_raw, name_raw = content[index:space], content[space + 1:nul]
                try: mode, name = mode_raw.decode("ascii"), name_raw.decode("utf-8", "strict")
                except UnicodeError as error: raise PreflightError("tree_edge_invalid") from error
                if mode not in {"40000", "100644", "100755", "120000"} or not name or "/" in name or name in {".", ".."} or name in local: raise PreflightError("tree_edge_invalid")
                local.add(name); child = content[nul + 1:nul + 1 + self.oid_bytes].hex(); path = prefix + name
                if not valid_path(path): raise PreflightError("tree_path_invalid")
                if mode == "40000": visit(child, path + "/")
                else:
                    if path in output: raise PreflightError("tree_duplicate_path")
                    self.object(child, "blob")
                    if mode == "120000": ignored_symlink_count += 1
                    else: output[path] = (mode, child)
                index = nul + 1 + self.oid_bytes
        visit(oid, ""); return TreeSnapshot(output, ignored_symlink_count)
    def snapshots(self, base_oid: str, head_oid: str) -> tuple[dict, dict, str, str]:
        base_tree_oid, _ = self.commit(base_oid); head_tree_oid, parents = self.commit(head_oid)
        if not parents or parents[0] != base_oid: raise PreflightError("first_parent_mismatch")
        return self.tree(base_tree_oid), self.tree(head_tree_oid), base_tree_oid, head_tree_oid
    def blob_for(self, trees: tuple[dict, dict], side: str, path: str, asserted_oid: str | None = None) -> bytes:
        if side not in {"base", "head"} or not valid_path(path): raise PreflightError("source_path_invalid")
        entry = trees[0 if side == "base" else 1].get(path)
        if entry is None: raise KeyError(path)
        if asserted_oid is not None and asserted_oid != entry[1]: raise KeyError(path)
        return self.object(entry[1], "blob")[1]
    def baseline_specs(self, trees: tuple[dict, dict]) -> list[dict]:
        specs = []; paths = sorted({path for tree in trees for path in tree if production_rust(path)}, key=lambda x: x.encode())
        for path in paths:
            before, after = trees[0].get(path), trees[1].get(path)
            if before == after: continue
            before_bytes = self.object(before[1], "blob")[1] if before else b""; after_bytes = self.object(after[1], "blob")[1] if after else b""
            try: before_lines, after_lines = before_bytes.decode("utf-8").splitlines(True), after_bytes.decode("utf-8").splitlines(True)
            except UnicodeDecodeError:
                sides = [("base", before, before_bytes), ("head", after, after_bytes)]
                for side, entry, raw in sides:
                    if entry is not None: body = {"role": "changed", "snapshot_side": side, "path": path, "start_line": 1, "end_line": max(1, raw.count(b"\n") + bool(raw and not raw.endswith(b"\n"))), "blob_oid": entry[1]}; specs.append({"required_id": stable_id("source-request", body), **body})
                continue
            matcher = difflib.SequenceMatcher(None, before_lines, after_lines, autojunk=False)
            for tag, i1, i2, j1, j2 in matcher.get_opcodes():
                if tag == "equal": continue
                for side, lo, hi, lines, entry in (("base", i1, i2, before_lines, before), ("head", j1, j2, after_lines, after)):
                    if lo == hi or entry is None: continue
                    start, end = max(0, lo - 3) + 1, min(len(lines), hi + 3); body = {"role": "changed", "snapshot_side": side, "path": path, "start_line": start, "end_line": end, "blob_oid": entry[1]}; specs.append({"required_id": stable_id("source-request", body), **body})
        merged = []
        for spec in sorted(specs, key=lambda x: (x["path"].encode(), x["snapshot_side"] != "base", x["start_line"], x["end_line"])):
            prior = merged[-1] if merged and all(merged[-1][key] == spec[key] for key in ("snapshot_side", "path", "blob_oid")) else None
            if prior is not None and spec["start_line"] <= prior["end_line"] + 1: prior["end_line"] = max(prior["end_line"], spec["end_line"]); body = {k: prior[k] for k in ("role", "snapshot_side", "path", "start_line", "end_line", "blob_oid")}; prior["required_id"] = stable_id("source-request", body)
            else: merged.append(dict(spec))
        return merged
    def provenance(self) -> dict: return {"schema": "m20.repository-provenance.v1", "repository_root": str(self.root), "object_format": self.object_format, "git_executable": str(self.GIT), "git_executable_sha256": "sha256:" + hashlib.sha256(self.GIT.read_bytes()).hexdigest()}
