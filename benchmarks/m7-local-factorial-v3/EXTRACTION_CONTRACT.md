# v3 candidate extraction contract

This document is frozen before v3 model calls and implements ADR 0028.

Extraction chooses location only. It never repairs or normalizes a candidate.
The selected object is passed to the unchanged
`reviewgraphen.benchmark.candidate_output.v1` validator.

Selection order is fixed:

1. fenced blocks, in opening order; first block containing a well-formed JSON
   object;
2. otherwise, the first well-formed JSON object found by left-to-right scan at
   `{` positions.

If none exists, the trial is protocol-invalid. If multiple objects exist, the
first selected object wins and later objects are ignored.
