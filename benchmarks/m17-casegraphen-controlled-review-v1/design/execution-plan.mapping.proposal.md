# Execution-plan mapping proposal

- `case_space_id`: `case-space:m17-casegraphen-controlled-review-v1`
- `observed_revision_id`: none; the disposable experiment case space has not been created.
- Source topology: `topology:m17-casegraphen-controlled-review-v1`.
- Runtime topology micro-nodes do not map one-for-one to CaseGraphen plan steps. The external runtime executes them and emits one content-addressed report per arm.
- The proposed CaseGraphen plan, if used for deterministic reconciliation only, has at most three governed steps corresponding to the two arms and aggregate. No LLM or agent CLI is registered as a CaseGraphen shell worker.
- Data edges remain external-runtime artifact dependencies. The cross-arm control edge preserves the preregistered order and prevents backend overlap. The review/authority edges mark blind Codex judgment as distinct from Qwen production.
- Information not representable without loss: topology resource claims, deployment budgets, runtime model declarations, ReviewGraphen source trace, and projection loss remain separately bound artifacts.
- Review still required: exact stable-plan schema mapping if deterministic reconciliation workers are used, runtime adapter boundary, actor/capability scopes, evidence promotion policy, retry policy, and base revision.
- No mutation, topology acceptance, plan proposal, plan acceptance, worker registration, or execution was performed by this proposal.
