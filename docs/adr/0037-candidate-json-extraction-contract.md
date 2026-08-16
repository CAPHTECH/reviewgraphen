# ADR 0037: Reviewer candidate JSON extraction contract

- Status: Accepted for `m7-local-factorial-v3` preregistration
- Date: 2026-08-16

## Context

The frozen v2 local schema pilot recorded two nonempty responses containing a
candidate JSON object wrapped in prose or a fenced code block. The v2 parser
rejected the whole response before candidate-schema validation. This is an
output-packaging observation, not evidence that the model could not construct
the candidate object.

The v2 artifacts and scores remain frozen. This ADR defines a new, additive v3
consumer contract; it does not reinterpret v2 outcomes.

## Decision

For v3 local rows only, the adapter consumer extracts one candidate JSON object
before applying the unchanged v2 candidate schema validator:

1. Scan fenced code blocks in document order. A block may have an optional
   `json` language marker. The first block whose first JSON value is a JSON
   object is selected.
2. If no fenced object exists, scan the response from left to right at every
   `{` and use the first `json.JSONDecoder.raw_decode` result whose value is a
   JSON object.
3. If no object is found, extraction fails. If the selected object is followed
   by more prose, that prose is ignored; no second candidate is considered.
4. The selected bytes are passed unchanged to the existing strict candidate
   schema validator. No field is filled, repaired, normalized, merged, or
   otherwise interpreted by extraction.

The rule is deterministic and leaves no post-hoc candidate choice. Extraction
is a location/framing operation only; schema acceptance remains unchanged.

## Boundary and asymmetry

The local v3 B1/full rows use this extraction contract. Frozen v1/v2 frontier
rows used the whole-response, bare-JSON contract and are not rerun. Thus local
versus frontier comparisons contain a packaging-contract confound. The primary
local B1-versus-full comparison is not affected because both local arms use the
same extractor, schema, model, and reasoning setting.

The v3 extractor does not grant tools, change bwrap mounts, alter authority, or
make model prose authoritative. Raw response bytes remain recorded and
non-authority. Candidate schema, oracle, denominator, and scoring rules remain
the v2 rules.

## Consequences

The v3 pilot can distinguish output framing failure from candidate-schema
failure while retaining strict typed validation. A response containing two
plausible objects is resolved by the fixed first-object rule; later objects are
not inspected or selected. Any extraction or schema failure remains
protocol-invalid under intention-to-measure.
