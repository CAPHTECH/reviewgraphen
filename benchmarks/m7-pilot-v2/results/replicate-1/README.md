# M7 detection pilot v2 — replicate 1 audit bundle

This bundle records one paired, blinded B1 versus G3 proxy run over the new `m7-pilot-v2` corpus. No v1 public/private material, candidates, or scores were used as inputs. Raw candidate bytes and prepared manifest bytes were copied without repair.

## Protocol result

- Prepared trials: 24 (12 opaque units × 2 arms)
- Collections: 23 structured, 0 abstained, 0 parse failures, 1 protocol-invalid, 0 missing, 0 binding-invalid
- The protocol-invalid trial was `unit:p7m1` / G3 proxy: its returned packet IDs differed from the manifest. It was not repaired, retried, or scored.
- Eligible paired denominator: 11 pairs. The excluded pair is an empty-root matched-control pair, so all six seeded-positive pairs remain eligible.

## Measured detection result

On the six eligible seeded roots, B1 detected 3/6 (0.50 recall) and G3 proxy detected 6/6 (1.00 recall), a measured difference of +3 roots in this replicate.

Within the 11 eligible pairs, B1 emitted 8 findings (3 deterministic root matches, 5 unmatched) and G3 proxy emitted 9 findings (6 deterministic root matches, 3 unmatched). On the five eligible empty-root controls, both arms emitted 3 findings across 3/5 units. Therefore this run shows a seeded-root recall difference, but no reduction in control findings.

Across every individually protocol-valid trial, including the B1 half of the excluded control pair, B1 emitted 9 findings (3 matched, 6 unmatched); G3 proxy emitted 9 (6 matched, 3 unmatched).

## Adjudication and limits

Deterministic scoring requires source-location overlap and at least one closed-ontology mechanism match. Nine unmatched/ambiguous findings were exported through the fixed arm-blind API and independently adjudicated. `adjudication/public/items.json` and `decisions.json` contain only opaque item IDs and public finding judgments; trial/arm reconciliation remains private.

The six B1 unmatched items were classified as 3 valid novel defects, 1 false positive, and 2 duplicates. The three G3 proxy unmatched items were classified as 1 valid novel defect, 1 false positive, and 1 insufficient. Duplicates were not linked or promoted through additional non-blind judgment.

Using every protocol-valid emitted candidate (9 per arm), the auxiliary candidate-level counts are: B1 6 confirmed valid (3 deterministic matches + 3 valid novel), 1 confirmed false, 2 duplicates, 0 insufficient; G3 proxy 7 confirmed valid (6 deterministic matches + 1 valid novel), 1 confirmed false, 0 duplicates, 1 insufficient. The conservative confirmed-valid precision lower bounds are therefore 6/9 (0.667) for B1 and 7/9 (0.778) for G3 proxy. If every unresolved duplicate/insufficient item were valid, the corresponding upper bounds are 8/9 (0.889) and 8/9 (0.889). These are descriptive auxiliary candidate metrics, not injected-root recall or a general precision estimate.

This is one replicate over six synthetic seeded positives and matched controls. G3 proxy is a bounded non-authority projection, not the complete ReviewGraphen system. One control pair was excluded by the predeclared protocol. Model revision and some inference telemetry are unavailable as declared in `execution-config.json`. These results support only the measured pilot observation; they do not establish general code-review superiority or statistical significance.

## Contents

- `raw-candidates/`: exact model outputs
- `agent-inputs/`: exact model-visible packets for each trial
- `manifests/`: exact prepared trial manifests
- `collections/`, `collections.json`: protocol collection records
- `scores/`, `scores.json`: deterministic scores for protocol-valid trials only
- `inventory.json`, `summary.json`, `metrics.json`: denominators and aggregate results
- `execution-config.json`, `execution-record.json`: execution declaration and canonical binding
- `adjudication/public/`: arm-blind items, contexts, and exact expert decisions
- `adjudication/private/` and `private/oracles/`: restricted reconciliation and v2 oracle copies
- `validation/`: per-artifact validation outputs
- `audit-index.json`: byte hashes for every other bundle file
