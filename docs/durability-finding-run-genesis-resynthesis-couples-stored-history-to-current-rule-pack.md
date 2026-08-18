# Durability finding: a stored run genesis becomes unloadable whenever the rule pack changes

- Status: diagnosis and options; option C accepted by the operator and being implemented in stages. Corrections are marked inline and dated.
- Date found: 2026-08-18
- Scope: `RunGenesisSnapshot::rebuild_aggregate`
  (`crates/reviewgraphen-core/src/event.rs`), every store and core path that
  decodes a genesis through it, and `UniverseDescriptor::validate_against`
  (`crates/reviewgraphen-core/src/synthesize.rs`). This is a durability-model
  finding about ReviewGraphen the implementation, not about any one milestone
  or fixture.

## Finding

Every decode of a persisted run genesis re-runs `MvpRulePack::synthesize`
against the genesis's own `ProgramSpace` and requires the freshly synthesized
universe and obligations to equal the stored ones **byte for byte**. A genesis
written by any rule pack other than the one compiled into the running binary
therefore fails to load at all.

Not degraded. Not flagged stale. Not loadable-with-a-warning. The failure is a
`DomainError::Validation` raised inside `rebuild_aggregate`, which is reached
from `JournalIdentity::new` and every other genesis entry point, so it fires
before any caller can inspect, migrate, or even report on the record. The
stored run is inert.

For a system whose stated purpose is durable, auditable review provenance,
this makes durability conditional on the analyzer never changing.

Nothing caused this recently. It has always been the behaviour;
`node.changed_public_symbol@2` (branch `wip-rule-version-at2`, unmerged) is
simply the first change that ever tried to exercise it, and it surfaced as
`index::v6::tests::v6_terminal_legacy_body_hash_fixture_is_rejected_before_target_only_recovery`
failing inside `JournalIdentity::new` on the checked-in
`crates/reviewgraphen-store/tests/fixtures/terminal-v5-gluing-genesis.json`,
whose obligations were synthesized under `node.changed_public_symbol@1`.

## Root cause, precisely

1. **The re-synthesis equality is unconditional.**
   `rebuild_aggregate` (`event.rs`, the block ending
   `"run genesis obligations and universe must equal deterministic MVP
   re-synthesis"`) calls `MvpRulePack::synthesize(&self.program_space)` and
   compares the result to the stored `universe` and `obligations`. There is no
   version branch, no compatibility path, and no migration hook anywhere in
   that function. The only field it consults before comparing is `schema`,
   which distinguishes the genesis *wire format*, not the rule pack.

2. **Both decode entry points route through it.** `from_canonical_bytes` (v2
   wire) and `from_canonical_bytes_v3` (the v3/v4 wire inherited by v5) each
   call `rebuild_aggregate` before returning. `from_canonical_bytes_for_index`
   and `from_canonical_v4_bytes_for_store` delegate to those two. There is no
   raw-deserialize path that skips it: `RunGenesisSnapshot`'s fields are
   private and every constructor validates.

3. **Every consumer inherits it.** `rebuild_aggregate` is called from
   `reviewgraphen-store` (`journal.rs` ×5, `index.rs` ×3, `test_support.rs`
   ×5) and from several places in core's own `event.rs`. All of them fail the
   same way on a genesis from a different pack.

4. **A second, independent coupling existed in the universe check.**
   `UniverseDescriptor::validate_against` (`synthesize.rs`) rejected on
   `self.rule_pack_version != "m1.fixture@1"` — a hardcoded comparison against
   today's constant, not against anything recorded. A genesis that truthfully
   records a *different* pack version was rejected by this check even before
   the re-synthesis comparison was reached. Worse for option C specifically:
   `validate_against` runs inside `ReviewAggregate::new`, which the decode path
   calls *after* computing the verdict, so the verdict would have been computed
   and then discarded by a hard error two lines later — option C's read path was
   unreachable until this was removed. **Fixed in `ae805f4`**, replaced by the
   version-agnostic invariant that the field must be present.

5. **A third coupling narrows it further.** `MvpRulePack::synthesize` returns
   an error unless `program.profile_key()` is `code-review@1` or the fixed M5
   double-submit fixture profile. A stored genesis under any third profile
   cannot be validated at all, for the same structural reason: the loader
   assumes exactly one rule pack and exactly two profiles.

