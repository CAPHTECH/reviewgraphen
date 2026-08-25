"""Outcome-blind selector. Git is invoked read-only; outputs are benchmark-local."""
import re, subprocess, unicodedata
from pathlib import Path
from .canonical import stable_id
from .oracle import OracleError, derive

REPOS = {"reviewgraphen": Path("/home/rizumita/workspace/reviewgraphen"), "fsl": Path("/home/rizumita/github/fsl"), "casegraphen": Path("/home/rizumita/github/casegraphen")}
SYMPTOM = {"fix", "fixes", "fixed", "bugfix", "repair", "correct", "prevent", "handle"}

def git(repo, *args):
    return subprocess.run(["git", "-C", str(repo), *args], check=True, stdout=subprocess.PIPE).stdout.decode("utf-8", "strict")

def normalize_title(title):
    value = unicodedata.normalize("NFKC", title)
    value = re.sub(r"(?:[A-Za-z]:)?(?:[/\\][\w.~-]+)+|\b[0-9a-fA-F]{7,64}\b", " ", value)
    return " ".join(value.split())

def enumerate_candidates(repository_id, limit=500):
    repo = REPOS[repository_id]
    head = git(repo, "rev-parse", "HEAD").strip()
    rows = []
    for oid in git(repo, "rev-list", "--first-parent", f"--max-count={limit}", head).splitlines():
        parents = git(repo, "show", "-s", "--format=%P", oid).split()
        if len(parents) != 1: continue
        title = normalize_title(git(repo, "show", "-s", "--format=%s", oid).strip())
        token = title.split(maxsplit=1)[0].lower() if title and title.split(maxsplit=1)[0].isascii() else ""
        kind = "symptom_fix" if token in SYMPTOM else "symbol_change"
        stat = git(repo, "diff", "--numstat", "--find-renames", parents[0], oid)
        files, changed, invalid = 0, 0, False
        for line in stat.splitlines():
            add, delete, path = line.split("\t", 2)
            if path.endswith(".rs"):
                if add == "-" or delete == "-": invalid = True; continue
                files += 1; changed += int(add) + int(delete)
        eligible = 1 <= files <= 8 and 5 <= changed <= 300 and not invalid and bool(title)
        cid = stable_id("candidate", repository_id, parents[0], oid, kind)
        rows.append({"candidate_id": cid, "repository_id": repository_id, "base_oid": parents[0], "fix_oid": oid, "task_kind": kind, "normalized_title": title, "rust_files": files, "changed_lines": changed, "pre_oracle_eligible": eligible})
    return {"repository_id": repository_id, "pinned_head": head, "candidates": rows}

def select(repository_id):
    candidates=enumerate_candidates(repository_id)["candidates"]
    repo=REPOS[repository_id]; oracle_admitted=[]
    for row in candidates:
        if row["repository_id"] != repository_id or not row["pre_oracle_eligible"]: continue
        try: oracle=derive(repo,repository_id,row["base_oid"],row["fix_oid"])
        except OracleError: continue
        symbols=len(oracle["symbol_ids"]); lines=oracle["evaluation_line_count"]
        if not (2 <= symbols <= 40 and 20 <= lines <= 2000): continue
        oracle_admitted.append({**row,"task_id":stable_id("task",row["candidate_id"],oracle["oracle_id"]),"oracle":oracle})
    selected = []
    for kind in ("symbol_change", "symptom_fix"):
        pool = [x for x in oracle_admitted if x["task_kind"] == kind]
        pool.sort(key=lambda x: (stable_id("sample-order", repository_id, x["base_oid"], x["fix_oid"], kind), x["task_id"]))
        if len(pool) < 10: raise ValueError(f"corpus_infeasible:{repository_id}:{kind}:{len(pool)}")
        selected.extend(pool[:10])
    return selected
