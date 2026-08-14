# Phase B adversarial verification

This phase uses fresh black-box mutations; passing pre-existing tests is not counted as evidence. The exact runner is `scripts/run_adversarial.py` and the canonical observation is `adversarial-results.json`.

## Measured outcome

Eleven attacks ran against the fixed CLI slice and its serialized V5 consumer boundary. Seven broke the scoped property; four did not break it.

| Property / attack | Verdict | Observation |
|---|---|---|
| Determinism: run fixed review twice | not broken | Both exits 0; byte equality true; both SHA-256 `1a54536ceed71b27f3a5b13227762328538733079530e86bc0f527be8078c635` |
| Closed review surface: unknown fixture | not broken | exit 2 |
| Schema conformance: unknown top-level field | not broken | schema exit 3; gate exit 12 |
| Non-authority: AI self-labels accepted | not broken | schema exit 3; gate exit 12 |
| Authority binding: mutate decision actor/authority, retain hashes | **broken** | schema exit 0; gate accepted report as valid blocked report (exit 10) |
| Evidence integrity: mutate evidence subject, retain hashes | **broken** | schema exit 0; gate exit 10 |
| Denominator omission with locally repaired counts | **broken** | schema exit 0; gate exit 10 |
| Denominator inflation with locally repaired counts | **broken** | schema exit 0; gate exit 10 |
| Denominator substitution across local arrays | **broken** | schema exit 0; gate exit 10 |
| Freshness forgery plus coherent pass fields | **broken** | schema exit 0; gate exit 0 (`pass`) |
| Coherent local gate rewrite retaining old ID/body hash | **broken** | schema exit 0; gate exit 0 (`pass`) |

“Not broken” means only that the named attack failed. It is not a safety claim.

## Boundary interpretation

Typed Core/Store construction enforces exact authority grants, evidence binding, freshness, denominator construction, and terminal report authority. The serialized `schema validate`/`gate` commands do not replay those source records and do not recompute all stable IDs/body hashes. Therefore the broad claim “an arbitrary report file is authenticated by gate” is false: gate is a schema-plus-report-local policy evaluator, not a journal proof verifier.

## Minimal reproduction

Run from the repository root after building the CLI:

```sh
python3 verification/full-reviewgraphen-v1/scripts/run_adversarial.py \
  --cli target/debug/reviewgraphen \
  --output /tmp/adversarial-results.json
```

For the shortest gate forgery, generate the fixed report, change only `gate.status` to `pass`, set `blocking_obligation_ids`, `incomplete_obligation_ids`, and `reasons` to empty arrays, retain the old gate `id` and `body_hash`, then run `reviewgraphen gate forged.json`. The measured exit is 0. The runner constructs this mutation exactly and records stdout/stderr/exit codes.

The CLI exposes no injection seam into private Store authority typestate, so this experiment did not independently attack every internal constructor. It tested the serialized report consumer boundary and separately exercised the supported production route for byte determinism.