6. **The only migration API in the crate does not cover this.**
   `migrate_program_space_v1_to_v2` migrates a `ProgramSpace` wire version. No
   equivalent exists for a genesis, a universe, or an obligation set.

## What the re-synthesis check actually defends — measured, not inferred

This check is not redundant, and any fix must keep what it provides.

I probed it directly by mutating a freshly built, otherwise-valid genesis and
observing which validation rejected each mutation. Every mutation below
preserves obligation `StableId`s, strict ordering, uniqueness, canonical
encoding, and per-record `validate_full()`:

| Mutation | Rejected by |
| --- | --- |
| `obligations[0].weight` 3.0 → 99.0 | re-synthesis only |
| `obligations[0].applicability_status` `unknown` → `applicable` (with reasons and qualification IDs cleared to stay self-consistent) | re-synthesis only |
| `obligations[0].required_capabilities` narrowed to `["ast"]` | re-synthesis only |
| `obligations[0].accepted_evidence_modes` narrowed to `["test"]` | re-synthesis only |
| `universe.rule_pack_version` → `m1.fixture@2` | re-synthesis only |

Nothing else in the decode path catches any of them. The other genesis
defences — canonical-bytes equality, unknown-field rejection, strict
`StableId` ordering, duplicate rejection, malformed-ID rejection — all pass
these mutations, because the mutations are well-formed.

So the guarantee is: **a genesis's obligations and universe are exactly what
the rule pack produces from that ProgramSpace, and not what the writer chose
to put there.** Without it, anyone who can write genesis bytes can lower a
critical obligation's weight (which drives risk-first planning order and the
coverage denominator's weighting), mark an obligation inapplicable, relax the
evidence modes it will accept, or shrink its required capabilities — all
while keeping the same obligation IDs, so nothing downstream that keys on IDs
notices. That is a direct attack on ADR 0003's premise that obligations define
the coverage universe.

Note what the check is *not*: it is not an integrity check on the bytes. CAS
addressing and the canonical-bytes equality already cover corruption and
transport tampering. This check specifically covers **authorship** — that the
content was produced by the rule pack rather than asserted by the writer.

## Is the intent documented?

**No.** ADR 0014 §"`RunGenesisSnapshot`" specifies the DTO and states that
"Core alone decodes it with unknown fields denied, reconstructs it through
`ReviewAggregate::new`, requires pristine state, and checks snapshot/profile
values against the manifest." It does not mention re-synthesis-and-compare.
The only in-code statement of intent is the doc comment on
`from_canonical_bytes`, which says the decode "rejects unknown fields,
noncanonical encodings, malformed ProgramSpace records, and any inconsistent
universe/obligation tuple" — "inconsistent" being the only word covering this
behaviour, and it does not say inconsistent *with what*.

ADR 0014 does, however, state the opposing concern explicitly: "V1 replay
alone retains the old `sha256(canonical_json(initial ReviewAggregate))`
formula, so **historical hashes remain valid**." Preserving the readability of
already-written history is a value the ADR names; this check works against it.

`grep` over `docs/` finds no occurrence of `rule_pack_version` at all, and no
ADR discusses rule-pack evolution against stored runs.

## Is `rule_pack_version` the intended discriminator?

It is the only field shaped like one, and it is **written truthfully but is
insufficient as maintained**. Three separate observations:

1. **It is written from one hardcoded literal.** `synthesize.rs` sets
   `rule_pack_version: "m1.fixture@1"` in `universe()`. There is exactly one
   rule pack, so this is currently truthful rather than fabricated — but it is
   a constant, not something derived from the pack's contents.

2. **It is persisted and projected, but never consulted as a discriminator.**
   It is stored in the derived-index `universe` table in all three index
   generations (`index.rs`, `index_v4.rs`, `index_v6.rs`) and carried into the
   coverage projection (`coverage.rs`). The only place anything *reads* it to
   make a decision was `UniverseDescriptor::validate_against`, and that
   compared it to the hardcoded current value — the coupling in root cause
   (4), not a discriminator. That comparison has since been removed
   (`ae805f4`); see the correction below.

   **Correction (2026-08-18).** An earlier revision of this document said the
   field "is bound into the universe's own `StableId`". It is not. The
   `rule_pack` binding in `universe()` (`synthesize.rs`) binds the *running*
   pack's literal, not `self.rule_pack_version`, so a recorded universe's own
   identity is blind to that field being altered — a genesis whose
   `rule_pack_version` is tampered keeps its recorded universe ID unchanged.
   Re-synthesis catches it only because it compares the whole
   `UniverseDescriptor` by value, which makes re-synthesis *more* load-bearing
   than this document originally credited, not less: it is the only check in
   the system that sees that field at all. Pinned by
   `universe_identity_check_admits_a_foreign_rule_pack_and_leaves_authorship_to_re_synthesis`
   in `crates/reviewgraphen-core/tests/m1.rs`.

