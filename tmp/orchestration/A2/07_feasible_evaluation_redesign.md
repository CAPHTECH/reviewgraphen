# Feasible evaluation redesign

Audit basis: repository revision `1a2fefb10c1bffb088f8377e102d391086e094a9`,
2026-08-23 (Asia/Tokyo). This is a replacement for A1's evaluation proposal,
not a change to it. No provider or network endpoint was contacted.

## 1. Correction to the A1 design

A1's `83 positive × 2 arms × 3 replicates + controls = 618` model calls is not
an executable preregistration. At a 600-second ceiling it consumes 103 model
hours, and the actual repository observations are commonly slower. More
importantly, the conjunction used to define a positive unsafe unit was never
censused. The two proposed external tag ranges were not locally present and
were not fetched in this audit. Therefore neither “83 units exist” nor even a
useful unsafe-boundary prevalence was measured.

The replacement design makes three changes:

1. select the wedge only after a model-free prevalence/packet gate;
2. use a paired, one-invocation-per-arm design with the **commit** as the
   independent cluster; and
3. freeze an endpoint that can show a large practical effect with 10–120 commit
   clusters, without relabeling a judge proxy as defect truth.

Population precision and recall are not primary claims in this design. They
require a gold defect universe that the available budget cannot create. The
primary model endpoint is instead a strictly validated grounded disposition
completion rate. Stage 0 separately establishes deterministic review-surface
utility at much larger `n` without a model.

## 2. Available execution resources and measured throughput

### 2.1 Product adapters

The generic request type admits Codex CLI, Claude CLI, Codex app-server, and
replay (`crates/reviewgraphen-runtime/src/generic.rs:98-124`). The live driver
wires Codex and Claude, rejects app-server as unsupported, and makes replay a
byte source (`generic.rs:333-375`). Thus:

- **Replay** is available for deterministic reruns but adds no new stochastic
  observation.
- **Codex CLI and Claude CLI** are executable adapter paths. On this host the
  installed binaries reported `codex-cli 0.147.0` and Claude Code `2.1.233`.
  Credentials, provider quota, latency, and a successful current model call were
  not tested; they must not be treated as procured evaluation capacity.
- **Codex app-server** is not capacity: both runtime and reviewer fail closed
  (`generic.rs:364-370`; `crates/reviewgraphen-reviewer/src/process.rs:551-560`).

### 2.2 Historical local Qwen backend

M15–M18 pin the same model-list hash
`99f80c…b7ab` and health hash `29aa6e…f74`. M19's captured identity reported a
healthy service with Qwen3.8-27B MLX 4-bit and 8-bit models loaded
(`benchmarks/m19-asymmetric-qwen-utility-v1/runs/r1/backend-identity.json:1-32`).
The runtime endpoint is `192.168.68.71:11999`, declared an **exclusive**
resource; the external runtime declares `max_parallel: 1`
(`benchmarks/m19-asymmetric-qwen-utility-v1/scripts/generate_design.py:110-113,484-491`).
The hashes prove identity of the captured observation, not present availability.
Network use is prohibited for this task, so current health and access are
**unmeasured**. Every future run must repeat the identity/health gate before
spending the budget; a mismatch or unavailable endpoint stops the model stages.

Observed end-to-end or node totals are the appropriate planning basis:

| Observation | Model time/result | Evidence |
| --- | ---: | --- |
| M15 4-bit/low | 900 s timeout, no report | `benchmarks/m15-qwen-intelligent-reviewgraphen-v1/RESULT.md:3-17` |
| M15 8-bit/high | 901 s timeout, no report | same, `:19-41` |
| M16 8-bit/high | 782 s, valid report | `benchmarks/m16-qwen-chained-reviewgraphen-v1/RESULT.md:7-25` |
| M16 4-bit/low | 901 s timeout | same, `:39-50` |
| M17 4-bit/low, four fresh nodes | 772.629 s, valid report; final alone 380.166 s | `benchmarks/m17-casegraphen-controlled-review-v1/RESULT.md:8-12,44-47` |
| M18 resident 8-bit/low, four nodes | 1,380.020 s, valid report | `benchmarks/m18-8bit-low-checkpoint-review-v1/diagnostics/resident-timeout600-r1/comparison.json:12-36` |
| M19 heterogeneous probe suite | 2,457.016 s, with a 600 s timeout and parse failure | `benchmarks/m19-asymmetric-qwen-utility-v1/runs/r1/result.json:1` |

