# Practical utility experiments — completed 2026-09-14

User objective: それらすべてを検出できなくてもよいので役に立つレベルでReviewGraphenが成立するか実験を繰り返して。 $herdr-orchestration $codex-implementation

This investigation is complete, but it did not demonstrate practical utility. The preceding
known-wiring calibration in README.md is not counted as unknown-defect discovery.
Duplication and missing abstraction are explicitly in scope; all fourteen sloppiness
categories need not be detectable.

## Completion state

- All `rg-utility` workers and the task orchestrator were closed after their artifacts
  were persisted. The `rg-utility-main` watch registry has no active agents.
- Task contract revision 1: `/tmp/reviewgraphen-utility-orch.vRtLuM/TASK.md`.
- Mutable state and evidence inventory: `/tmp/reviewgraphen-utility-orch.vRtLuM/STATUS.md`.
- Parent independently observed source HEAD `c0747614c2c20a51771f6f6495f196682e3912fd`.
- Root pre-existing dirty changes remain in place; experiment workers use isolated scratch locations.

The first pilot contract and candidate inventory must be checked before sizeable
implementation. Each measured batch must freeze its corpus, rule/context configuration,
comparison conditions and adjudication criteria before execution. Compare against a
competent baseline, retain rejected and unknown candidates, and report confirmed root
causes separately from tentative abstraction opportunities. Version-specific compatibility
code is not automatically a missing abstraction; include justified-separation controls.

## Exploratory checkpoint observation

The Sol pilot draft enumerated six responsibility groups. Parent independently
opened `context.rs:670`, benchmark `lib.rs:344`, and core `id.rs:99-155` in the
pinned clone. The SHA-256 checks differ textually (ASCII hex vs lowercase-only),
but `ContentHash::parse` normalizes to lowercase, `sha256` generates lowercase,
the backing field is private, and deserialization delegates to `parse`.
The observed predicate difference therefore does not establish reachable
behavioral divergence through those public construction paths. It is an
exploratory rejected defect hypothesis, not a measured B/C result or a proof
that repeated validation has no maintenance cost.

Parent requested three changes before measured execution: natural versioned
groups are not pre-proven negative controls; opaque packets must omit analyst
disposition guesses; bounded type/contract/caller expansion must be available
equally to both arms. A standalone Python packet experiment must be attributed
to the methodology/harness, not the existing ReviewGraphen executable. That
pilot is an intermediate step; practical tool reuse/extension remains unproven.

No new measured pilot result has been accepted at this checkpoint. No practical
viability, AI-versus-human quality difference, or HigherGraphen-specific advantage is
claimed here. Final artifacts will be integrated only after checking the actual evidence.

## Final experimental verdict (2026-09-14)

The experiments completed without establishing practical, tool-attributable utility for
ReviewGraphen on this corpus. This is a negative result within the measured scope, not a
claim that obligation-driven context selection can never help.

- Standalone structural-sloppiness F1 (five natural groups, diagnostic after one
  calibration-arm failure): orchestrator-audited useful findings were baseline 0 and
  methodology arm 0. The methodology arm under-called the only structural finding.
- Existing ReviewGraphen rule `relation.changed_public_callee@1`, D-H1 (one screened
  history pair, three obligations): useful findings were baseline 0 and RG 0. Its original
  baseline lost all large diff sections and the batch was downgraded before review.
- Exposed-pair fairness follow-up D-H1f reused the same RG packets and gave the baseline
  declarations plus U3 target-relevant hunks under the same 24 KiB materialized-packet
  ceiling. Every role with relevant hunks retained at least one. Final useful findings
  were again baseline 0 and RG 0.

In D-H1f, RG alone raised a public-API capacity-accounting hypothesis on one packet. The
blinded adjudicator agreed, and the baseline had received the relevant `raw.len()` /
`parse_fake_reviewer_output_with_capacity` hunk, so missing baseline evidence does not
explain the difference. However, the independent verification reviewer rejected the
generated integration test because it depended on crate-private test fixtures and could
not exercise the claimed caller path in the permitted form. No Cargo test ran. The claim
therefore remains tentative and unattributed. With one exposed pair and one reviewer draw
per arm, context-selection effect and reviewer variance cannot be separated.

RG was also more expensive in D-H1f: 7 versus 2 expansion items, 72,255 versus 54,561
context bytes, 251,394 versus 230,161 ms arm time, and 764,683 versus 638,960 observed
thread tokens, before charging the failed translation and review (1,227,400 additional
tokens) to RG. Because both useful sets are empty, no efficiency claim follows.

