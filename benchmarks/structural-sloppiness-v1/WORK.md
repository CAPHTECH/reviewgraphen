# Structural-sloppiness trial: implementation state

Date: 2026-09-13. Tier C: a new experimental analysis contract, report/schema,
CLI, and empirical trial cross several interfaces. Sol/high planned; a fresh
Sol/high session authors acceptance tests; Terra/xhigh implements; a fresh
Sol/high session reviews. One writer at a time. No external publication.

## 1. Goal

Try ReviewGraphen for structural-sloppiness detection using a runnable,
source-backed producer/consumer wiring check, a real historical regression,
its repair, and a calibrated relation-erasure experiment. Success means this
bounded trial is implemented, tested and actually run; useful results and
negative results must both remain. It does not mean proving general utility.

## 2. Observed behavior

The current consumer asks whether a function has `changed:true` itself or an
incoming one-hop `contains` from an artifact with `changed:true`
(`crates/reviewgraphen-core/src/synthesize.rs:2971`). The producer emits
`changed_by(symbol, change)` and now the reciprocal `contains(change, symbol)`
(`crates/reviewgraphen-ingest/src/lib.rs:900`). Commit
`7b40d48fa49ed3a823609c98f9ac35405518059a` supplied the previously missing marker
and inverse relation. The parent is `271c93200ac96f8ca7335145e02105b1d2c1d86d`.
Old and current `core/src/program.rs` are identical, and the full v2/v3
ProgramSpace export can be decoded by the current core.

An actual old-ingest run over HigherGraphen
`5990679bda201e377d1275a3d2a59bf69f9debcf` ->
`852af1ed281871e3f27cf1aa997a4b7a450ddc79` completed with 11,075 artifacts and
18,309 relations. It has zero eligible public free-function changed_by edges:
retain as NOT EXERCISED, not clean. Global AST/containment are partial.
A second real diff was selected by inspecting added public declarations:
`ac45c9e8e1bf090fcfdcafc0926cfe41c1ab2412` ->
`a992243a18700d2ed32e6057ddf22090fb855970`. Its old export completed with 10,914
artifacts, 18,010 relations and THREE eligible public-function producer edges
(`run_fixpoint`, `attempt_gluing`, `attempt_structural_gluing`).

The root checkout has unrelated user modifications to runtime verification,
the skill, and orchestration log, plus untracked benchmark tests. Baseline
`scripts/ci.sh fast` passed bundle checks and failed rustfmt on that pre-existing
runtime change. Work therefore uses this clean isolated checkout at
`c0747614c2c20a51771f6f6495f196682e3912fd`; later integrate only this task's diff.
The missing pinned nextest was installed at `/tmp/reviewgraphen-slop-tools/bin`.

## 3. Explicit requirement

> sol/terraを利用しつつReviewGraphenで雑さ検出転用を試してみて

## 4. Recovered constraints

- Keep program facts, candidate claims, evidence and unknowns separate;
  no accepted/verified/human-accepted promotion (`AGENTS.md:14`).
- Bind snapshot, profile, rule/extractor version, source IDs and declared
  information loss (`AGENTS.md:19`).
- Deterministic outputs; unknown source regions remain unknown
  (`AGENTS.md:27`, `crates/reviewgraphen-ingest/src/lib.rs:1`).
- ADR precedes implementation; additive versioned report, typed errors and
  source/boundary tests (`AGENTS.md:54`).
- Benchmarks are non-authoritative (`crates/reviewgraphen-benchmark/src/lib.rs:1`).
- Cargo admission stays disabled; input repository code is never executed
  by the analyzer (`crates/reviewgraphen-ingest/src/lib.rs:120`).
- Partial extraction does NOT suppress an observation of the actual consumer
  predicate on the exact supplied graph. It DOES prohibit a completeness or
  source-level absence claim. Preserve limitation IDs and capability states.
- A zero eligible denominator means `not_exercised`, never successful detection.

## 5. Affected contracts

Additive experimental API, fixed named wiring contract, closed report/schema,
standalone benchmark CLI and trial manifest. Existing product schemas, ingest
facts, D/Node obligation identities, store/events, CLI and review state unchanged.

## 6. Scope and protected paths