3. **Decisively: it does not track rule versions.** `node.changed_public_symbol@1`
   → `@2` changes individual obligation IDs and therefore the universe ID
   (measured: `universe:sha256:6b554b4a…` → `universe:sha256:e7a9c6ca…`), but
   leaves `rule_pack_version` at `m1.fixture@1`, because the literal is
   unrelated to the rules in the pack. A fix that branched on "same recorded
   `rule_pack_version` ⇒ safe to re-synthesize and compare" would take the
   *same* path that fails today and produce the *same* failure.

So `rule_pack_version` is a plausible-looking field that would need to become
a genuinely maintained version — bumped whenever any rule in the pack changes
its identity or trigger — before it could serve as the discriminator. That is
a new standing maintenance obligation, not a free win, and nothing today
enforces the bump.

## The authorship-in-validation pattern, and why it is now closed

Four instances of one mistake were found, across two functions, all reached
from `ReviewAggregate::new`. Each asserted something about *who wrote a
record* inside a check whose job is whether the record is internally
well-formed, and each therefore discarded a verdict that had already been
computed correctly:

| # | Where | The assertion | Now |
| --- | --- | --- | --- |
| 1 | `UniverseDescriptor::validate_against` | `rule_pack_version != "m1.fixture@1"` | replaced by "the field must be present" (`ae805f4`) |
| 2 | `MvpRulePack::synthesize`'s profile gate, propagated by `?` | the pack declines the profile ⇒ decode error | reported as `GenesisReproduction::NotSynthesizable` (`8906e65`) |
| 3 | `UniverseDescriptor::validate_against` | recomputes the universe `StableId` through `universe_id`, which binds the running pack's literal | unchanged, but no longer reached for a record this pack cannot synthesize |
| 4 | `ReviewAggregate::validate`, `subgraph` arm | `version().rule() == "capability_gap.origin_rule@1"` | replaced by the property ID, which is stable across a rule revision |

Only the first was found by looking. The second surfaced when the verdict was
made reachable, the third when the second was fixed, and the fourth only
because the third prompted a deliberate search for more. That is three of four
discovered by consequence, which is why the closing search was done by asking
what *shape* to look for rather than waiting for the next break.

**Closed** means: every remaining check in `ReviewAggregate::validate` and in
`UniverseDescriptor::validate_against` is either a structural property of the
record (a reference resolves, a namespace matches, an ID set agrees, a field
is present) or a coherence property tied to the target kind (a subgraph
obligation names the snapshot and carries no determinate applicability).
Nothing left in either function asks which rule pack, rule version, or profile
produced the record. Authorship is established in exactly one place —
`RunGenesisSnapshot::reproduction` — and reported as a verdict rather than
raised as a validation error.

Instance 4 was the most dangerous of the four and the only one that had not
yet bitten. Every obligation real ingestion produces today is a capability
gap, so versioning `capability_gap.origin_rule` would have made every stored
genesis fail to construct an aggregate simultaneously. It is pinned by
`a_capability_gap_from_a_later_rule_version_is_still_a_well_formed_subgraph_obligation`.

Instance 3 is worth stating precisely because it is the one that still bites,
by design: a universe minted by a genuinely different rule pack cannot be
rebuilt into an aggregate, because this pack's `universe_id` derivation cannot
reproduce its identity. That is why
`from_canonical_bytes_with_reproduction` returns such a record with its
verdict and *without* an aggregate, and says so at the call site.

## How the write boundary is actually held (option C, step 2)

The obvious implementation of "refuse at every extension point" is to
enumerate the extension points and add a guard to each. A sweep of
`reviewgraphen-store` found that would have been fragile:

- `JournalIdentity::new` is the **only** construction site for that type in the
  workspace; every other value is a clone. So whatever that constructor
  admits, every consumer inherits.
- Several read-named functions write. `EventJournal::recover_terminal_proof_v5`
  puts a terminal proof object into CAS. `replayed_v2_session`,
  `replayed_v3_session` and `replayed_v4_session` each take the *exclusive*
  journal lock and hand back a full `append_*` surface, and are called from
  read-only report and index code. `inspect_recovery_v4` mints a capability
  that authorizes destructive tail truncation.