Authoritative scratch records:

- `/tmp/reviewgraphen-utility-orch.vRtLuM/runs/F1-exploratory-join-S1`
- `/tmp/reviewgraphen-utility-orch.vRtLuM/runs/D-H1-BATCH-RECORD-r2.md`
- `/tmp/reviewgraphen-utility-orch.vRtLuM/runs/D-H1f-BATCH-RECORD.md`
- `/tmp/reviewgraphen-utility-orch.vRtLuM/runs/D-H1f-score-audit.json`

The narrow remaining signal worth a future held-out test is whether RG's selected support
windows reproducibly surface the capacity-accounting class when a diff-bearing baseline
has the same relevant hunk. D-H1f does not establish that result. No further infrastructure
work is justified before a second frozen pair exists.

## A7 / D-H2 held-out continuation

That narrow signal was tested once on a different, prospectively selected history pair.
A7 froze the previously unscreened suffix of the existing order and stopped at its first
qualifier: `6b7edbe7` -> `42adab7d`, with two applicable changed-public-callee obligations
and no deferred obligations. Seven earlier suffix pairs had no applicable obligation.

The repaired U3 baseline and the ReviewGraphen projection both fit the same 24 KiB
materialized ceiling. Both fresh reviewers judged both obligations compatible; fresh
blinded adjudication rejected both candidate findings and credited neither response with
an actionable finding. No executable check ran because there was no positive claim.
Direct reading confirmed only the bounded result: both inspected callers propagate the
fallible `prepare_context` result and no conflicting caller assumption was found in the
cited paths. It does not prove global compatibility.

D-H2 result: useful baseline 0, ReviewGraphen 0. The RG-only capacity-accounting
hypothesis from D-H1f did not recur. The earlier signal therefore has no observed
repeatability, and the measured work still establishes no tool-attributable utility.
The authoritative record is
`/tmp/reviewgraphen-utility-orch.vRtLuM/runs/D-H2-BATCH-RECORD.md`.

[R] Additional reviewer-arm runs on `relation.changed_public_callee@1` now have low
expected value: the rule was sparse in both screens and produced no verified useful item.
This inference would change if a larger prospective sample yielded repeated actionable
findings. The higher-value next experiment is a small candidate-discovery test for
repeated policy and missing-abstraction boundaries—the original structural-sloppiness
target—not more harness work around this relation rule.

## A8 exact-shape abstraction-candidate screen

The first candidate-discovery pilot ran on pinned snapshot `c0747614`. A scratch
ReviewGraphen ingest produced a snapshot-bound ProgramSpace; an external deterministic
normalizer then grouped non-test production free functions by exact normalized token
shape. This normalizer is experiment code, not a ReviewGraphen rule.

The production denominator was 1,861 functions and yielded 75 groups. Versioned v4/v5
and v3/v4 families dominated the top ranks, demonstrating why version separation must be
kept as a candidate-control stratum rather than equated with either a defect or a reason
to ignore all shared helpers. The five frozen non-version-dominated groups produced three
unverified structural candidates and two tentative candidates after bounded source
inspection; no behavioral defect was verified.

The clearest candidates were duplicated excerpt slicing across benchmark/runtime,
duplicated dangling-reference validation inside core, and duplicated `Severity` wire-text
mapping across core/store. The two tentative groups were benchmark reason accumulation
and a too-broad cluster of bounded-length checks crossing distinct error layers.

ReviewGraphen's useful contribution was narrower than candidate discovery: it supplied
snapshot-bound function locations, labels, test/public attributes and source content
hashes. A stale-source mutation was rejected, and two candidate runs were byte-identical.
Higher relation facts did not change any of the five dispositions: containment duplicated
path/name information, common resolved callees were empty, and partial direct-call facts
could not support negative conclusions. The 246 MiB whole-repository ProgramSpace also
required raising the file ceiling twice because unrelated benchmark artifacts blocked
ingest; the request has no path filter.

Result: modest reuse value as an extraction/provenance substrate, but no observed
HigherGraphen-specific advantage and no current ReviewGraphen abstraction-discovery
capability. The authoritative diagnostic record is
`/tmp/rg-abstraction-v1.jyupVg/A8-RESULT.md`.

### A9 plain syntax baseline

