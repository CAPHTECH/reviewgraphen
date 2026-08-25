"""Realized-fix oracle derived from pinned product accepted-fact projections."""

from __future__ import annotations

import re
from dataclasses import dataclass
from pathlib import Path

from .canonical import hash_json, stable_id
from .product import PRODUCT_CLI_SHA256, ProductError, commit_parent, resolve_commit, run_product_review, _git


class OracleError(ValueError):
    def __init__(self, code: str, detail: str | None = None):
        self.record = {
            "schema": "m21.typed_failure.v1",
            "code": code,
            "detail": detail or code,
        }
        super().__init__(code)


@dataclass(frozen=True)
class Hunk:
    old_path: str
    new_path: str
    old_start: int
    old_count: int
    new_start: int
    new_count: int
    added_lines: tuple[tuple[int, str], ...]
    deleted_lines: tuple[tuple[int, str], ...]

    @property
    def substantive(self) -> bool:
        rows = [text for _, text in (*self.added_lines, *self.deleted_lines)]
        return any(_line_has_code(text) for text in rows)


def _line_has_code(text: str) -> bool:
    stripped = text.strip()
    return bool(stripped and not stripped.startswith(("//", "/*", "*", "*/")))


def _changed_hunks(repo: Path, base: str, target: str) -> list[Hunk]:
    text = _git(
        repo,
        "diff",
        "--unified=0",
        "--no-ext-diff",
        "--no-color",
        "--find-renames",
        "--diff-algorithm=myers",
        base,
        target,
        "--",
        "*.rs",
    )
    old_path = new_path = None
    current = None
    rows: list[Hunk] = []
    old_line = new_line = 0
    for line in text.splitlines():
        if line.startswith("--- a/"):
            old_path = line[6:]
        elif line.startswith("+++ b/"):
            new_path = line[6:]
        elif line.startswith("@@"):
            match = re.match(r"@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@", line)
            if not match or old_path is None or new_path is None:
                raise OracleError("diff_hunk_invalid")
            old_start, old_count = int(match[1]), int(match[2] or 1)
            new_start, new_count = int(match[3]), int(match[4] or 1)
            current = {
                "old_path": old_path,
                "new_path": new_path,
                "old_start": old_start,
                "old_count": old_count,
                "new_start": new_start,
                "new_count": new_count,
                "added_lines": [],
                "deleted_lines": [],
            }
            rows.append(current)
            old_line, new_line = old_start, new_start
        elif current is not None and line.startswith("+"):
            current["added_lines"].append((new_line, line[1:]))
            new_line += 1
        elif current is not None and line.startswith("-"):
            current["deleted_lines"].append((old_line, line[1:]))
            old_line += 1
        elif current is not None and line.startswith(" "):
            old_line += 1
            new_line += 1
    hunks = [Hunk(**{**row, "added_lines": tuple(row["added_lines"]), "deleted_lines": tuple(row["deleted_lines"])}) for row in rows]
    hunks = [hunk for hunk in hunks if hunk.substantive]
    if not hunks:
        raise OracleError("no_substantive_rust_hunks")
    return hunks


def _span_intersects(start: int, end: int, hunk_start: int, hunk_count: int) -> bool:
    if hunk_count == 0:
        return start <= hunk_start <= end + 1
    return start <= hunk_start + hunk_count - 1 and hunk_start <= end