New benchmark module `src/structural_sloppiness.rs` (or module directory),
dedicated binary, new tests UNDER src/structural_sloppiness/tests.rs, ADR 0041,
new experimental schema/example and schema inventory registration, and this
experiment directory. Only a module declaration in benchmark lib.rs is needed.
No new dependency unless justified by an actual missing requirement.

Protect core/, ingest/, runtime/, reviewer/, store/, product CLI, existing
schemas/fixtures, all previous benchmarks, benchmark/tests/, skill and root
orchestration log. The tiny exporter under the old /tmp checkout is a harness,
not a producer modification. Do not commit, push or publish.

## 7. Compatibility and selected property

Fixed `changed_input_consumer_bridge_mismatch@1` contract:
eligible accepted `changed_by(S,C)` has producer provenance
`reviewgraphen.ingest.git.changed_structure.v1`, S is an accepted public
`kind=function`, C is a change artifact with that producer provenance and
appropriate change_kind/path attributes. Evaluate EXACTLY:
`S.changed == true OR exists contains(X,S) with X.changed == true`.
X is not required to equal C; do not invent transitive closure.
If false, produce a candidate about this consumer's inability to observe S in
this ProgramSpace. Do not assert a source bug or that an obligation should fire
(other applicability conditions are outside this check).

Keep separate observed input/gate records and candidate claims pointing to
those records. Bind full canonical ProgramSpace hash and contract hash, versions,
source IDs/locations, eligible/excluded counts with reasons, capability states,
limitation references and loss. Validation against the supplied ProgramSpace
must recompute the analysis and reject stale or tampered output. No authority.

The flat projection retains artifact records and capabilities, relation
kind/provenance counts but erases endpoints/edge IDs. It is an explicit
information-loss ablation, NOT a competitive static-analysis baseline. Rewiring
a bridge endpoint while preserving this inventory must change the graph result
and leave flat bytes unchanged. A flat missing-marker warning on the original
old graph is allowed; do not claim graph-exclusive discovery from that case.

## 8. Verification and experiment

Acceptance tests written separately before implementation; verify expected RED,
pin their hashes, then Terra edits production only. Cover exact OR branches,
marker-only and bridge-only failures, wrong/reversed endpoints, nonpublic/method
exclusions, unrelated provenance, not_exercised, partial extraction, deterministic
ordering, stale/tampered input binding, schema, no authority and flat ablation.
Add at least one deliberate detector fault that the tests reject.

Focused: `cargo test -p reviewgraphen-benchmark structural_sloppiness`.
Impacted: benchmark crate tests and clippy all targets `-D warnings`.
New Rust files must be explicitly rustfmt checked (the repo gate selects tracked
files only). Schema example must validate. Required full gate:
`PATH=/tmp/reviewgraphen-slop-tools/bin:$PATH scripts/ci.sh fast` in clean clone.
Concept ledger classifies the global gate as declaration-only; the task-specific
acceptance suite will be independently calibrated. Verification is not solely
visual. Missing xseries/token/cost measurements remain blank.

Real experiment: hold HigherGraphen diff fixed; run old 271c932, exact fixed
7b40d48 and current c074761 ingest with source-identical exporter, then analyze
their full ProgramSpace exports. Require old eligible count >0, old mismatch,
fixed/current clearing for the same eligible targets. Keep the first nonexercising
pair. Record exact revisions, input/report hashes, commands/exit codes, scope and
limitations. Retain small reports and reproducible harness, not tens of MB of
duplicated ProgramSpace data. Fixture witness is labeled synthetic; old/fixed
integration run is historical calibration, not held-out new-bug discovery.

## Checkpoint

Parent accepted Sol's revised producer/consumer plan. No unknown user choice or
external write is required. Relevant source witnesses checked locally; the
consumer's exact one-hop OR is an additional protected semantic boundary.
Real positive denominator confirmed (3) before implementation. All cited local
path:line witnesses passed the required existence check (exit 0); parent opened
the producer and consumer implementations. Acceptance-test authoring is approved;
production implementation waits for parent acceptance of the calibrated RED suite.
Sol's initial signature-drift alternative is not selected.
Clean-clone gate needs explicit host admission:
`REVIEWGRAPHEN_TRUSTED_CARGO=/home/rizumita/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin/cargo`;
this path was resolved by the trusted root harness (exit 0). No mise trust change.

