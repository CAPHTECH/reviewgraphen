# Execution-plan mapping proposal

- `case_space_id`: `case-space:m17-casegraphen-controlled-review-v1`
- `observed_revision_id`: none; the disposable experiment case space has not been created.
- Source topology: `topology:m17-casegraphen-controlled-review-v1`.
- Each topology node maps to one proposed execution-plan step with the same suffix and a content-bound input artifact.
- Data edges map to artifact dependencies. The cross-arm control edge preserves the preregistered order and prevents backend overlap. The review/authority edges mark blind Codex judgment as distinct from Qwen production.
- Information not representable without loss: topology resource claims, deployment budgets, runtime model declarations, ReviewGraphen source trace, and projection loss remain separately bound artifacts.
- Review still required: exact stable-plan schema mapping, worker binding, command allow-list, actor/capability scopes, retry policy, and base revision.
- No mutation, topology acceptance, plan proposal, plan acceptance, worker registration, or execution was performed by this proposal.
