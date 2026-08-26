"""Machine-readable transcription of EVALUATOR_SPEC numeric, enum, and conjunction contracts."""

SPEC_CONTRACTS = {
    "schema": "m20.spec-contract-matrix.v1",
    "numeric": (
        {"id":"model.raw_bytes","surface":"model_bytes","maximum":1048576},
        {"id":"model.nesting_depth","surface":"model_depth","maximum":32},
        {"id":"reviewer.claims","surface":"claims","minimum":1,"maximum":3},
        {"id":"reviewer.observations","surface":"observations","minimum":1,"maximum":8},
        {"id":"reviewer.basis_loss_ids","surface":"basis_losses","minimum":1,"maximum":3},
        {"id":"reviewer.summary_scalars","surface":"summary","minimum":1,"maximum":1024},
        {"id":"reviewer.prose_scalars","surface":"mechanism","minimum":1,"maximum":512},
        {"id":"reviewer.observation_line","surface":"line","minimum":1},
        {"id":"judge.score_records","surface":"score_records","exact":2},
        {"id":"judge.dimension","surface":"dimension","minimum":0,"maximum":2},
        {"id":"judge.total","surface":"judge_total","minimum":0,"maximum":8},
        {"id":"packet.admitted_source_bytes","surface":"source_bytes","maximum":65536},
        {"id":"baseline.diff_context_lines","surface":"diff_context","exact":3},
        {"id":"text.claim_utf8_bytes","surface":"text_claim","minimum":24,"maximum":1024},
        {"id":"text.prose_utf8_bytes","surface":"text_prose","minimum":24,"maximum":512},
        {"id":"text.distinct_ascii_tokens","surface":"distinct_tokens","minimum":3},
    ),
    "enums": (
        {"id":"git.tree_mode","surface":"tree_mode","allowed":("40000","100644","100755","120000"),"invalid":"160000"},
        {"id":"source.role","surface":"source_role","allowed":("changed","context","support"),"invalid":"caller"},
        {"id":"source.snapshot_side","surface":"snapshot_side","allowed":("base","head"),"invalid":"working"},
        {"id":"disposition.kind","surface":"disposition_kind","allowed":("claim","abstention"),"invalid":"unknown"},
        {"id":"claim.conclusion","surface":"conclusion","allowed":("issue_present","issue_absent","inconclusive"),"invalid":"unknown"},
        {"id":"abstention.reason","surface":"abstention_reason","allowed":("task_blocking_source_unavailable","task_blocking_reference_unresolved","task_blocking_projection_integrity"),"invalid":"routine_scope_omission"},
        {"id":"judge.verdict","surface":"verdict","allowed":("usable","not_usable"),"invalid":"unknown"},
        {"id":"judge.score_source","surface":"score_source","allowed":("judge","mechanical_forced_zero"),"invalid":"caller"},
    ),
    "conjunctions": (
        {"id":"repository.first_parent","surface":"first_parent","terms":("has_parent","first_parent_is_base")},
        {"id":"utility.pass","surface":"utility","terms":("judgeable","source_specificity","hidden_task_relevance","mechanism_or_blocker_specificity","audit_actionability","total_at_least_6","verdict_usable")},
        {"id":"observation.changed_support","surface":"changed_observation","terms":("known_source","range_admitted","role_changed")},
        {"id":"primary.completed","surface":"primary","terms":("process_and_schema_valid","closure_valid","mechanical_usefulness_valid","policy_valid","hashes_retained","utility_judge_valid","failure_codes_empty")},
        {"id":"packet.source_ceiling","surface":"source_ceiling","terms":("sum_every_admitted_source","at_most_65536")},
    ),
}


def derived_cases():
    cases=[]
    for row in SPEC_CONTRACTS["numeric"]:
        if "deferred" in row: continue
        values=[]
        if "exact" in row: values=((row["exact"]-1,False),(row["exact"],True),(row["exact"]+1,False))
        else:
            if "minimum" in row: values.extend(((row["minimum"]-1,False),(row["minimum"],True)))
            if "maximum" in row: values.extend(((row["maximum"],True),(row["maximum"]+1,False)))
        cases.extend(("numeric",row,value,expected) for value,expected in values)
    for row in SPEC_CONTRACTS["enums"]: cases.extend(("enum",row,value,True) for value in row["allowed"]); cases.append(("enum",row,row["invalid"],False))
    for row in SPEC_CONTRACTS["conjunctions"]: cases.append(("conjunction",row,None,True)); cases.extend(("conjunction",row,term,False) for term in row["terms"])
    return cases


def matrix_counts():
    return {"items":sum(len(SPEC_CONTRACTS[key]) for key in ("numeric","enums","conjunctions")),"numeric":len(SPEC_CONTRACTS["numeric"]),"enums":len(SPEC_CONTRACTS["enums"]),"conjunctions":len(SPEC_CONTRACTS["conjunctions"]),"derived_cases":len(derived_cases())}
