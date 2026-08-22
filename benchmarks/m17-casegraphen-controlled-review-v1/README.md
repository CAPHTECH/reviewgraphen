# m17 CaseGraphen-controlled ReviewGraphen experiment

This experiment compares the frozen m16 single-session chained controller with a checkpointed external-runtime DAG. The runtime governs model-node dependency order, budgets, attempts, barriers, typed handoffs, and liveness. CaseGraphen records the accepted topology and governs arm-level authority, readiness, evidence review, transitions, and replay. ReviewGraphen continues to own projection construction, source IDs, declared information loss, the consumed-card ledger, findings, grounding checks, and review-state meaning.

Qwen convergence is therefore an effect of the external checkpoint runtime, not of CaseGraphen scheduling. The CaseGraphen part of the experiment asks whether that runtime can be governed without laundering its completion claims into accepted review state.

The design is proposal-only until an independent topology review accepts the exact topology and deployment-policy manifest. Generating or linting these files does not create or mutate a CaseGraphen case space.

```sh
python3 benchmarks/m17-casegraphen-controlled-review-v1/scripts/generate_design.py
casegraphen graph lint \
  --input benchmarks/m17-casegraphen-controlled-review-v1/design/execution.topology.json \
  --format json \
  --output benchmarks/m17-casegraphen-controlled-review-v1/design/graph.analysis.report.json
python3 benchmarks/m17-casegraphen-controlled-review-v1/scripts/bind_proposal.py
```