M17 enforced 240-second selection and 420-second final ceilings
(`benchmarks/m17-casegraphen-controlled-review-v1/scripts/run_arm.py:35-36,79-168`).
M19 used 600 seconds for 4-bit/low, 1,200 for 8-bit/medium, and 420 for the
judge (`generate_design.py:445-455`). These are single-replicate runtime
observations, not stable throughput estimates. They nevertheless falsify a
600-second-every-time assumption.

### 2.3 Planning assumption

Use one fixed, bounded, no-tool **Qwen 4-bit/low** invocation per arm with a
900-second hard ceiling, then one blind judge batch per commit with a 90-second
planning allowance. The arms run serially on the exclusive backend. Reserve
approximately 15% for identity gates, validation, failures, and artifact
sealing. Do not use the four-node adaptive route: this experiment tests a
preconstructed obligation packet, so selectors would spend budget without
changing the estimand.

Codex judge time is counted in the model budget even though historical judge
calls took 48–78 seconds
(`benchmarks/m19-asymmetric-qwen-utility-v1/runs/r1/aggregate.json:31-45`;
`benchmarks/m17-casegraphen-controlled-review-v1/aggregate.json`, blind-judge
record). If Codex access is not procured, Stage 1 can still test its primary
mechanical endpoint but all judge outcomes remain unmeasured and Stage 2 does
not start.

## 3. Frozen unit, arms, and outcomes

### 3.1 Independent unit and pairing

One unit is one base→head **commit cluster**. Multiple D obligations, claims,
call edges, or model outputs from that commit are correlated observations and
never increase `n`. At most one deterministically ranked D obligation is used
for the model packet; all other obligation IDs remain in the audit denominator
and are reported as not sampled for model evaluation. Repository is a blocking
stratum, not an independent observation. No stochastic replicate is used in
the initial study; breadth across commits is worth more than repeated sampling
of one commit under this budget.

Each commit receives both arms in randomized order:

- **Baseline:** base/head diff plus the same byte/token budget and the same
  frozen output schema, without the D relation, endpoint-first projection,
  universe, or loss/unknown records.
- **ReviewGraphen:** one frozen D obligation and subject-first packet, with exact
  caller/callee source IDs, universe revision, exclusions, limitations, and
  information loss.

The same model identity, effort, timeout, maximum findings, and no-tool policy
apply to both. The evaluator, not the implementation agent, builds both packets.

### 3.2 Primary model endpoint

`grounded_disposition_completed(commit, arm) = 1` iff, before 900 seconds:

1. the process exits successfully and output passes the frozen schema;
2. unit, property, target, projection, and source-ID closure validate;
3. every claim cites admitted sources and no policy/tool violation occurred;
4. the output contains either a concrete property-specific claim or a concrete
   typed abstention tied to an admitted limitation/information-loss item; and
5. the raw response and parsed result hashes are retained.

A generic “looks safe”, empty response, malformed JSON, timeout, invented ID,
or ungrounded claim is `0`. This metric does **not** say the claim is true. It
captures a practical failure repeatedly seen in M15–M19: no usable, auditable
disposition at all.

The paired estimand is `P(B=1)-P(A=1)`. Let `b` be commits where ReviewGraphen
alone completes and `c` those where baseline alone completes. Use an exact
two-sided McNemar/binomial test on `b+c`, alpha .05; publish all four paired
cells and an exact interval. Do not count findings as independent trials.

Mathematically, the smallest significant sample is six unanimous improvements
(`b=6,c=0`, two-sided `p=.03125`). That result is too brittle to operate on.
The frozen operational minimum is 10 pairs, allowing one reverse discordance:

