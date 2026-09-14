# FSL planned-responsibility search check

Status: bounded real-repository wiring check; not held-out efficacy evidence.

On 2026-09-14, `search-responsibility` consumed a ProgramSpace for FSL commit
`38f97bfdaf5a7d251de62dd43e37ab4b41e4ef73` and a three-clause planned contract
for loading approval records. The query declared callable terms `approvals` and
`load`, signature terms `Option`, `PathBuf` and `Vec`, and operation terms
`join`, `read`, `read_versioned_record`, `sha256_bytes` and `sort`.

Two fresh output files were byte-identical:

```text
sha256:ff5a3cc393b9ec8290acda72f92bac8010069cf03e72beb6b1c6f47cdd9185e4
```

The closed report schema validated. From 6,453 accepted Rust symbols, the
declared production/test-scope boundary admitted and evaluated 2,917 functions
or methods. It emitted four candidates and therefore 12 unverified clause
obligations:

1. `load_approvals_for_check`: all declared query signals matched.
2. `run_approval_create`: three of five operation terms matched.
3. `load_approvals`: all declared query signals matched.
4. `load_evidence`: four of five operation terms matched.

Candidate order is the deterministic candidate-ID order, not a semantic
relevance score. Direct source reading confirmed that the first two are the
intended existing implementations: both read and decode versioned approval
records, enforce a `requirements_document` target, sort record digests and hash
the joined values. Their purpose differs: `load_approvals` verifies signed
records for generation, while `load_approvals_for_check` deliberately does not
re-verify signatures during structural reproduction. `load_evidence` is an
adjacent loader with similar I/O and digest operations but a different record
contract. `run_approval_create` is an incidental broad-operation match.

All three natural-language clauses remain present in every candidate's
`unverified_clause_ids`. This run therefore shows deterministic candidate
retrieval and an explicit verification denominator. It does not prove that a
candidate satisfies the planned contract, should be reused, or belongs to an
accepted responsibility family. The same FSL repository informed earlier
signal calibration, so recall, precision and implementation-time savings are
not established.