A direct `syn` baseline then scanned only `crates/*/src/**/*.rs`. Two runs took
3,169 ms and 3,130 ms and were byte-identical after removing elapsed time. It found
61 exact-shape groups and reproduced all five frozen A8 member sets. Its report was
44 KiB, compared with the 246 MiB whole-repository ProgramSpace and 202 KiB A8
projection; the successful ReviewGraphen ingest exceeded 30 seconds, but exact elapsed
time was not recorded [U]. The universe counts are not identical because the scratch
tokenizers/test-scope walkers differ, so only the frozen-five reproduction is claimed.

This strengthens the negative tool-attribution result for exact clones: a normal syntax
pass found the same reviewed candidates much faster, without HigherGraphen relations.
ReviewGraphen retained an auditability advantage—snapshot identity, stable IDs, source
hashes and explicit extraction limitations—but that is evidence management, not clone
detection. Full record:
`/tmp/rg-abstraction-v1.jyupVg/A9-PLAIN-BASELINE-RESULT.md`.

### A10 near-clone diagnostic

A five-token-shingle screen was calibrated against the already-known C-02 path-policy
pair, then run on cross-file non-exact pairs. Its sensitivity gate was invalid: the
calibration measured raw Jaccard, while the natural algorithm subsequently removed
shingles occurring in more than 24 functions. The known pair was not emitted. A10
therefore supports no recall or coverage claim.

It emitted one natural pair, `d2_scan_primitive` and `scan_json_primitive`. Bounded source
inspection classified it as an unverified structural candidate: both implement the same
JSON primitive terminator policy and differ only in error construction. No current
behavioral divergence was observed, and no ProgramSpace relation fact changed the
decision. This candidate came from external shingle code, not a ReviewGraphen rule.
Record: `/tmp/rg-abstraction-v1.jyupVg/A10-RESULT.md`.

### A11 corrected near-clone diagnostic

A direct-Jaccard v2 removed A10's frequency-based shingle loss. Its exact
calibration emitted the known C-02 pair. On the natural universe it considered
358,488 length-compatible cross-file pairs and emitted 416 candidates; two runs were
byte-identical. Nevertheless the known calibration pair ranked only 269th.

The frozen natural top five were all rejected. They were combinations of unrelated enum
or context-to-string match functions; normalizing all identifiers erased their domain
types and variant names, making different policies look identical. ProgramSpace relation
facts did not change any rejection. The current graph has no complete signature/type,
data-flow or declared-policy relation able to restore the lost distinction.

Result: corrected sensitivity but unusable ranking precision, with no useful candidate
in the frozen top five and no HigherGraphen-specific contribution. Further threshold
tuning on this corpus is stopped. Record:
`/tmp/rg-abstraction-v1.jyupVg/A11-RESULT.md`.

### Consolidated direction after A8–A11

Stop implementing threshold-based clone similarity inside ReviewGraphen. The plain
`syn` baseline reproduced every reviewed exact-shape group in about 3.2 seconds, while
the corrected near-clone screen produced 416 candidates whose frozen top five were all
false positives; the known policy pair ranked 269th. Current ProgramSpace relations did
not improve either result.

The next smallest empirical comparison, if continued, is an existing Rust-capable
Type 1–3 detector such as jscpd as an upstream candidate producer, followed by
ReviewGraphen for snapshot binding, obligations, evidence, verification and explicit
keep-separate reasons. No such detector is installed locally, and no installation has
been attempted. HigherGraphen remains a hypothesis for representing one policy across
three or more implementations and exceptions, not for discovering similarity. Any
utility claim must beat a normal table/relation representation on confirmation time,
stale-evidence mistakes or maintenance burden.

### A12 implemented-model check

Source inspection found that the proposed post-detector role is not executable yet.
The conceptual model names Subgraph and Invariant targets, but aggregate validation
reserves `subgraph` for a snapshot-targeted unknown capability gap, while Invariant is
an accepted ProgramSpace property. Generic v4 exposes only one relation and one node
rule. A similarity group therefore cannot currently become one non-authoritative
shared-policy obligation without a ReviewGraphen domain/runtime change.

The smallest faithful addition is a snapshot-bound external candidate-group observation
followed by a group-targeted ReviewSpace obligation. It must not assert semantic
equivalence or create an accepted invariant. This is a model-gap result, not evidence
that HigherGraphen improves review. Full source-bound record:
`/tmp/rg-abstraction-v1.jyupVg/A12-MODEL-GAP.md`.

