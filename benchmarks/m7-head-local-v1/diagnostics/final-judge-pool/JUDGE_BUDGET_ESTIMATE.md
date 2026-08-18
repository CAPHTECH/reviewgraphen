# Final judge pass — budget estimate, recorded before any real call

Status: recorded before issuing any of the 7 final judge calls, per
operator instruction to estimate sufficiency against the $30 cap first.

## Remaining balance (conservative floor, not exact)

`INTERIM_JUDGE_REPORT.md`'s Spend section: interim spend was tracked
conservatively as at most 3 x $3 = $9 of the $30 cap (the enforced
ceiling, actual very likely lower — none of the 3 interim calls
approached their ceiling). **At least $21 of $30 remains.** No exact
billed figure is available (`--output-format text` does not surface a
cost field, and the temp credential dir is deleted immediately after
each call); this floor is a lower bound, not a precise remaining
balance.

## Per-call ceilings for the 7 final calls

Sized larger for bigger pools (more findings to judge, more source bytes
to read), not flat, and deliberately **not** set to consume the entire
$21 floor in the worst case:

| unit | pool size | source files | ceiling |
| --- | --- | --- | --- |
| head-local-00 | 13 | 4 | $4.00 |
| head-local-01 | 10 | 2 | $3.50 |
| head-local-02 | 10 | 6 | $3.50 |
| head-local-03 | 1 | 2 | $1.50 |
| head-local-04 | 2 | 3 | $1.50 |
| head-local-05 | 1 | 3 | $1.50 |
| head-local-06 | 3 | 5 | $2.00 |

**Worst-case total exposure: $17.50** — under the conservative $21 floor
with **$3.50 of margin even in the worst case**, where every single call
simultaneously hits its individual ceiling (not expected; the interim
calls came in well under their own ceilings).

For calibration: the interim pass's largest pool (`head-local-00`, 13
findings after this experiment's later 3-way split — actually the
interim's `head-local-00` pool was smaller, 4 qwen-only findings; the
$3 ceiling used there was for a substantially smaller pool than this
pass's 13-finding `head-local-00`) — $4 for this pass's largest, denser
pool is a reasoned increase, not an arbitrary one.

## Decision

Sufficient to proceed without a separate stop-and-report cycle, per the
operator's own framing ("足りないなら実行前に報告する"). Reported here
regardless, since exact remaining balance is not known precisely and
this is disclosed as an estimate against a conservative floor, not a
guarantee.
