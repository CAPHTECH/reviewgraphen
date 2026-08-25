"""Benchmark-local subject-first projection over accepted, source-traced facts."""
from .canonical import hash_json, stable_id

REASONS={"subject","caller","callee","reference","type","test","configuration","other"}

def build(task, facts, max_lines=4096):
    ordered=sorted(facts,key=lambda x:(x["distance"],x["path"],x["start_line"],x["end_line"],x["fact_id"]))
    subjects=[x for x in ordered if x["relation"]=="subject" and x["binding"]=="resolved"]
    bindings={x["symbol_id"] for x in subjects}
    if not subjects:
        state="ambiguous" if any(x["binding"]=="ambiguous" for x in ordered) else "unresolved"
        losses=[{"scope":"task subject","reason":"ambiguous_binding" if state=="ambiguous" else "unresolved_symbol","attempt_source_ids":sorted({x["source_id"] for x in ordered})}]
        admitted=[]
    else:
        losses=[]; admitted=[]; used=0
        for row in subjects+[x for x in ordered if x not in subjects]:
            size=row["end_line"]-row["start_line"]+1
            if used+size>max_lines:
                losses.append({"scope":f'{row["path"]}:{row["start_line"]}-{row["end_line"]}',"reason":"budget","attempt_source_ids":[row["source_id"]]}); continue
            used+=size; admitted.append({"path":row["path"],"symbol_id":row["symbol_id"],"start_line":row["start_line"],"end_line":row["end_line"],"reason":row["relation"] if row["relation"] in REASONS else "other"})
    admitted.sort(key=lambda x:(x["path"],x["start_line"],x["end_line"],x["symbol_id"]))
    body={"policy_id":"m21.task_subject_windows@1","task_id":task["task_id"],"snapshot_id":task["snapshot_id"],"subject_symbol_ids":sorted(bindings),"source_ids":sorted({x["source_id"] for x in ordered}),"context_items":admitted,"coverage_claim":"partial" if losses else "complete","declared_losses":losses,"fact_set_sha256":hash_json(sorted(facts,key=lambda x:x["fact_id"]))}
    return {"schema":"m21.task_subject_packet.v1","packet_id":stable_id("packet",hash_json(body)),**body}