| Total pairs | Frozen “clear benefit” boundary | Absolute paired improvement | Exact two-sided p |
| ---: | --- | ---: | ---: |
| 10 | `b≥8, c≤1` | at least .70 | .03906 at 8/1 |
| 40 | `b≥18, c≤7` | at least .275 | .04329 at 18/7 |
| 120 | `b≥36, c≤18` | at least .15 | .01983 at 36/18 |

These are preregistered decision boundaries, not promises of 80% power under an
unknown joint distribution. Ten pairs can only establish an enormous effect;
40 a large effect; 120 a moderate effect. If the observed discordance misses
the applicable boundary, the practical-benefit gate fails rather than being
rescued by a secondary metric.

### 3.3 Frozen secondary endpoints and safety gates

- blind, arm-hidden `issue_should_be_created / should_not / unable` disposition
  per normalized claim; judge approval remains non-authority proxy evidence;
- commits with at least one judge-positive, per 1,000 model seconds and per
  admitted source byte (commit-clustered paired randomization interval);
- false-positive commits in D-eligible clean/refactor controls;
- timeout, parse failure, grounded abstention, claim count, source bytes, and
  wall time.

No secondary endpoint can turn a failed primary boundary into success. For the
40/120-pair stages, at least 20% of commits are evaluator-verified clean/refactor
controls; ReviewGraphen must have no more judge-positive clean commits than the
baseline plus one. This is a safety gate, not a powered non-inferiority claim at
small `n`. Root recall may be reported descriptively only for commits whose
root was independently established before arm outputs were opened.

## 4. Model-free Stage 0

After implementation and artifact freeze, the evaluator resolves three or more
Rust repositories and takes 100 first-parent commits per repository. The unit is
all 300 commit clusters, including zero-obligation commits. The primary Stage 0
endpoint is **auditable bounded review-surface success**. All gates must pass:

1. at least 45/300 commits (15%) yield at least one applicable D obligation and
   at least 60 applicable obligations exist in total;
2. exact caller and callee subjects are present in at least 95% of applicable
   packets; every remainder is a typed excluded/unknown item naming its source
   and recovery reference—never silent loss;
3. the median admitted source bytes are at most 50% of a frozen whole-changed-
   production-files baseline and p90 is at most 64 KiB;
4. two clean rebuilds produce identical obligation/universe/source IDs,
   applicability, windows, loss declarations, and canonical bytes for 100% of
   commits; and
5. `direct_calls=partial` plus its source-backed limitation and rule-level
   enumeration gap remain visible for 100% of repository snapshots. No report
   may claim complete call coverage.

The 15% prevalence threshold is intentionally below this audit's loose local
proxy because exact-public-callee and syntactic-unique resolution will reduce
it. It is still high enough to make the wedge routinely exercisable. A failure
of any gate stops the evaluation: no model run, no “feasibility success.”

Stage 0 establishes reproducible targeting, denominator honesty, subject
retention, and substantial context reduction. It does not establish defect
detection or reviewer accuracy.

## 5. Stage 1 and conditional Stage 2

### 5.1 Six model-hours: Stage 1

- **Scale:** 10 paired commit clusters = 20 Qwen calls. At the conservative
  900-second ceiling this is 5.0 Qwen hours; ten batched judges need at most
  0.25 hours by plan, leaving about 0.75 hours for gates/validation. Eight units
  are evaluator-selected D changes with a reviewable contract question; two are
  D-eligible clean/refactor controls.
- **Primary claim:** only a very large grounded-completion benefit, using
  `b≥8,c≤1`. No population precision/recall claim.
- **Advance rule:** Stage 0 passes, the primary boundary passes, no leakage or
  identity mismatch occurs, and there is no ReviewGraphen-only judge-positive
  clean control. Otherwise stop and publish failure/indeterminate reason.

### 5.2 Twenty-four cumulative model-hours: Stage 2A

- **Scale:** add 30 fresh pairs for 40 total: 80 Qwen invocations (20 ceiling
  hours), 40 judge batches (1 hour), about 3 hours reserve. Use 32 substantive
  D changes and eight clean/refactor controls total.