## 9. Acceptance-test handoff (Sol/high, 2026-09-13)

The fixed callable/DTO contract is in `CONTRACT.md`. A compile-only production
stub exposes that API and returns typed `NotImplemented`; it contains no detector
logic. Fifteen tests use the real `ProgramSpace::from_json_slice` validation
boundary and cover the exact OR, any marked incoming container, strict one-hop
bound, the real producer's added-path shape, missing marker/bridge,
wrong/reversed endpoints, unmarked ordinary
container, eligibility/provenance exclusions, zero denominator, partial
extraction, ordering, full-input/tamper validation, closed schema/no authority,
and flat endpoint erasure. The fixture is synthetic calibration input.

Focused calibration command:

```text
REVIEWGRAPHEN_TRUSTED_CARGO=/home/rizumita/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin/cargo cargo test -p reviewgraphen-benchmark structural_sloppiness
```

Observed exit: `101`. Compilation succeeded; the schema-document check passed,
and all 14 functional tests reached the callable stub and failed with
`NotImplemented`. This is the intended functional RED, not a link/compile
failure. No full gate was run. After Terra
reaches GREEN, parent will calibrate at least one deliberate detector mutation;
that mutation has not yet been executed here.

Pinned acceptance artifacts (SHA-256):

```text
f309ac1545278af73f4fc214592e9c8da3f79207e4958c3a5792b157ceddac06  crates/reviewgraphen-benchmark/src/structural_sloppiness/tests.rs
5e18de8dae55c269a58cfbe1a963aaac5b1829e739620cd8526a96f7421c3939  benchmarks/structural-sloppiness-v1/fixtures/missing-bridge-program-space.json
305820a934288d57a8c00e0ceb9597bc62693ba007bcfbf5aaff4504aa642027  benchmarks/structural-sloppiness-v1/schema/structural-sloppiness-report-v1.schema.json
e634baafbdab6395d53b9cac0a03d40927633575621f8de78834fd3221d0bfbd  benchmarks/structural-sloppiness-v1/CONTRACT.md
```

## 10. Parent implementation checkpoint

The parent inspected the acceptance tests, fixed contract, actual producer and
consumer, and verified all four pinned hashes. The reported functional RED
exit 101 is accepted; no duplicate run is required. Production implementation
is authorized for fresh Terra/xhigh. The JSON Schema dev dependency already
exists at the workspace-pinned version and only adds one benchmark lock entry.

One pre-handoff requirement error was corrected: requiring both change paths
to be nonempty would exclude every real positive (all three are added files).
`crates/reviewgraphen-ingest/src/git.rs:1252` actually emits empty base/nonempty target for additions. The
corrected fixture and dedicated test now cover this boundary. A literal false
constant was also removed from a test description; it was not fault injection.
The flat ablation retains opaque capability evidence references and therefore
does not claim to erase every occurrence of a relation ID in the input.

Real captures now all completed with exit 0. Independent jq evaluation of the
exact predicate observed old positive 3 false / 0 true, exact fixed 0 false /
3 true, and current 0 false / 3 true. These are pre-analyzer checks only; the
implemented analyzer must still process and validate the same full inputs.

## Parent empirical verification

- Mutated the actual detector OR to AND using apply_patch. Exact-branches test
  compiled and failed at its own-marker assertion, exit 101. Restored OR, rebuilt
  the CLI, and ran all 15 focused tests: exit 0. Four pinned hashes unchanged.
- Five actual CLI analyses and recomputation validations completed: first diff
  0 eligible/not_exercised; old positive 3/3 candidates; fixed 3/0; current 3/0;
  synthetic endpoint rewire 3/1. Same eligible subject IDs across old/fixed/current.
- Flat current and valid-rewired bytes are identical, SHA-256
  `e684b5c1044557ca65d39ce6394aeac630dc8cb8741947c47448ed96ae3cdffd`.
- First mutation attempt changed target_ids but omitted ordered_target_ids.
  Real ProgramSpace admission rejected it with exit 3. It was not counted as
  a detection case. Corrected both endpoint representations and reran the control.
- Authority accepted=true tamper rejected by CLI recomputation, exit 4. A current
  report applied to the rewired input was rejected, exit 4. Copied change attrs
  on a function-kind artifact yielded change_shape_invalid and zero eligibility.
