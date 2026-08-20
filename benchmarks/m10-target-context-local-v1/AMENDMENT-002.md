# AMENDMENT-002 — shell quoting is immaterial to projection-use detection

Recorded after `projection-2` completed and the series stopped at its
projection-use gate.

## Observed defect

`projection-2` invoked the frozen projection successfully with the shell token
`'visit_block'`. The detector introduced by AMENDMENT-001 recognized only the
equivalent unquoted token `visit_block`, so it recorded `used: false` despite
the immutable stream containing successful quoted invocations.

## Correction

`check_projection_use.py` now treats the unquoted, single-quoted, and
double-quoted spellings of each preregistered selector as equivalent. It still
does not count `--help`, a missing selector, a different selector, or prose
that merely mentions the command.

The existing `projection-2` stream is not changed or regenerated. Its
projection-use record is recomputed deterministically from that stream, the
trial is archived, and `run_series.sh` is resumed. Completed trials remain
skipped by their existing verification records. No model output, hidden
acceptance result, or implementation verdict is changed by this correction.