- **Primary claim:** a large grounded-completion benefit only if
  `b≥18,c≤7` over all 40 frozen pairs. Secondary judge-positive efficiency and
  clean-control rates are corpus-bound proxies.
- **Honest ceiling:** stable completion evidence over the frozen repository
  strata; no general Rust precision/recall and no verified-defect claim.

This is the **recommended authorized design**. It is small enough to finish and
large enough not to depend on near-unanimity.

### 5.3 Seventy-two cumulative model-hours: Stage 2B

- **Scale:** add 80 fresh pairs for 120 total: 240 Qwen calls (60 ceiling
  hours), 120 judge batches (3 hours), about 9 hours reserve. Use 96 substantive
  changes and 24 controls total.
- **Primary claim:** a moderate grounded-completion benefit only if
  `b≥36,c≤18`. Repository-blocked estimates and leave-one-repository-out
  sensitivity are mandatory.
- **Honest ceiling:** stronger cross-commit completion and proxy-yield evidence
  for the frozen repository class. Gold root precision/recall still requires a
  separately funded oracle study.

The 24/72-hour extensions run only after Stage 1 passes. Unspent budget is not a
reason to weaken thresholds, add repositories post hoc, rerun failures, change
the primary metric, or count obligations as independent units.

## 6. Wedge × evaluability

| Wedge | Stage 0 | 6 h | 24 h | 72 h |
| --- | --- | --- | --- | --- |
| A: changed unsafe boundary | Local development proxy was 0/143 production-Rust commits even under a permissive unsafe-file upper bound; likely prevalence stop | Cannot reliably source 10 independent positives | Census risk dominates | External prevalence remains unmeasured; more budget does not create units |
| C: public API compatibility | Deterministic comparator utility is evaluable, but the rustdoc/toolchain/cfg adapter does not exist | Model study is not the natural primary value | Possible only after the 11–14-unit implementation and toolchain pin | Strong mechanical study possible; still a different, larger product |
| D: changed public callee over accepted direct calls | High local proxies and no new ingest capability; exact gate still required | 10-pair large-effect pilot is feasible | **40-pair recommended confirmation** | 120-pair moderate-effect extension feasible |

Evaluation feasibility therefore changes the recommendation from A to D. C
remains the fallback if exact D Stage 0 prevalence is below 15% or if the product
is specifically a public-library compatibility tool.

## 7. Holdout separation that can actually work

“Do not tell the implementation agent the repository names” is not blindness
when the same Unix identity can read the files. A hidden directory or prompt
instruction on this machine is not an access boundary.

The valid sequence is:

1. Before implementation, freeze this protocol, selection algorithm, schemas,
   thresholds, and evaluator role—but not repository names/ranges.
2. Finish implementation; record Git revision, build artifacts, rule/profile/
   extractor hashes, and evaluation container hash. The implementation unit is
   then closed and receives no further holdout-driven edits.
3. A human/evaluator that did not implement the slice resolves repositories and
   commit ranges **after freeze**, on another account with ACLs denying the
   implementation account, or on a separate machine. The evaluator authors the
   enumerator, arm packet builder, leakage scanner, control labels, and root
   mappings. These scripts are not assigned to the implementation agent.
4. The evaluator publishes a hash commitment to the canonical manifest, packet
   inventory, randomization seed commitment, and image hashes before model
   execution, while retaining names/content behind the actual access boundary.
5. The runner exposes only randomized arm-neutral packets to model sessions.
   Results and raw hashes are sealed before repository names, controls, roots,
   and arm labels are revealed. Then the full manifest and selection failures
   are published.

If no separate account/machine/evaluator is available, run the same protocol as
an **open development evaluation**. It can test determinism and observed paired
utility, but must not be described as organizationally blind or holdout-valid.

## 8. Freeze rule

Once the implementation revision and evaluator protocol are committed, the
primary endpoint, timeout classification, unit, repository selection rule,
sample sizes, thresholds, missing-data rule (`failure=0`), and stop/advance
rules do not change. Any repair creates a new versioned study and cannot replace
the failed frozen result. This redesign is legitimate now because it precedes
implementation; changing it after seeing arm results would not be.
