# ReviewGraphen capability matrix (Phase A)

Audit date: 2026-08-14.  This matrix describes executable capability, not intended architecture.

Classification means:

- **mechanically enforced** — a production constructor, typestate, parser, reducer, or CLI rejection boundary enforces the scoped claim;
- **test-backed only** — executable code and a contract test exist, but the supported CLI vertical slice does not expose an end-to-end production route;
- **documentation only** — the current workspace has no executable production route establishing the claim.

## Premise audit

| Premise | Finding | Code evidence |
|---|---|---|
| No HTTP client, LLM SDK, or async runtime dependency | Qualified correct. The stated list is exactly `workspace.dependencies`; crate-local dependencies additionally include `rusqlite` and optional `base64`. | `Cargo.toml:17`; `crates/reviewgraphen-store/Cargo.toml:18` |
| CLI review surface is one fixed fixture | Correct. Dispatch accepts only `review --fixture double-submit`; other command families are schema and gate. | `crates/reviewgraphen-cli/src/lib.rs:65` |
| The product cannot call a model | Corrected. A live path is currently absent, not technically impossible. The current reviewer used by the slice is fake/no-tools; local CLI processes are a dependency-free implementation route. | `crates/reviewgraphen-cli/src/lib.rs:344`; `crates/reviewgraphen-reviewer/src/lib.rs:862`; `benchmarks/m7-real-v1/scripts/run_isolated_trial.sh:1` |
| The fixed route traverses the claimed vertical slice | Correct within the fixed fixture: it constructs source/target runs, obligations, reviewer claims, evidence/verification, human authority/decision, terminal authority, and a V5 report. | `crates/reviewgraphen-cli/src/lib.rs:194`; `:209`; `:282`; `:331`; `:392`; `:468`; `:521`; `:563`; `:640`; `:669` |

The official OpenAI model page records a 2026-02-16 knowledge cutoff for `gpt-5.6-sol` and support for `high` reasoning. This is execution metadata for Phase D, not evidence about current product capability.

## Fixed vertical slice

1. Closed CLI dispatch (`crates/reviewgraphen-cli/src/lib.rs:65`).
2. Isolated temporary Store and fixed M4 prefix (`crates/reviewgraphen-cli/src/lib.rs:194`).
3. Source M5 gluing and authority inspection (`crates/reviewgraphen-cli/src/lib.rs:209`).
4. Source index rebuild and target V5 publication/index validation (`crates/reviewgraphen-cli/src/lib.rs:242`).
5. Incremental mapping, obligation selection, and partial reruns (`crates/reviewgraphen-cli/src/lib.rs:282`).
6. Fixed raw reviewer response and strict structured admission (`crates/reviewgraphen-cli/src/lib.rs:331`).
7. Target gluing and verifier-contract binding (`crates/reviewgraphen-cli/src/lib.rs:392`).
8. Native static/harness verification (`crates/reviewgraphen-cli/src/lib.rs:468`).
9. Explicit human grant and decision (`crates/reviewgraphen-cli/src/lib.rs:521`).
10. M5 completion and proof-bound terminal marker (`crates/reviewgraphen-cli/src/lib.rs:563`).
11. Dual-run terminal report authority and canonical V5 generation (`crates/reviewgraphen-cli/src/lib.rs:640`, `:675`).

## Capability classification

| ID | Scoped property | Classification | Primary implementation/test evidence |
|---|---|---|---|
| C01 | CLI review surface is closed to one fixture | mechanically enforced | `crates/reviewgraphen-cli/src/lib.rs:65`, `:1011` |
| C02 | Fixed route executes persisted M4/M5/M6 continuation | mechanically enforced | `crates/reviewgraphen-cli/src/lib.rs:194`, `:675`, `:1054` |
| C03 | Fixed input yields canonical byte-identical V5 output | mechanically enforced | `crates/reviewgraphen-core/src/canonical.rs:9`; `crates/reviewgraphen-cli/src/lib.rs:675` |
| C04 | Facts, claims, evidence, verification, decisions are separate types | mechanically enforced | `crates/reviewgraphen-core/src/program.rs:757`; `crates/reviewgraphen-core/src/review.rs:604`, `:934`, `:1142` |
| C05 | Typed generation derives coverage from an explicit denominator | mechanically enforced (typed generation only) | `crates/reviewgraphen-report/src/report_v5.rs:114`, `:1352`, `:4185` |
| C06 | Reviewer request is source-closed bytes without paths/tools | mechanically enforced | `crates/reviewgraphen-reviewer/src/lib.rs:328`, `:338`, `:2730` |
| C07 | AI execution claim is proposed/unreviewed | mechanically enforced | `crates/reviewgraphen-core/src/execution.rs:2696`; `schemas/reviewgraphen.report.v5.schema.json:2117` |
| C08 | Human decision admission requires an exact grant | mechanically enforced (Core/Store) | `crates/reviewgraphen-core/src/event.rs:12795`, `:15072`, `:61761` |
| C09 | Evidence/verification trace is claim-bound and freshness-aware | mechanically enforced (Core aggregate) | `crates/reviewgraphen-core/src/review.rs:3044`, `:3229`; `crates/reviewgraphen-core/src/m6_test_support.rs:611` |
| C10 | Local sections are glued before global projection | mechanically enforced (fixed slice) | `crates/reviewgraphen-cli/src/lib.rs:392`, `:563`, `:1077` |
| C11 | V5 generation requires non-serializable terminal authority | mechanically enforced | `crates/reviewgraphen-store/src/journal.rs:1222`; `crates/reviewgraphen-report/src/report_v5.rs:560`, `:4576` |
| C12 | V5 schema is closed to external fields/values | mechanically enforced | `crates/reviewgraphen-cli/src/lib.rs:109`; `schemas/reviewgraphen.report.v5.schema.json:1` |
| C13 | Gate rejects malformed/locally inconsistent reports | mechanically enforced (report-local only) | `crates/reviewgraphen-cli/src/lib.rs:144`; `crates/reviewgraphen-report/src/report_v5.rs:95` |
| C14 | Journal/index replay rejects body/chain/CAS/index tampering | mechanically enforced | `crates/reviewgraphen-store/src/journal.rs:2736`; `crates/reviewgraphen-store/src/index.rs:5495`, `:6385`, `:6931` |
| C15 | Rust/Git ingestion accepts an arbitrary admitted local snapshot | test-backed only | `crates/reviewgraphen-ingest/src/git.rs:1393`; `crates/reviewgraphen-ingest/tests/m2.rs:210` |
| C16 | Product can call a live LLM adapter | documentation only (before Phase C) | no implementation; architecture at `docs/05_system_architecture.md:294` |
| C17 | Live calls are isolated, recorded, and replayable | documentation only (before Phase C) | no implementation; architecture at `docs/05_system_architecture.md:400` |
| C18 | Generic Stage 0–10 product pipeline exists | documentation only | no generic CLI route; architecture at `docs/05_system_architecture.md:218` |
| C19 | Parallel independent obligations use stable logical commit order | documentation only | no scheduler; architecture at `docs/05_system_architecture.md:360` |
| C20 | Product path improves detection over free-form review | documentation only (before Phase D) | no product-path measurement; claim at `docs/00_vision_and_scope.md:191` |
| C21 | Rust implementation executes over HigherGraphen crates | documentation only | no Cargo dependency; conceptual map at `docs/04_highergraphen_mapping.md:1` |

The complete machine-readable fields, qualifications, and secondary documentation witnesses are in `capability-matrix.json`.