def _accepted_facts(audit: dict) -> tuple[dict[str, dict], list[dict]]:
    """Project accepted definitions/containment/calls retained by product v3."""
    definitions: dict[str, dict] = {}
    relations: dict[str, dict] = {}
    for envelope in audit.get("contexts", []):
        context = envelope.get("context", {})
        paths = {
            row.get("artifact_id"): row.get("path")
            for row in context.get("materialized_sources", [])
            if isinstance(row, dict)
        }
        endpoints = {}
        for outcome in context.get("subject_outcomes", []):
            if not isinstance(outcome, dict) or outcome.get("state") != "admitted":
                continue
            symbol_id = outcome.get("endpoint_id")
            source_id = outcome.get("source_artifact_id")
            span = outcome.get("requested_range", {})
            row = {
                "symbol_id": symbol_id,
                "source_id": source_id,
                "path": paths.get(source_id),
                "start_line": span.get("start_line"),
                "end_line": span.get("end_line"),
                "fact_ids": sorted(filter(None, [symbol_id, source_id, outcome.get("window_id")])),
            }
            if not isinstance(symbol_id, str) or not symbol_id.startswith(("function:", "type:")):
                continue
            if not isinstance(row["path"], str) or not all(isinstance(row[key], int) for key in ("start_line", "end_line")):
                raise OracleError("accepted_definition_invalid")
            prior = definitions.get(symbol_id)
            if prior is not None:
                if any(prior[key] != row[key] for key in ("symbol_id", "source_id", "path", "start_line", "end_line")):
                    raise OracleError("accepted_definition_conflict")
                row["fact_ids"] = sorted(set(prior["fact_ids"]) | set(row["fact_ids"]))
            definitions[symbol_id] = row
            endpoints[outcome.get("role")] = symbol_id
        for window in context.get("windows", []):
            if not isinstance(window, dict):
                continue
            source_id = window.get("source_artifact_id")
            span = window.get("range", {})
            for symbol_id in window.get("owner_ids", []):
                if not isinstance(symbol_id, str) or not symbol_id.startswith(("function:", "type:")):
                    continue
                row = {
                    "symbol_id":symbol_id,
                    "source_id":source_id,
                    "path":paths.get(source_id),
                    "start_line":span.get("start_line"),
                    "end_line":span.get("end_line"),
                    "fact_ids":sorted(filter(None, [window.get("id"), source_id, *window.get("support_anchor_ids", [])])),
                }
                if not isinstance(row["path"], str) or not all(isinstance(row[key], int) for key in ("start_line", "end_line")):
                    raise OracleError("accepted_definition_invalid")
                prior = definitions.get(symbol_id)
                if prior is not None:
                    if any(prior[key] != row[key] for key in ("symbol_id", "source_id", "path", "start_line", "end_line")):
                        raise OracleError("accepted_definition_conflict")
                    row["fact_ids"] = sorted(set(prior["fact_ids"]) | set(row["fact_ids"]))
                definitions[symbol_id] = row
        relation_ids = context.get("target_refs", [])
        if len(relation_ids) == 1 and set(endpoints) == {"caller", "callee"}:
            relation_id = relation_ids[0]
            row = {
                "relation_id": relation_id,
                "caller_id": endpoints["caller"],
                "callee_id": endpoints["callee"],
            }
            prior = relations.get(relation_id)
            if prior is not None and prior != row:
                raise OracleError("accepted_relation_conflict")
            relations[relation_id] = row
    if not definitions:
        raise OracleError("accepted_fact_closure_empty")
    return definitions, sorted(relations.values(), key=lambda row: row["relation_id"])


def _symbol_names(repo: Path, target: str, definitions: dict[str, dict]) -> dict[str, str]:
    by_path: dict[str, list[str]] = {}
    for row in definitions.values():
        by_path.setdefault(row["path"], []).append(row["symbol_id"])
    names = {}
    pattern = re.compile(r"\b(?:fn|struct|enum|trait|type)\s+([A-Za-z_][A-Za-z0-9_]*)")
    for path, symbol_ids in by_path.items():
        source = _git(repo, "show", f"{target}:{path}").splitlines()
        for symbol_id in symbol_ids:
            row = definitions[symbol_id]
            excerpt = "\n".join(source[row["start_line"] - 1:row["end_line"]])
            match = pattern.search(excerpt)
            if not match:
                raise OracleError("accepted_definition_name_unavailable", symbol_id)
            names[symbol_id] = match.group(1)
    return names


def _added_identifiers(hunks: list[Hunk]) -> dict[str, dict[int, set[str]]]:
    result: dict[str, dict[int, set[str]]] = {}
    token = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")
    for hunk in hunks:
        block_depth = 0
        for line_number, text in hunk.added_lines:
            code = []
            index = 0
            quote = None
            while index < len(text):
                pair = text[index:index + 2]
                if block_depth:
                    if pair == "/*": block_depth += 1; index += 2; continue
                    if pair == "*/": block_depth -= 1; index += 2; continue
                    index += 1; continue
                if quote:
                    if text[index] == "\\": index += 2; continue
                    if text[index] == quote: quote = None
                    index += 1; continue
                if pair == "//": break
                if pair == "/*": block_depth = 1; index += 2; continue
                if text[index] in {'"', "'"}: quote = text[index]; index += 1; continue
                code.append(text[index]); index += 1
            result.setdefault(hunk.new_path, {})[line_number] = set(token.findall("".join(code)))
        if block_depth:
            raise OracleError("oracle_lexical_boundary_unknown")
    return result


def _rename_paths(hunks: list[Hunk]) -> dict[str, str]:
    """Return the accepted old-to-new path normalization used by stable keys."""
    old_to_new: dict[str, str] = {}
    new_to_old: dict[str, str] = {}
    for hunk in hunks:
        prior_new = old_to_new.setdefault(hunk.old_path, hunk.new_path)
        prior_old = new_to_old.setdefault(hunk.new_path, hunk.old_path)
        if prior_new != hunk.new_path or prior_old != hunk.old_path:
            raise OracleError("rename_mapping_ambiguous")
    return old_to_new