### A13 overlap trap

The 416 A11 pairs cover 333 unique functions; 173 functions occur in multiple
pairs and the maximum degree is 11. This apparent higher-order structure is not
semantic evidence. The known path-policy pair is bridged by stronger similarity
edges to `valid_version` and `valid_profile_name`, which direct source inspection
shows implement unrelated input languages. Connected components or transitive
closure would therefore turn one plausible pair into a false shared-policy group.

Constraint: preserve detector-emitted candidates as non-authoritative observations;
never infer a semantic higher cell from similarity-edge closure. Overlap can schedule
review, but each member still needs a common policy predicate. Record:
`/tmp/rg-abstraction-v1.jyupVg/A13-OVERLAP-TRAP.md`.

### A14 one-step group evolution

The unchanged plain scanner was run once on first parent `a64e9bd5`. Base/target
counts were 1,464/1,490 bounded functions and 60/61 exact groups. Fifty-nine
groups retained identical shape and membership; one existing group gained a
member; one group was target-only. All five frozen A8 groups were unchanged.

The changed group is actionable structural evidence: adding generic-review v4
added `v4_request_is_valid` to the identical v2/v3 request-schema validator
family. No defect is verified, but a real feature change extended the copy.

The experiment's shape-only candidate ID stayed constant as membership changed.
Any product intake must therefore separate persistent `shape_id` from a
snapshot/member/config-bound `group_instance_id`; otherwise prior evidence can
survive a changed denominator. This demonstrates a plausible ReviewGraphen
staleness-management role, not a HigherGraphen advantage. Record:
`/tmp/rg-abstraction-v1.jyupVg/A14-RESULT.md`.

### A15 temporal screen

The unchanged plain scanner covered ten frozen first-parent revisions that
changed `crates` in 31.629 seconds total. Across nine selected transitions it
recorded 511 persistent unchanged-group instances, one membership growth, five
births, and no shrink, replacement or death. Births were not adjudicated.

The sole growth event was the v2/v3 request validator family gaining v4; direct
reading retained it as an unverified structural opportunity. This makes
copy-family expansion a substantially cheaper priority signal than the A11
static near-clone list in this repository slice, without claiming comparable
precision or a repository-wide rate.

Provisional sloppiness property: a change extends an existing implementation
family without introducing or reusing a shared policy boundary. Emit one
obligation per changed group instance, never per pair. Enumeration still needs
only syntax plus snapshot comparison; ReviewGraphen's plausible role begins at
identity, denominator, staleness and evidence tracking. Record:
`/tmp/rg-abstraction-v1.jyupVg/A15-RESULT.md`.

### A16 all five births reviewed

All five A15 exact-group births were source-reviewed: three structural-unverified,
two keep-separate, zero verified defects. The structural groups were the v2/v3
request validators, duplicate draft/durable occurrence renderers, and duplicate
legacy/rule-neutral exclusion-ID preimage builders. The validator group later
grew to v4, so birth and growth are two events for one evolving family.

The separations are important counterexamples. `initial_remaining` is duplicated
inside a deliberately implementation-independent validation oracle; sharing it
would weaken the check. Adjacent `boolean` and `unsigned` report accessors already
share field lookup and preserve distinct output-type conversions.

This makes a durable `keep-separate` decision and reason part of the minimum
product contract. A detector cannot equate clone birth with debt. The observed
3/5 structural yield is descriptive for five births only, and no HigherGraphen
advantage is established. Record: `/tmp/rg-abstraction-v1.jyupVg/A16-RESULT.md`.

### A17 literature correction

Clone genealogy, inconsistent change and late propagation are established
research areas, so temporal clone extraction is not a ReviewGraphen novelty.
Primary studies also disagree with treating clones as uniformly harmful: one
large manually adjudicated study confirmed faults among inconsistent groups,
while a release-level study reported only 1.02–4.00% defect-introducing
genealogies in its three systems.

AI-specific 2026 results narrow the premise further. One study found clones in
497 of 7,851 AI-agent PRs and many recurring across commits, but had no human
rate control. Another six-project/350-lineage study reported humans introduced
85.71% of clones versus agents' 14.29%, with humans predominantly maintaining
agent-created lineages. The research must not assume AI has the higher clone
rate.

