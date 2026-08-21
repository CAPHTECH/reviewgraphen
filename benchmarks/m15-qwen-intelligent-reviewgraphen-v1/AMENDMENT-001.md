# Amendment 001 — analysis counter correction after 4bit-low-r1

This amendment was written after `4bit-low-r1` and before `8bit-high-r1`.

The analyzer labeled the count of all Bash calls as
`reviewgraphen_call_count`.  That presentation was wrong: the trial made 12
Bash calls, but only two matched the registered ReviewGraphen request forms.
The field now counts parsed ReviewGraphen requests and a separate
`bash_call_count` records all Bash calls.  The protocol and success result are
unchanged because off-protocol Bash was already checked independently and the
trial also timed out without a report.

No prompt, card, model, budget, execution order, or report validation rule was
changed.  The 4bit analysis is regenerated with the corrected counter before
the 8bit request.