- New replay.sh ran against all four captures and completed with exit 0,
  including the valid synthetic control, flat comparison and expected stale
  refusal. The checked-in results/summary.json is its exact compact output, not
  a full AnalysisReport; omissions and source-report hashes are explicit.
- Source-identical exporter is retained under harness/; its SHA matches the one
  actually used at old, fixed and current producer revisions.

Full mandatory gate attempt with nextest default concurrency ran 1,136 tests:
1,134 passed, 2 timed out at the existing 120-second bound, 2 were skipped by
existing configuration. Exit 100. The two timeouts are the existing
`v3_positive_d_pair_is_projected_from_basis_bound_run` and
`v3_positive_pair_reaches_context_construction` real-tree integration tests.
No tests/configuration/timeouts were changed. Parent is testing the resource
contention hypothesis with these two tests sequentially at the same timeout;
full-workspace GREEN is NOT yet claimed. Focused/package/schema checks are green.

## 11. Terra implementation delta and handoff (2026-09-13)

Σ delta: the benchmark-only stub is now a deterministic v1 analyzer. It admits
only the fixed producer `changed_by` relation, a public free `function`, and a
producer-shaped `change`/`custom` artifact (including the exact Git path
shapes). It evaluates the source consumer's own-marker OR one incoming marked
`contains` predicate; observations, candidate claims, and extraction
obstructions remain separate. The report binds full canonical ProgramSpace
bytes and the exact bytes of `CONTRACT.md`; it remains non-authoritative.

The new `reviewgraphen-structural-sloppiness` binary accepts validated
ProgramSpace JSON only, writes create-new report/flat outputs, and has typed
nonzero malformed-input, stale/tampered-report, and existing-output failures.
Its flat ablation erases relation-record endpoints and IDs while retaining
artifacts, capabilities, limitations, and kind/provenance counts; opaque
capability/limitation references may still name relation IDs. The generated
synthetic example is schema-checked by `scripts/validate_bundle.py`.

Verification in this isolated checkout (all exit 0):

```text
rustfmt --edition 2024 --check crates/reviewgraphen-benchmark/src/structural_sloppiness.rs crates/reviewgraphen-benchmark/src/bin/reviewgraphen-structural-sloppiness.rs
cargo test --locked -p reviewgraphen-benchmark structural_sloppiness
cargo test --locked -p reviewgraphen-benchmark
cargo clippy --locked -p reviewgraphen-benchmark --all-targets -- -D warnings
python3 scripts/validate_bundle.py
```

CLI smoke results: `--help` exited 0; an existing output exited 5; malformed
ProgramSpace JSON exited 3. This worker did not rerun historical captures, the
full workspace gate, or detector-mutation calibration; the parent owns those
next steps. [U] Parent-reported captures have no public free function meeting
the local concurrency hints (`async`/`awaits`/`spawns` false and empty
`concurrency_primitives`), so the three historical observations are named
predicate mismatches, not evidence of three lost applicable obligations.

Writer ownership is released to the parent after the final pinned-hash and
diff checks; no concurrent source writer is authorized by this handoff.

## 12. Sol review and repair checkpoint

Sol reported one major and two minor findings. Parent reproduced the major:
valid colon-bearing relation/change IDs yield two gates but one unique report
observation ID. Root constraint: distinct structured identity tuples must remain
distinct when encoded; delimiters valid inside inputs are not framing.
Affected surfaces are observation IDs, derived candidate IDs, references,
generated example and replay reports. The parent-authored identity_tests.rs
fails at unique-count 1 versus 2 (exit 101); its formatted SHA-256 is
92cbb24917121a884e01342d829870db822abb3b2705c1d226e1644b1deea399.
This test and the original four pinned artifacts remain implementation-protected.
Checkpoint: repair only benchmark-local ID framing, regenerate example, preserve
report scope, schema, exact predicate and non-authority. No global identity or
product behavior expansion. Terra receives source writer ownership for this repair.

Minor findings retained: replay trusts positional producer-role labels rather
than checking capture-manifest hashes; change-shape/capability branches have
partial test coverage. Neither is evidence against the measured added-function
calibration. These limitations must remain explicit until separately addressed.

