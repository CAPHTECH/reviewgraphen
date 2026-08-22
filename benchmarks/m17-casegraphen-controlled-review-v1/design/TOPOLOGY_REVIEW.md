# Independent topology review notes

Review the exact pair:

- `execution.topology.json`
- `deployment-policy-manifest.json`

The manifest binds the canonical topology hash emitted by CaseGraphen lint and the byte hashes of all proposed verification and budget policies.

## Intended boundary

CaseGraphen owns dependency ordering, attempt and wall-clock budgets, barriers, completeness, and typed artifact handoffs. ReviewGraphen owns all review semantics: the frozen Projection cards, included source IDs, information-loss declarations, consumed-card ledger, finding schema, grounding validation, and claim/evidence meaning. A CaseGraphen node report or completed cell must not be treated as an accepted, evidence-supported, verified, or human-accepted ReviewGraphen claim.

## Lint findings requiring judgment

- Eight deterministic `redundant_reachability` information findings arise because each expansion consumes both the prior context-state artifact and a selection artifact, and final validation consumes both the context trace and the review. Alternate graph reachability does not transport those separately typed inputs. Removing these data edges would remove a required input binding.
- `barrier_on_pipeline_path` warnings are expected: each selector must observe one complete Projection state, and each projector must validate one complete selector result before the next selector starts. Streaming partial model output has no defined semantics here.
- `authority_concentration_candidate` is unresolved at design time. The topology and policies share a proposal author; runtime production and judgment are intended to use distinct actors, capabilities, and sessions. Those runtime declarations are not accepted facts.
- `side_effect_without_verification_policy` applies to Qwen selector calls. Their requests affect which context is projected, but ReviewGraphen validates uniqueness, membership, and round budget deterministically. Semantic route quality is evaluated only through the final blind judgment. Attaching the final-finding verification policy directly to selectors would overstate what is independently verified.
- `verification_independence_uninspectable` is retained because topology v0 cannot prove the verifier/session/world-anchor separation declared in `verification.policy.json`.

## Questions for the independent reviewer

1. Does the topology preserve the ReviewGraphen/CaseGraphen semantic boundary above?
2. Are fresh single-choice Qwen selectors a fair intervention relative to the m16 single-session chained baseline, given the preregistered interpretation ceiling?
3. Are the three strict expansion barriers and cross-arm backend serialization justified?
4. Is final protocol validation correctly separated from blind quality judgment?
5. Should selector calls receive a distinct route-quality verification policy, or is final-outcome judgment sufficient for this feasibility experiment?

No topology review, mutation, worker enablement, or execution has occurred.
