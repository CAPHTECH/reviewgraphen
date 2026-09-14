# FSL held-out responsibility-candidate measurement

Status: diagnostic candidate-yield measurement, not an efficacy or acceptance
claim. Date: 2026-09-14.

## Bound input

- Repository: `git@github.com:ymm-oss/fsl.git`
- Snapshot: `38f97bfdaf5a7d251de62dd43e37ab4b41e4ef73`
- Profile: production functions and methods under `rust/*/src/**/*.rs`
- ProgramSpace SHA-256: `a1dd4ab3ce67afa073e97a29b97bad1f688c7f2f84b3008d516ea51e293f1fd7`
- Exact report SHA-256: `f09fa7c6b8793141fa587031a1f05a6284ac075fe70b5adc4e30e27f4f115f1d`
- Near report SHA-256: `d2720e479cb80692c25919db9e4957b251d9e4cb84f0ac1d35ddacffe0c928b9`

Two independent snapshot/exact/near runs were byte-identical. This establishes
one deterministic replay at this snapshot only.

## Denominators and yield

| Measure | Exact v2 | Near v1 |
| --- | ---: | ---: |
| Accepted Rust symbols | 6,453 | 6,453 |
| Eligible production functions/methods | 2,917 | 2,917 |
| Excluded tests | 284 | 284 |
| Unknown test scope | 0 | 0 |
| Missing shape facts | n/a | 0 |
| Candidate groups | 65 | 116 |
| Candidate members | 177 | 360 |
| Exact-only shape groups excluded | n/a | 37 |

## Bounded source-reading check

Only the first ten ranked groups in each report were read. The exact sample
contained plausible repeated responsibilities including four parser annotation
helpers, duplicated `violation_bindings_json`, `has_bounds`, sanitizer helpers,
and `unknown_span`. The near sample recovered useful non-identical candidates,
notably `error_at`, strict-warning wrappers, `invariant_names_selected`, and
`normalized_kernel_ast`/`normalized_ast` (whose source explicitly requires the
normalization to mirror).

The near sample also exposed expected false-positive pressure: `eval` versus
`expr_json`, and `run_verify` versus `run_testgen`, share token shape but do not
thereby share responsibility. [R] This supports using near shape only to form a
decision obligation, never to accept a family. It would be falsified by a
contract/change-reason assessment showing that those pairs do share one
maintenance responsibility.

Not checked: the remaining 55 exact and 106 near groups, runtime reachability,
cross-language code, macro-expanded bodies, or whether any candidate should be
implemented as a shared validator rather than a shared conformance test or an
intentional separation. These remain `[U]`.
