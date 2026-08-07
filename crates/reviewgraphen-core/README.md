# reviewgraphen-core

`reviewgraphen-core` is the deterministic M0/M1 core. It owns typed,
language-neutral ProgramSpace facts, ReviewSpace obligations and claims, and
EvidenceSpace records without collapsing their authority boundaries.

The crate deliberately supports only the checked-in manual JSON ProgramSpace
fixture and the five-rule M1 synthesis slice. It does not ingest Git or Rust
syntax, execute reviewers or verifiers, construct context envelopes, glue local
reviews, propagate staleness, use SQLite, or perform network or shell I/O.

All externally supplied ProgramSpace JSON is deserialized through validating
constructors. `MvpRulePack::synthesize` retains an internal aggregate but
serializes `ObligationBundle` as the checked-in obligation JSON-schema contract.
`ReviewAggregate` is built by validated event envelopes; an accepted finding
must cite the same supporting evidence, fresh passed verification, and explicit
human decision. A high confidence AI claim remains proposed.

The fixture's input profile (`code-review` plus version `1`) is normalized by a
small compatibility adapter to the bundle spelling `code-review@1`. This is a
representation difference in the checked-in contracts, not a semantic rewrite.

## M1 trust admission

Evidence events carry the snapshot on which they were observed. A validated
`ProgramSpace` first exposes an opaque `EvidenceSnapshotAdmission`; it alone
mints a non-serializable, exact `EvidenceAdmission` bound to the event's run,
snapshot, evidence ID, and canonical evidence body. Replay needs that same
host-supplied admission again. Verification freshness is derived from the exact
evidence records and stored canonically; callers cannot label a verification
fresh. Historical evidence remains auditable but cannot support an accepted
current-snapshot sign-off.

`TrustedHumanAdmission` is the narrow M1 boundary for a host that has already
authenticated a human. The host mints a non-serializable `DecisionAdmission`
for one exact decision body and run. `EventCommand` is likewise local-only;
there is no public deserializable event payload. Persisted envelopes replay only
when their evidence/decision records have exact run-bound admissions, their
payload hash, and their canonical pristine initial-aggregate (genesis) hash all
match. This is not an
identity provider or signature scheme: real credentials and authenticated
durable storage remain outside M1.

`Projection` is an internal, loss-declaring audience view rather than a report
contract. `ReviewReport::from_aggregate` is the deterministic adapter for the
checked-in `reviewgraphen.review.report.v1` schema. M1 retains only a claim's
`execution_id`, not reviewer or context-envelope metadata. The report therefore
emits a resolvable but explicitly `abstained` execution placeholder with
`unknown`/`unresolved` fields, an abstention reason, a projection-loss
limitation, and an obstruction blocking the affected claims and obligations.
It does not invent an execution engine, reviewer identity, or context fact;
gluing remains empty and report status stays `partial`.