Revised hypothesis: given upstream genealogy events, test whether ReviewGraphen
reduces human classification time and stale-decision mistakes for co-change,
intentional separation and consolidation. This is governance/evidence utility,
not clone detection. Record: `/tmp/rg-abstraction-v1.jyupVg/A17-LITERATURE.md`.

### A18 40-snapshot exact genealogy

The unchanged plain exact-shape scanner was extended to 40 frozen first-parent
code revisions, covering 39 selected transitions in 117.547 seconds of recorded
scan time. It found 2,161 unchanged group instances, 11 births and two growths,
but zero shrink, death, mixed membership change, identical-member migration or
later reappearance. The frozen risk-event selector therefore returned an empty
review set; no additional source body was inspected and no finding was inferred
from the zero.

One growth was the previously reviewed v2/v3 request-validator family gaining
v4. The other joined corresponding test-fixture identity helpers. This extends
the evidence that exact genealogy can cheaply prioritize copy-family birth and
growth, but it did not surface an inconsistent-change or late-propagation signal
in this repository window. It does not cover semantic or Type-2/3 divergence.

[R] The next useful test is not another global similarity threshold. Track a
small known-policy family by stable member identity and emit an event when only
a subset changes or its policy tests diverge. Then compare a plain event table
with ReviewGraphen's snapshot, denominator, evidence, keep-separate and staleness
management. Record: `/tmp/rg-abstraction-v1.jyupVg/A18-RESULT.md`.

### A19 asymmetric change in reviewed families

A19 reused A18 records and Git objects without another source scan. For the five
A16-reviewed exact families, only eight adjacent family-transition instances
existed. None contained a partial-member or all-member raw source change. The
only membership event was the known v2/v3 validator family gaining v4, while
the two stable member bodies remained unchanged.

The frozen selector therefore produced no new review obligation. A scratch
single-member range mutation produced exactly one asymmetric event, showing the
event path was sensitive to the injected change. This is not natural-change
recall evidence.

[R] Further exact-history expansion has low expected value for this corpus.
The next experiment should begin from a supported shared-policy identity and
track asymmetric semantic or test behavior, rather than lower a global clone
threshold. Record: `/tmp/rg-abstraction-v1.jyupVg/A19-RESULT.md`.

### A20 non-exact policy-family expansion

The corrected C-02 five-member path-policy set was fixed before a presence scan
over the same 40 Git snapshots. The family grew naturally from two to three
members when the benchmark subsystem added `safe_relative_path`, then from
three to five when one feature commit added profile `validate_path` and ingest
`normalized_repository_path`. Direct pre/post declaration checks confirmed the
three additions.

All five implement the previously adjudicated base grammar, while their extra
checks form three variants: program/m6, profile/ingest and benchmark. Because
their normalized bodies are not exact, A18 did not expose these additions as
one genealogy. This is a concrete example of the user's concern: new subsystems
reimplemented an existing policy family instead of reusing an explicit boundary.

No defect or universal grammar is verified. [R] A shared conformance corpus or
policy matrix is a safer prospective boundary than forcing one production
helper, because it can retain boundary-specific error types and explicit
exceptions. ReviewGraphen's possible value is reopening one snapshot-bound
group obligation when membership changes; discovery was ordinary Git/table
processing, and superiority over that table remains unmeasured. Record:
`/tmp/rg-abstraction-v1.jyupVg/A20-RESULT.md`.

### A21 current executable-surface boundary

The current experimental analyzer was checked against its ADR, contract,
implementation and CLI source. It implements only
`changed_input_consumer_bridge_mismatch@1`: a fixed one-hop marker/containment
predicate over accepted `changed_by` facts for public free functions. It has
sound non-authority and full-input staleness boundaries for that wiring trial.

It has no candidate-group input, policy-family identity, member/exception set,
membership transition, prior-decision staleness or group-obligation reopening.
The five A20 validators are private and therefore also outside its eligibility.
Consequently A20 is not evidence of utility for the existing executable.

[R] Any A20 implementation must be a new versioned contract, not a semantic
expansion of v1. Before implementing it, compare against a plain group table;
otherwise the trial would establish only that ReviewGraphen can be wired.
Record: `/tmp/rg-abstraction-v1.jyupVg/A21-CURRENT-SURFACE.md`.

### A22 plain-table baseline

An 82-line plain Python table processed A20's two natural membership events
and a manually curated five-member policy registry. It separated stable policy
identity from snapshot/member/source-bound instances, marked synthetic prior
decisions stale after both membership changes, emitted one reopened group
obligation per event, and preserved common checks, member exceptions and
unverified assumptions.