Full gate diagnosis: sequential execution still timed out for both named tests.
The report test also timed out at 120.101 seconds on pristine c074761 tracked
source (exit 100), confirming that timeout on the baseline. Runtime baseline
has not been separately checked. Next hypothesis is unoptimized test-build cost;
any optimized-profile check must preserve assertions and the 120-second limit.

Parent follow-through: Terra's framed-ID source was present and the parent ran
16 focused tests successfully. The worker was explicitly interrupted before
handoff; no unreceived worker verification is claimed, and parent resumed sole
writer ownership. Candidate hashes no longer preserve observation-ID ordering.
The existing contract requires each output array's own-ID order; a new 16-gate
ordering test failed with exit 101 before parent added candidate sorting.
Original protected artifacts, including identity_tests.rs, were not changed.

The pristine baseline report test passed in 61.203 seconds with
CARGO_PROFILE_TEST_OPT_LEVEL=1 and the unchanged 120-second timeout (exit 0).
The full fast gate is being run with that build setting and one test thread.

## 13. Final review and deserialization calibration

Sol's final bounded source review reported no unresolved blocker/major and
retained the two documented minor limits. It additionally requested a direct
deserialization regression test rather than relying on JSON Schema validation.
Parent inspected the current source: AnalysisReport already rejects unknown
fields. Removing that attribute in the isolated clone made the new boundary
test fail with exit 101; restoring it yielded 18/18 focused tests, exit 0, in
both clone and root. The rebuilt actual CLI rejected an extra top-level report
field with exit 3. No additional report authority or schema change was made.

The pre-ordering optimized fast gate completed exit 0 (1,137 selected tests;
existing configuration skipped two). A new full gate on the final 18-test source
is running; earlier gate success is not substituted for this final check.
The integrated root preserves all three pre-existing dirty-file hashes.
The central implementation-delegation ledger was appended with observed data;
unmeasured model tokens, time/cost and cross-family review remain blank.

## 14. Completion audit (2026-09-14)

- User requirement: Sol planned/authored acceptance tests, Terra implemented,
  and fresh Sol reviewed. The resulting benchmark CLI was actually run over
  four real captures plus the valid synthetic rewire, not merely proposed.
- Calibration: known old producer 3/3 mismatches, exact repair 3/0, current 3/0,
  endpoint rewire 3/1; initial zero-eligible case retained as not_exercised.
  Current/rewired flat bytes remain identical. Full report recomputation and
  stale refusal ran. This is one known wiring defect, not unknown-bug efficacy.
- Invariants: original test/fixture/schema/contract pins preserved; ID collision,
  ordering and closed-report deserialization each have observed failing
  calibration and passing restoration. Final focused suite: 18/18 in both trees.
  Actual extra-field CLI rejection: exit 3. No report gains authority.
- Final Sol review: no unresolved blocker/major; deserialization test minor
  closed after reading the calibrated test. Two disclosed minor limits remain
  (manual capture-role/hash comparison and partial deterministic branch tests).
- Final-source fast gate: exit 0, 1,139/1,139 selected tests passed, two existing
  configuration skips, 71 doc tests passed. Nextest elapsed 317.675 seconds.
  The report/runtime historical integration tests passed in 59.500/58.868 seconds.
  No tests/assertions/timeouts were weakened. Exact invocation below.
- Integration: all 22 task files matched between isolated and root checkouts
  before this documentation closure. Root focused tests, bundle validation,
  explicit new-source rustfmt, replay shell syntax and diff checks passed.
  Three pre-existing dirty-file SHA-256 values remain unchanged. Whole dirty
  root formatting is not claimed green; its pre-existing failure is in section 2.
- Local fast-gate log: /tmp/reviewgraphen-slop-final-fast.log, SHA-256
  9dfdebf936808565df12645abdf09fe126810ce39daa62c7f52173c37f75a2df.
  No commit, push, publication, target-code execution, or human acceptance.

```text
PATH=/tmp/reviewgraphen-slop-tools/bin:$PATH \
REVIEWGRAPHEN_TRUSTED_CARGO=/home/rizumita/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin/cargo \
CARGO_BUILD_JOBS=4 CARGO_PROFILE_TEST_OPT_LEVEL=1 \
CARGO_TARGET_DIR=/tmp/reviewgraphen-slop-old.hZypZx/target \
NEXTEST_TEST_THREADS=1 scripts/ci.sh fast
```