- Several write-named functions do not write —
  `seal_static_verification_attempt_v3` is an in-memory core operation.
- The two functions every durable path funnels through, `validate_prefix` and
  `chain_genesis`, are shared with the read paths, so neither is a safe gate.

Gating by name or by enumeration would therefore have had to be right about a
list where the names actively mislead in both directions, and a single miss is
a silent hole.

So the boundary is held by construction instead. `JournalIdentity::new` stays
strict and refuses any V5 genesis the running rule pack does not reproduce;
`JournalIdentity::new_for_read` is the opt-in that reports the verdict rather
than refusing. Every extension point takes a `JournalIdentity`, and the only
way to get a non-reproduced one is to have asked for it explicitly. Missing a
consumer now fails closed — it keeps the old strict behaviour — rather than
opening a hole.

`new_for_read` returns a verdict only for the V5 wire. On v2/v3/v4 it is
exactly `new`, because those projections cannot yet carry a verdict to a
reader, and admitting a version-crossed run into a projection that renders it
as ordinary is the defect this change exists to avoid.

## A separate defect found while tracing the paths

`decode_index_v5_genesis` (`crates/reviewgraphen-store/src/journal.rs`) is the
only genesis decode in the workspace that does not go through one of core's
`RunGenesisSnapshot::from_canonical_bytes*` seams. It calls
`serde_json::from_slice` directly and then `rebuild_aggregate()`, so it never
checks `snapshot.canonical_bytes()? == bytes`.

`serde`'s `deny_unknown_fields` and `rebuild_aggregate`'s own structural and
authorship checks still apply, so a forged obligation body is still refused
there. What is missing is the canonical-encoding equality every other path
enforces: a noncanonical byte encoding of an otherwise valid genesis is
accepted on this path and rejected on all the others.

This is unrelated to the durability finding above and is not fixed as part of
it. It is recorded here and at the call site so the next person to touch that
function finds it deliberately rather than by accident.

## Options, and what each gives up

### A. Validate against the recorded version rather than the current one

Re-synthesize only when the genesis's recorded pack version equals the running
pack version; otherwise skip the comparison and accept the stored tuple after
the existing structural checks.

- **Gives up:** the authorship guarantee entirely, for any genesis claiming a
  different version. A hostile corpus writes `rule_pack_version:
  "m1.fixture@99"` and every obligation body becomes attacker-chosen —
  weights, applicability, evidence modes, capabilities — while still loading
  cleanly. This is strictly worse than the status quo unless the version field
  is itself authenticated.
- **Requires:** `rule_pack_version` to become genuinely maintained (see above),
  and some binding that prevents a writer from simply declaring an unknown
  version to opt out of validation.

### B. Store the synthesized result as the authority; never re-derive

Treat the genesis as the record of what was synthesized, and drop
re-synthesis. Authorship is then guaranteed at *write* time only.

- **Gives up:** the ability to detect a genesis that was never produced by any
  rule pack. Every mutation in the table above becomes accepted. Whoever can
  write bytes into CAS controls the review universe.
- **Mitigation that would be needed:** an authenticity binding over the
  genesis independent of its content hash — a signature or a MAC from the
  producing run — so authorship is proven cryptographically rather than by
  re-derivation. That is a trust-boundary change (ADR 0028 territory), not a
  local fix.

### C. Keep re-synthesis, but make mismatch a typed outcome instead of a load error

Decode succeeds and returns the stored tuple together with an explicit,
typed verdict: `Reproduced` (fresh synthesis matched) or
`NotReproducible { recorded_pack, running_pack }`. Callers decide. Read-only
consumers — audit, replay of history, index projection, the M6 staleness
comparison — accept `NotReproducible` and mark downstream results as
version-crossed. Anything that *extends* a run (appending events, accepting
new evidence, minting new obligations) refuses unless the verdict is
`Reproduced`.

- **Gives up:** nothing at the write/extend boundary — the authorship
  guarantee is preserved exactly where it protects the coverage universe.
- **Gives up, at the read boundary:** the assurance that a *displayed*
  historical universe was rule-pack-authored. A hostile corpus could be
  read and shown with forged weights, so any surface that renders a
  `NotReproducible` run must carry the verdict with it rather than presenting
  it as equivalent to a reproduced one. The verdict has to propagate into the
  index and the report, not stop at the decode call.
