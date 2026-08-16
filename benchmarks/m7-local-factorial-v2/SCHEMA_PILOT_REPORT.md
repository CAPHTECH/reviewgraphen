# M7 local-factorial v2 schema pilot report

Correction added after raw capture: the causal classification
`context budget exhausted before final` below is superseded. The equality
212,520 + 49,624 = 262,144 remains a frozen usage observation, but the old
collector mislabeled non-reasoning provider usage as final content. A
diagnostic rerun captured only reasoning-summary deltas through sequence
12,900 and no output-text event before an intentional diagnostic stop. The
prior `thinking_tokens=0` and `final_content_tokens=49,624` fields are under a
known measurement defect and must not be interpreted literally. See
`THINKING_MEASUREMENT_CORRECTION.md`.

The production experiment did not start. Stage 1 produced two valid trials
out of four assigned trials, below the preregistered gate. The final trial
also returned upstream HTTP 500, which independently required an immediate
stop without retry.

| Snapshot | Arm | Reconciled outcome | Input tokens | Output tokens | Final bytes | Seconds |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 06 | B1 | valid | 16,283 | 38,880 | 3,151 | 980.899 |
| 06 | full | valid | 39,370 | 6,606 | 1,030 | 331.331 |
| 34 | B1 | context budget exhausted before final | 212,520 | 49,624 | 0 | 4,243.771 |
| 34 | full | upstream HTTP 500 | unknown | unknown | unknown | 690.177 |

The two nonempty final responses both conformed to the candidate schema, so
conditional schema conformance was 2/2. That is not the preregistered
estimand: operational validity was 2/4, and production required 4/4 (or an
expanded probe only after exactly 3/4). No target-detection result is reported.

## Context-capacity result

The initial 2.4 bytes/token extrapolation for snapshot-34 B1 was not borne out.
The provider accepted 212,520 input tokens, not roughly 360,000. Content and
arm changed the observed admitted-byte/token ratio from 2.397 to 4.084, so a
single byte ratio cannot identify exact over-limit units.

The earlier report treated exact total-context use as causal:

```text
212,520 input + 49,624 output = 262,144 model context tokens
```

The request completed with HTTP 200 but Codex materialized zero final bytes.
No evidence of implicit input truncation was observed. The equality alone does
not distinguish reasoning from final content and therefore does not establish
the earlier causal classification. The raw-capture rerun establishes a
reasoning-only stream for the rerun; the corresponding full trial could not
complete the comparison because the server returned HTTP 500.

The deterministic capacity audit uses all three completed requests to form an
empirical bytes/token interval and applies it to all 20 frozen positive units.
For B1, 7 units have possible input-limit risk; 1 has definite and 11 have
possible input-plus-output budget risk. For full, none are flagged by this
interval. These are risk labels, not exact token counts. Exact classification
would require the provider tokenizer or completed provider usage for every
request. The per-unit rows are in `capacity-audit.private.json`.

If a later, separately authorized experiment runs these units, execution
coverage and target detection must be reported separately. A context-exhausted
unit cannot be relabeled as ordinary `not_detected`; target detection can be
reported conditionally on mutually executable pairs, while the operational
20-unit table retains the unexecutable count as a distinct scaffold-capacity
outcome.

## Server and thinking interpretation

The earlier attribution of the frozen v1 74-minute empty response solely to an
MLX runner hang is superseded by the raw-capture correction. The independent
pre-resume chat still terminated normally with nonempty `thinking` and final
`OK`. The v2 snapshot-34 B1 historical response reached the exact context
window, but its unretained stream cannot now be split between reasoning and
final content. The diagnostic rerun exhibited reasoning-only generation.

ReviewGraphen did not disable thinking: the profile recorded reasoning effort
`high`, and no temperature or top-p override was supplied. Nevertheless the
proxy reported `reasoning_tokens=0` for every completed pilot request. Raw
event types now show that field is not a trustworthy thinking counter. The
historical zero is retained as provider-emitted usage only.

The snapshot-34 full request ended after 690.177 seconds with HTTP 500 and the
provider diagnostic `high demand`. The runner stopped immediately and made no
automatic retry. Elapsed time alone was never used as a failure condition.

## Statistical boundary

Production was not run, so the local B1/full target-detection comparison and
the model-by-scaffold interaction remain unmeasured. Even a completed 20-unit
run would be descriptive: FSL has 28 presence-eligible units versus the
conservative required sample of 83. No significance, equivalence, or
interaction claim is made.