The 7,555-byte output built and exactly recomputed in 0.04 seconds each in one
local observation; a second output was byte-identical. Mutating either a target
instance ID or prior-decision applicability was rejected with exit 4.

This proves only deterministic mechanics. Prior decisions were synthetic, the
policy registry was curated, and no human classification time or natural stale
mistake was measured. Nevertheless it establishes that ordinary JSON/table
processing already handles A20 representation and direct stale-decision
prevention. ReviewGraphen must improve a workflow outcome—verification time,
missed exceptions, evidence reuse, revision conflict or audit cost—to add
utility. Record: `/tmp/rg-abstraction-v1.jyupVg/A22-RESULT.md`.

### A23 staleness precision and the existing morphism layer

Across all 39 selected transitions, snapshot binding would reopen the C-02
decision 39 times, full-file binding eight times, and membership/function-
lineage binding twice. The last two are exactly the natural membership growths.
Six full-file reopenings occurred without a named-function change in the frozen
Git `-L` histories. A one-function lineage mutation moved the precise count
from two to three, showing sensitivity.

This corrects the A22 baseline: safe snapshot binding alone is far too coarse
for useful decision reuse. ReviewGraphen's ordinary Rust artifacts do not solve
that—their IDs are snapshot/key-derived and function content hashes are whole-
file hashes, as confirmed in the A8 ProgramSpace. However M6 already contains
accepted Rust signature/body anchors and cross-snapshot artifact mapping by
path/label/language and equal anchors.

The current structural-sloppiness v1 analyzer does not consume M6 or represent
group transitions. [R] The next feasibility check should reuse M6 on A20's two
transitions, not invent HigherGraphen identity. Its preparation cost must be
counted against the plain table. Record:
`/tmp/rg-abstraction-v1.jyupVg/A23-RESULT.md`.

### A24 M6 reuse feasibility

M6's inner mapper has the needed Rust signature/body anchors and staged
cross-snapshot correspondence, but it is not exposed as a lightweight mapping
surface. `IncrementalSourceClosureV5` has no public constructor. The sole
exported end-to-end proposal function requires source EventLogV4, authority
replay basis, completed M5 gluing, target EventLogV5 and both Store index hashes.
Neither the main CLI nor structural-sloppiness v1 exposes this workflow.

Building complete accepted histories solely for the A20 five-member comparison
would make integration setup dominate the plain baseline, so no M6 run was
performed. This is a bounded feasibility failure, not an M6 correctness or
performance result. A lightweight non-authority API would be new product work
and is not justified until a natural workflow demonstrates a failure of the
plain lineage/table approach. Record:
`/tmp/rg-abstraction-v1.jyupVg/A24-M6-FEASIBILITY.md`.

### A25 shared-policy mutation matrix

The same `..` component-rejection fault was injected independently into all
five C-02 validators in the pinned scratch clone. Baseline package suites were
green: core 605 tests, ingest 138 with its admitted Cargo executable, and
benchmark 19. Every mutant compiled. Sources were restored between mutations
and all five final hashes match the pre-mutation list.

Program and profile killed the fault with two tests each. The M6, ingest and
benchmark mutations survived their complete package suites: 605, 138 and 19
tests respectively remained green. This verifies a narrow test-effectiveness
gap at three sites for one explicit shared-policy regression; it is not a live
defect, reachability result or general mutation score.

This is the first actionable verified result in the abstraction line, but its
discovery is not attributable to ReviewGraphen. A manually curated family and
plain five-row matrix produced it. [R] The viable obligation is one policy group
with a member × conformance-case denominator, preserving caught, survived,
not-executed and intentional exception outcomes. A shared conformance corpus is
safer than automatically consolidating production functions. Record:
`/tmp/rg-abstraction-v1.jyupVg/A25-RESULT.md`.

## Protocol F1 and test-author phase

`/tmp/reviewgraphen-utility-orch.vRtLuM/FREEZE.md` pins the pilot protocol. Parent
matched the actual hashes of the contract (`133fd766…`), candidate inventory
(`9c8d60cb…`), overriding amendment A1 (`a4c5d26b…`), and original task
(`8ef80a20…`). A1 separates functional defects, structural findings, justified
separation, tentative opportunities, rejected claims, and unknowns. Structural
findings do not require an already-divergent bug: they require shared policy,
concrete multi-site change burden, and evidence for a safe shared boundary.

