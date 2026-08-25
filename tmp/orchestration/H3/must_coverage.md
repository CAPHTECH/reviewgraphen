# H3 MUST/MUST NOT kill map

Basis: `EVALUATOR_SPEC.md` sections 1--14. The 16 uppercase normative sentences split into 18 independently killable contracts. `Automated` means a production mutation or hostile boundary operation has a named oracle. `External gate` means the fact is outside the evaluator tree and cannot honestly be established by evaluator code. External gates are retained below rather than laundered into a self-attestation.

## Explicit uppercase contracts

| ID | Section | Contract | If killed, this must fail | Coverage |
| --- | --- | --- | --- | --- |
| U01 | preamble | implementation remains below `evaluator/` | orchestration changed-path scope gate | External gate |
| U02 | preamble | implementation uses Python 3 | G6 `runtime_compatible`, `test_runtime_contract_and_portable_bundle` | Automated |
| U03 | preamble | no `crates/` or ADR 0038 edit | orchestration changed-path scope gate | External gate |
| U04 | preamble | evaluator author differs from Candidate D implementer | preregistration identity review | External gate |
| U05 | 1.3 | canonical/source/excerpt/text algorithms carried forward unchanged | 52 vectors; bundle identity; H3N01 normalization mutant | Automated |
| U06 | 3.2 | planned keys and required IDs are unique | N16 and N17 | Automated |
| U07 | 10 | hostile serialized vectors do not restore production DTO inputs | P01--P05, P08, N01/N03/N04/N07/N11/N12 and CLI surface test | Automated |
| U08 | 10 | generator input has exactly 52 versioned vectors | `test_all_52`; CLI requires total 52 | Automated |
| U09 | 11.1 | production never imports/calls hostile-artifact read path | H3A01 and `test_production_sink_has_no_read_and_pipeline_has_no_verifier` | Automated |
| U10 | 11.2 | generated fixtures include the eight semantic runs and all vector expansions | H3F01; byte-exact `generate-fixtures --check`; freeze acceptance gate | Automated |
| U11 | 11.2 | attack success is not inferred from counts/copied summaries | M01--M10, N24/N25, H3R01/H3D01/H3N01 execute mutated copies; hostile rows execute boundary code | Automated |
| U12 | 11.4 | every REQUIRED production mutation makes its oracle fail | `run-attacks`, including the three H3 re-mutations | Automated |
| U13 | 12 | runtime/host/design/provenance values do not enter portable bundle preimage | M10 and `test_runtime_contract_and_portable_bundle` | Automated |
| U14 | 12 | semantic runtime requirement change creates a new versioned study | protocol version/freeze review; evaluator proves only execution-hash change | External gate |
| U15 | 12 | freeze hashes are written to preregistration only after checks | protocol freeze writer/reviewer outside permitted edit scope | External gate |
| U16 | 12 | recorded evaluator/slice identities differ | preregistration identity review | External gate |
| U17 | 13 | invariant exit 4 is never converted to arm zero | H3C01 invokes public CLI error dispatch | Automated |
| U18 | 14 | other m20 documents do not restate evaluator algorithms | cross-document design review outside permitted edit scope | External gate |

Totals: 18 explicit contracts; 12 automated; 6 honest external gates; 0 evaluator-internal contract without an oracle. The six external gates require repository/protocol review and were not replaced with evaluator self-claims.

## Security, authority, closure, and determinism defenses

| Defense | Kill/hostile operation | Falling oracle |
| --- | --- | --- |
| Git object preimage hash | replace digest comparison with `if False` | H3R01 plus `test_object_hash_mismatch` |
| Git batch framing | missing header newline/invalid length framing | H3R02 plus `test_object_framing_oid_and_type_are_distinct` |
| Git OID syntax | non-lower-hex OID | H3R02 plus repository unit test |
| Git expected type | valid blob requested as tree | H3R02 plus repository unit test |
| Commit shape | commit without leading tree | H3R02 plus `test_commit_invalid` |
| Tree edge framing | truncated binary OID | H3R02 plus `test_malformed_tree_edge` |
| Tree mode | unsupported `100600` | H3R02 plus `test_invalid_tree_mode` |
| Duplicate tree path | repeated local path edge | H3R02 plus `test_duplicate_tree_path` |
| Model byte/depth bounds | exact/+1 inputs; replace `_depth` with no-op | H3D01/H3D02 plus model-boundary tests |
| Model collection bounds | claims 3/4, observations 8/9, loss IDs 3/4 | H3D02 |
| Model scalar bounds | strings 512/513 and 1024/1025; integer 2/3 and I-JSON max/+1 | H3D02, P06, N22 |
| Model encoding/code points | invalid UTF-8, surrogate, NUL, duplicate key | H3D02, N21 |
| Text normalization/threshold | remove casefold; exact/+1 byte thresholds | H3N01 and M06 |
| Source/payload closure | orphan/foreign payload and tampered audit artifacts | M07, P01--P03, N08--N12 |
| Paired authority and scoring | opportunity, permutation, judge identity/hash/type, conjunction mutants | M02--M05, P06/P07, N13--N15, I04--I07 |
| Freeze determinism/authority | runtime in portable preimage, symlink, shadow instruction, changed production oracle | M10, N24/N25, I09/I10 |

Priority audit result: uncovered security = 0, authority = 0, closure = 0, determinism = 0.