def _stable_definition_index(
    definitions: dict[str, dict],
    names: dict[str, str],
    *,
    old_to_new: dict[str, str] | None = None,
) -> dict[tuple[str, str, str], str]:
    """Index accepted definitions by the frozen cross-tree stable key."""
    index: dict[tuple[str, str, str], str] = {}
    for symbol_id, row in definitions.items():
        path = row["path"]
        normalized_path = (old_to_new or {}).get(path, path)
        key = (symbol_id.split(":", 1)[0], normalized_path, names[symbol_id])
        if key in index and index[key] != symbol_id:
            raise OracleError("stable_definition_key_ambiguous", "::".join(key))
        index[key] = symbol_id
    return index


def _coalesce(definitions: dict[str, dict], symbol_ids: set[str]) -> list[dict]:
    rows = sorted(
        ({key: definitions[symbol_id][key] for key in ("path", "start_line", "end_line")} for symbol_id in symbol_ids),
        key=lambda row: (row["path"], row["start_line"], row["end_line"]),
    )
    merged = []
    for row in rows:
        if merged and merged[-1]["path"] == row["path"] and row["start_line"] <= merged[-1]["end_line"] + 1:
            merged[-1]["end_line"] = max(merged[-1]["end_line"], row["end_line"])
        else:
            merged.append(dict(row))
    return merged