The already-examined hash group is now explicitly an unscored calibration trap;
the measured exploratory denominator is five remaining groups, not six. Synthetic
controls precede that measurement. Both arms can request bounded type/contract/caller
expansion. No measured arm result exists yet.

The combined test-and-implementation draft brief was corrected before delivery.
`rg-terra-harness` at `w3:p1T` is the test author only, with Terra xhigh and an
isolated cwd. It must stop after the missing-implementation RED result; a fresh
session will implement against pinned tests, fixtures, and instruction files.
The task orchestrator owns that worker and its checkpoint. The finished Sol
design worker has been closed, according to the orchestrator's report.

Test author result is now at
`/tmp/reviewgraphen-utility-orch.vRtLuM/harness/TESTS-RESULT.md`: it reports
`python3 -m unittest discover -s harness/tests`, exit 1, 27 failures caused by
missing production scripts. Parent inspected the retained final log and the
updated paired instruction texts; it did not rerun that command. This is
acceptance-test preparation, not a measured sloppiness-detection result.

### A28 responsibility-family maintenance experiment

The second non-equivalent common-policy mutation (`.` component rejection)
reproduced A25 exactly before intervention: program and profile caught it,
while M6, ingest and benchmark allowed it through their complete package suites.
The fixed two-mutation by five-endpoint denominator was therefore 4 caught and
6 survived.

The lowest-risk alternative was then executed: keep all production validators
separate and add one versioned eight-case conformance fixture consumed by one
crate-local test at each endpoint. The post-change baseline passed 608 core,
139 ingest and 20 benchmark tests. Repeating the exact ten mutations killed all
10; surviving cells fell from 6 to 0. No production predicate, public API,
typed error, purpose-specific constraint or runtime path was changed.

This is evidence for shared contract tests, not for automatic abstraction. A
common validator was stopped before implementation because the fixture achieved
the measured maintenance objective without adding public/dependency and error-
mapping surface. Performance and the full workspace suite remain unmeasured.

A scratch `reviewgraphen.responsibility_family.v1` record holds the snapshot,
members, common contract, intentional differences, denominator, results,
unknowns and reinspection triggers. Its checker detected both a fixture edit
and a member-source edit as stale and returned verified after restoration. This
demonstrates the desired state shape only: the current CLI cannot ingest the
record and did not discover the family. Record:
`/tmp/rg-abstraction-v1.jyupVg/A28-RESULT.md`.

### A29 selective reinspection from family state

A deterministic scratch planner compared the A28 family bindings with current
bytes. It emitted zero obligations when unchanged, five when the shared fixture
changed, and only `m6-mapping-path` when that member source changed. The latter
obligation bound expected and observed member and fixture hashes.

With the M6 dot-component rejection disabled, the selected exact conformance
test failed in a local 4.12-second observation and filtered 462 other core
library tests. After restoration, the same test passed once in 5.06 seconds,
the plan became empty, and the family state validated again. Compared with the
767 tests in A28's complete-family baseline, this shows selective immediate
reinspection for one exercised member fault. It does not constitute package-
level regression sign-off.

The result still required no graph engine: plain Python represented the routing.
Whole-file member hashes also over-invalidate unrelated same-file changes.
ReviewGraphen would add utility only by providing more precise accepted member
lineage or by materially reducing manual state handling—not by merely encoding
the same table. Record: `/tmp/rg-abstraction-v1.jyupVg/A29-RESULT.md`.

### A30 function-anchor invalidation precision

A scratch Rust extractor used locked `syn` to locate the five recorded private
functions and hash each function token stream. It is a versioned non-authority
observation, not proof of semantic equivalence.

An unrelated comment in the M6 source file caused the whole-file plan to reopen
M6 but left the function-anchor plan current. Disabling the M6 dot-component
predicate caused both plans to reopen only M6, and its exact conformance test
failed. After restoration, the five-anchor artifact was byte-identical to the
baseline (`9d65f5f4...`) and both plans were empty.

For these two controlled edits, function anchoring removed one false-positive
reinspection without missing the one relevant change. The observation does not
cover docs-only contracts, macros, moves, splits, merges, duplicate symbols or
cross-language members. Those are the cases where a morphism/gluing layer might
add value beyond a deterministic table. Record:
`/tmp/rg-abstraction-v1.jyupVg/A30-RESULT.md`.
