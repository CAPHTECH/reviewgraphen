# Revised user decisions after A2

Six decisions materially change scope, architecture, or evidentiary claims.
Recommendations below supersede the corresponding recommendations in A1/06;
the A1 file remains historical and unchanged.

## Q1. Which semantic wedge should be implemented first?

**Recommended answer: Candidate D.** Use accepted syntactic-unique local calls
whose exact-public callee changed. The local development probe found no unsafe
candidate even under a permissive upper bound (0/143 production-Rust commits),
while D prerequisite proxies appeared in 68.5–95.8%. D also removes the new
unsafe-ingest unit and fits a feasible paired evaluation.

If A is selected, first fund a separate prevalence census; do not reuse A1's
83-unit plan. If C is selected, accept an 11–14-unit rustdoc/toolchain/cfg
slice. C is the recommended fallback if D's exact Stage 0 prevalence fails.

**Needed before the ADR.**

## Q2. How narrow is Candidate D v1?

**Recommended answer: changed exact-public callee only.** Require an accepted
`calls` edge with `resolution=syntactic_unique`, a changed callee function, and
callee `public=true`. Defer “changed caller calls callee,” method/trait/cross-
crate calls, callsite-changed, signature-changed, and error-type-changed
variants. Report one obligation per accepted edge and preserve every overflow
candidate as deferred/excluded with weight.

Choosing both changed endpoints increases fan-out and weakens the contract-
change rationale. Choosing signature/error changes adds a new extractor and
base/head API-diff contract, increasing cost and versioning risk.

**Needed in the ADR rule definition.**

## Q3. May applicability be separated from enumeration completeness?

**Recommended answer: yes, through an explicit versioned contract split.** A
resolved relation can be applicable when its own `ast`, containment, change
mapping, endpoints, and accepted resolution are valid, while
`direct_calls=partial` remains an enumeration limitation and creates a
rule-level gap. Reports must distinguish “coverage of resolved D targets” from
“unknown total caller space.”

If no split is authorized, every D target remains `unknown` under current code;
D then fails the practical applicability gate and C should be selected. Merely
dropping `direct_calls` from requirements or declaring it complete is not an
option: it launders an incomplete denominator.

**Needed before public type/schema design.**

## Q4. What evaluation budget is authorized?

**Recommended answer: staged 24 cumulative model-hours.** First require the
300-commit model-free Stage 0. If it passes, run the 10-pair/6-hour Stage 1; only
if its frozen primary boundary passes, extend to 40 total pairs within 24
hours. The primary remains grounded-disposition completion; judge-positive
yield and clean-control false positives are secondary/non-authority.

Choosing 6 hours permits only a very-large-effect pilot (`b≥8,c≤1`) and no
stable precision/recall claim. Choosing 72 hours permits 120 pairs and a
moderate completion effect, but still does not manufacture a gold defect
universe. Choosing no model budget limits the claim to deterministic targeting,
denominator visibility, and context reduction.

**Needed before model procurement, not before core implementation.**

## Q5. Who owns the holdout and evaluator?

**Recommended answer: a separate human/evaluator with an actual access
boundary.** Resolve repository names/ranges only after implementation artifacts
are frozen; keep them on another machine or Unix identity whose ACL denies the
implementation account. The evaluator—not the implementation agent—authors
enumeration, arm packets, leakage scans, controls, and root mappings, commits
manifest hashes before execution, and reveals after results are sealed.

If this separation cannot be supplied, label the study an open development
evaluation. A hidden filename or instruction to the same account is not blind;
the consequence is loss of holdout/generalization claims, not cancellation of
the deterministic tests.

**Needed before Stage 0 corpus resolution.**

## Q6. Is the first release still non-authority?

**Recommended answer: yes.** D obligations, model claims/abstentions, verifier
observations, judge dispositions, and reports remain proposed/unreviewed with
`trusted_pass=false`. Neither exact call resolution, schema validity, test
success, judge approval, nor evaluation success becomes human acceptance.

If the same release must record human acceptance, scope expands into Store
admission, current-snapshot freshness, conflict/recovery, explicit actor
authority, and likely gluing/gate work. That requires a separate authority ADR
and should not be hidden inside the D slice.

**Needed before the ADR authority ceiling.**

## Non-decisions / fixed refusals

- Do not fetch external repositories for this audit or claim Crossbeam/Bytes
  prevalence; it is unmeasured.
- Do not call unresolved method/cross-crate calls individual covered D targets.
- Do not cap fan-out by silently deleting obligations from the denominator.
- Do not change the frozen primary metric after seeing arm results.
- Do not count claims or multiple obligations from one commit as independent
  samples.
- Do not let model prose, confidence, completion, or judge approval promote
  facts or claims to accepted/verified/human-accepted state.