def derive(repository, repository_id, base_oid, fix_oid):
    """Derive T_old union T_match union R; no CLI/audit/artifact injection exists."""
    repo = Path(repository)
    if repo.is_symlink() or not repo.is_dir():
        raise OracleError("repository_not_allowed")
    repo = repo.resolve(strict=True)
    try:
        base = resolve_commit(repo, base_oid)
        fix = resolve_commit(repo, fix_oid)
        parents = _git(repo, "show", "-s", "--format=%P", fix).split()
        if parents != [base]:
            raise OracleError("not_direct_single_parent")
        hunks = _changed_hunks(repo, base, fix)
        base_parent = commit_parent(repo, base)
        base_product = run_product_review(repo, repository_id, base_parent, base)
        base_rederived = run_product_review(repo, repository_id, base_parent, base)
        product = run_product_review(repo, repository_id, base, fix)
        rederived = run_product_review(repo, repository_id, base, fix)
    except ProductError as error:
        raise OracleError(error.record["code"], error.record["detail"]) from error
    if (
        base_rederived.audit_sha256 != base_product.audit_sha256
        or base_rederived.snapshot_id != base_product.snapshot_id
        or base_rederived.universe_id != base_product.universe_id
        or rederived.audit_sha256 != product.audit_sha256
        or rederived.snapshot_id != product.snapshot_id
        or rederived.universe_id != product.universe_id
    ):
        raise OracleError("product_rederivation_mismatch")

    base_definitions, base_relations = _accepted_facts(base_product.audit)
    definitions, relations = _accepted_facts(product.audit)
    base_names = _symbol_names(repo, base, base_definitions)
    names = _symbol_names(repo, fix, definitions)
    old_to_new = _rename_paths(hunks)
    base_index = _stable_definition_index(base_definitions, base_names, old_to_new=old_to_new)
    target_index = _stable_definition_index(definitions, names)
    identifiers = _added_identifiers(hunks)

    # T_old is intentionally computed only in B coordinates: accepted base
    # declaration spans against old_start/old_count.  It does not depend on a
    # target definition existing, so deletion-only symbols remain observable.
    t_old = {
        symbol_id
        for symbol_id, row in base_definitions.items()
        if any(
            hunk.old_path == row["path"]
            and hunk.old_count
            and _span_intersects(row["start_line"], row["end_line"], hunk.old_start, hunk.old_count)
            for hunk in hunks
        )
    }
    t_match = set()
    target_to_base: dict[str, str] = {}
    for key, target_symbol_id in target_index.items():
        base_symbol_id = base_index.get(key)
        if base_symbol_id is not None:
            target_to_base[target_symbol_id] = base_symbol_id
    for target_symbol_id, row in definitions.items():
        if target_symbol_id not in target_to_base:
            continue
        if any(
            hunk.new_path == row["path"]
            and hunk.new_count
            and _span_intersects(row["start_line"], row["end_line"], hunk.new_start, hunk.new_count)
            for hunk in hunks
        ):
            t_match.add(target_to_base[target_symbol_id])

    referenced = set()
    reference_fact_ids = set()
    for relation in relations:
        caller = definitions[relation["caller_id"]]
        callee_id = target_to_base.get(relation["callee_id"])
        if callee_id is None:
            continue
        callee_name = base_names[callee_id]
        lines = identifiers.get(caller["path"], {})
        if any(
            caller["start_line"] <= line_number <= caller["end_line"] and callee_name in tokens
            for line_number, tokens in lines.items()
        ):
            referenced.add(callee_id)
            reference_fact_ids.add(relation["relation_id"])
    definitions_by_name = {}
    for symbol_id, name in base_names.items():
        definitions_by_name.setdefault(name, []).append(symbol_id)
    for path, changed_lines in identifiers.items():
        for line_number, tokens in changed_lines.items():
            enclosing = [
                symbol_id
                for symbol_id, definition in definitions.items()
                if target_to_base.get(symbol_id) in t_match
                and definition["path"] == path
                and definition["start_line"] <= line_number <= definition["end_line"]
            ]
            if not enclosing:
                continue
            for name in tokens & set(definitions_by_name):
                candidates = definitions_by_name[name]
                if len(candidates) != 1:
                    raise OracleError("reference_binding_ambiguous", name)
                symbol_id = candidates[0]
                if symbol_id not in {target_to_base[item] for item in enclosing}:
                    referenced.add(symbol_id)
                    reference_fact_ids.update(base_definitions[symbol_id]["fact_ids"])

    product_symbols = t_old | t_match | referenced
    if not t_old or not t_match:
        raise OracleError("accepted_fact_closure_incomplete")
    if any(
        not any(
            (hunk.old_count and definition["path"] == hunk.old_path
             and _span_intersects(definition["start_line"], definition["end_line"], hunk.old_start, hunk.old_count))
            for definition in base_definitions.values()
        )
        and not any(
            (hunk.new_count and definition["path"] == hunk.new_path
             and _span_intersects(definition["start_line"], definition["end_line"], hunk.new_start, hunk.new_count))
            for definition in definitions.values()
        )
        for hunk in hunks
    ):
        raise OracleError("accepted_fact_closure_incomplete")

    symbols = t_old | t_match | referenced
    ranges = _coalesce(base_definitions, symbols)
    lines = sorted(
        [[row["path"], line] for row in ranges for line in range(row["start_line"], row["end_line"] + 1)],
        key=lambda row: (row[0], row[1]),
    )
    accepted_fact_projection = {
        "base_definitions": [base_definitions[symbol_id] for symbol_id in sorted(base_definitions)],
        "base_relations": base_relations,
        "fix_definitions": [definitions[symbol_id] for symbol_id in sorted(definitions)],
        "fix_relations": relations,
        "base_snapshot_id": base_product.snapshot_id,
        "product_snapshot_id": product.snapshot_id,
        "product_universe_id": product.universe_id,
    }
    identity = {
        "repository_id": repository_id,
        "base_oid": base,
        "fix_oid": fix,
        "base_snapshot_id": base_product.snapshot_id,
        "extractor": "reviewgraphen-cli:pinned-accepted-facts-v3",
        "product_cli_sha256": PRODUCT_CLI_SHA256,
        "product_snapshot_id": product.snapshot_id,
        "product_universe_id": product.universe_id,
        "accepted_fact_projection_sha256": hash_json(accepted_fact_projection),
        "t_old_symbol_ids": sorted(t_old),
        "t_match_symbol_ids": sorted(t_match),
        "reference_symbol_ids": sorted(referenced),
        "reference_fact_ids": sorted(reference_fact_ids),
        "symbol_ids": sorted(symbols),
        "product_symbol_fact_ids": sorted(product_symbols),
        "symbol_sources": [
            {
                "symbol_id": symbol_id,
                "name": base_names[symbol_id],
                "path": base_definitions[symbol_id]["path"],
                "memberships": sorted(
                    label
                    for label, members in (("T_old", t_old), ("T_match", t_match), ("R", referenced))
                    if symbol_id in members
                ),
                "accepted_fact_ids": base_definitions[symbol_id]["fact_ids"],
            }
            for symbol_id in sorted(symbols)
        ],
        "ranges": ranges,
    }
    return {
        "schema": "m21.realized_fix_oracle.v1",
        "oracle_id": stable_id("oracle", hash_json(identity)),
        **identity,
        "evaluation_lines": lines,
        "evaluation_line_count": len(lines),
        "evaluation_line_set_sha256": hash_json(lines),
        "source_base_audit_sha256": base_product.audit_sha256,
        "source_base_manifest_sha256": base_product.manifest_sha256,
        "source_audit_sha256": product.audit_sha256,
        "source_manifest_sha256": product.manifest_sha256,
    }