- **Note:** this is the only option that distinguishes the two things the
  current code conflates — "is this genesis well-formed and authored" and "was
  it authored by *this* binary".

### D. Version the genesis wire and migrate

Treat a pack change as a genesis schema change: bump `schema`, and write a
migration for each older version, mirroring `migrate_program_space_v1_to_v2`.

- **Gives up:** little in guarantee terms, but it is the most expensive
  option and scales badly — a migration per rule-pack revision, each of which
  must reconstruct what the old pack would have produced, which in practice
  means keeping every historical rule pack compiled in forever.
- **Worth noting** only because it is the pattern the codebase already uses
  for `ProgramSpace`, so it is the "consistent" answer even though the cost
  profile is different.

### E. Narrow the comparison to what the guarantee needs

Re-derive and compare only the fields an attacker could profit from and that
are stable across rule revisions, rather than the whole tuple. In practice
this is hard to make meaningful: the profitable fields (weight, applicability,
evidence modes, capabilities) are exactly the ones a rule revision legitimately
changes. Recorded here because it is the obvious first idea and it does not
survive contact with the measurement above.

## My reading

Option **C** is the only one that keeps the measured guarantee where it
matters while making stored history readable. It is also the smallest change
in guarantee terms, and it matches the shape the codebase already uses
elsewhere — ADR 0023's staleness model is built on typed verdicts about
historical records rather than on refusing to load them, and
`StaleReasonV5::RuleChanged` already exists to express "the rule that produced
this differs from the current one" for obligations. Extending that vocabulary
to the genesis itself would be consistent rather than novel.

Option A is a trap: it looks like the minimal fix and it silently removes the
guarantee for exactly the inputs an attacker controls.

This is a recommendation, not a decision. It changes production durability
semantics and belongs to the operator.

## What is established, and what is inference

**Established by direct measurement or by reading the code paths:**

- The re-synthesis comparison is unconditional; both decode entry points reach
  it; there is no version branch, compatibility path, or migration hook in it.
- The five mutations in the table are rejected only by that comparison, and
  nothing else in the decode path catches them.
- `validate_against` hardcodes `rule_pack_version != "m1.fixture@1"`.
- `MvpRulePack::synthesize` errors for any profile other than `code-review@1`
  and the M5 double-submit fixture profile.
- `rule_pack_version` is written from one literal, persisted into the universe
  ID, all three index generations, and the coverage projection, and is read as
  a decision input in exactly one place — the hardcoded comparison above.
- `rule_pack_version` does not change when a rule version changes; the
  `@1`→`@2` measurement confirms it stays `m1.fixture@1` while the universe ID
  moves.
- No ADR or doc states the re-synthesis intent; ADR 0014 describes the decode
  without it, and explicitly values keeping historical hashes valid.

**Inference, stated as such:**

- That the guarantee's *purpose* is authorship rather than integrity. The code
  carries no comment saying so; I am reading it off what the check uniquely
  catches versus what CAS addressing and canonical-bytes equality already
  cover.
- That the affected population is "every stored genesis". I verified the code
  paths are unconditional and reproduced the failure on one real stored
  genesis; I did not enumerate every genesis artifact in the repository and
  load each one.
- The severity ranking of the options, and the claim that Option C is
  smallest-in-guarantee. That is engineering judgement, not measurement.

## Not done, deliberately

`crates/reviewgraphen-store/tests/fixtures/terminal-v5-gluing-genesis.json`
was **not** regenerated. It is one leg of a frozen hostile-corpus chain
(`terminal-v5-gluing.jsonl`, `terminal-proof-v5-gluing.json`) whose genesis
hash is embedded in every chained event hash, and whose stated purpose is that
it "predates the strict terminal DTO and persists a non-contract `body_hash`".
Regenerating the genesis alone changes its hash, the chain stops matching, and
`publish_fixture_v5` still returns `Err` — but for the wrong reason, so
`v6_terminal_legacy_body_hash_fixture_is_rejected_before_target_only_recovery`
would pass vacuously and the vacuity would be invisible. Regenerating the whole
chain with current code would produce a well-formed corpus, which is the
opposite of what the test needs.

`node.changed_public_symbol@2` is complete and preserved on the unmerged
branch `wip-rule-version-at2`, red on exactly this one test. It should land
after this finding is settled, if it still should.
