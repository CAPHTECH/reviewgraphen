# m20 reviewer model pin rationale

Decision date: 2026-08-24. This is the single prose record for the reviewer
model choice. The five normative m20 documents state the pin and refer here;
they do not duplicate this evidence narrative.

## Decision

The only permitted reviewer configuration is:

- model: `Qwen3.8-27B-MLX-4bit`;
- effective reasoning effort: the mlx-dspark server default, currently `low`;
- request rule: omit the `reasoning_effort` field rather than sending `low` or
  any other override;
- requested `max_output_tokens`: 12,000;
- hard timeout: 900 seconds;
- admitted-source ceiling: 65,536 bytes.

The 8-bit models, every `ornith-*` model, and `xhigh` effort are not m20
reviewer alternatives. This is a feasibility, reliability, and authorized-time
pin, not a claim that 4bit/low is intrinsically more capable.

The 12,000 output setting was confirmed only after the 65,536-byte boundary
probe described below. It is a common requested backend cap, not a locally
verified token budget and not evidence that the arms realize equal tokens.

## Prior benchmark observations

The closest same-effort quantization comparison is the m18 8bit/low resident
diagnostic. It completed in 1,380.020 seconds with three findings, versus the
m17 4bit/low reference at 772.629 seconds with three findings: an observed
elapsed ratio of 1.786 (+78.6%). The blind judge marked 0/3 8bit/low findings
and 1/3 4bit/low findings `issue_should_be_created`. These are recorded in the
[m18 timeout comparison](../m18-8bit-low-checkpoint-review-v1/diagnostics/resident-timeout600-r1/comparison.json)
and [m18 accuracy comparison](../m18-8bit-low-checkpoint-review-v1/diagnostics/resident-8bit-medium-final1200-r1/accuracy-comparison.json).
At a 360-second selector limit, 8bit/low was incomplete and produced zero
findings, as recorded in the
[m18 360-second comparison](../m18-8bit-low-checkpoint-review-v1/diagnostics/resident-timeout360-r1/comparison.json).

The 8bit/medium record must not be conflated with that 8bit/low 3-finding/zero-
positive row. Under the fixed 600-second policy, 8bit/medium produced no valid
review or judgeable finding, hence zero judge-positive outputs, and had already
used 919 more estimated thinking tokens than the completed 8bit/low run; see the
[m18 medium comparison](../m18-8bit-low-checkpoint-review-v1/diagnostics/resident-8bit-medium-timeout600-r1/comparison.json).
A later, separate extended final attempt yielded one finding and one blind-
judge positive; its counterfactual single-run duration was 1,417.814 seconds
and actual diagnostic compute, including the failed attempt, was 2,017.815
seconds. It is retained as contrary evidence, not erased by the fixed-policy
failure.

The broader history is also contradictory. In
[m16](../m16-qwen-chained-reviewgraphen-v1/RESULT.md), 8bit/high completed in
782 seconds with three findings and a 1 create / 1 reject / 1 unable judgment,
while 4bit/low timed out at 901 seconds; m16 described 8bit/high as preserving
tool scope better. In [m17](../m17-casegraphen-controlled-review-v1/RESULT.md),
the direction reversed: 8bit/high timed out at select-3, while 4bit/low
completed and produced one judge-positive finding. Both experiments changed
quantization and effort together and used one replicate per cell. They
therefore cannot attribute either observed difference to quantization, effort,
their interaction, or run variance. The m20 pin does not claim otherwise.

## Pre-freeze development probes

On 2026-08-24, an operator ran non-holdout probes against this repository.
For the same clean-coverage packet, 4bit/low used 2,823 output tokens and
118.74 seconds; 4bit/xhigh used 24,128 output tokens and 917.21 seconds. The
observed ratios were 8.55x tokens and 7.72x time. Both returned the same
abstention, but low named `lines ~150–232` and
`DomainError::HistoricalPrefixMismatch`, whereas xhigh omitted both line and
symbol specificity. Extrapolating this single xhigh latency to 40 pairs x two
arms is about 20.4 model-hours, nearly the authorized 21-hour ceiling. The
temporary response artifacts were `r2-clean-coverage-qwen.resp.json`
(`sha256:0af5017d7959c52b2737d6dc625b381aff0997e309f65ca815e6ae4a3a6e3f50`)
and `dis.resp.json`
(`sha256:afe7c57ff56b3bdf7fb5f5014a5d4a8256eae3047882d669d1dcbc19a2afa456`).

Across the four 4bit/low 16 KB probes, valid JSON was 4/4 and one response
contained a source-grounded claim identifying the real missing closing
backtick at `error.rs:47`. Across six ornith probes, valid JSON was 3/6
(one timeout and two output-budget terminations), with zero claims and four
abstentions. Ornith nevertheless had material strengths: every completed
response obeyed the schema, independently checked citations were accurate,
and it fabricated no IDs. These strengths are recorded alongside, not hidden
by, its lower completion and claim yield.

The subsequent boundary probe packed exactly 65,536 admitted-source bytes.
With 4bit/low, the response stopped normally with valid JSON and an
`issue_present` disposition after 2,599 of the requested 12,000 completion
tokens (21.7%) and 153.76 of 900 seconds (17.1%). The output cap therefore had
about 4.6x headroom at the preregistered source boundary. The response artifact
was `ceil.resp.json`
(`sha256:387df7ff767381e06ad0a8ad2300634d88410262032d0ed7e351256dab084daf`).

The boundary response identified a second real defect: public
`Ratio::percentage` in `coverage.rs` stores `numerator / denominator`, a
fraction in `[0,1]`, despite the percentage name; `projection.rs:485` consumes
that field. The operator checked the cited code and confirmed the unit/name
ambiguity. Together with the `error.rs:47` closing-backtick defect, 4bit/low
produced two code-confirmed development-probe claims. Neither xhigh nor ornith
produced a claim in these probes.

An earlier concern that 12,000 might be insufficient came from the 24,128-token
xhigh observation. Applying that xhigh behavior to 4bit/low was an unsupported
generalization. The exact 65,536-byte 4bit/low probe falsified that concern for
the observed boundary packet. The earlier 18 KB run used 6,083 tokens, so
quadrupling admitted source did not increase output; it reduced output to
2,599 while changing the disposition from abstention to a claim. More context
plausibly resolved the question sooner, but this one comparison does not prove
that causal explanation or guarantee all future packets will use fewer tokens.

The development response files live in the orchestration scratchpad rather
than the m20 bundle. Their names and hashes are provenance aids, not frozen
study evidence. They were observed during development on this repository,
were not holdout trials, and are n=1-style model-characteristic probes. They
cannot estimate population performance or establish m20 utility. They justify
only the pre-freeze pin choice. Once the protocol and evaluator are frozen,
the pin cannot be changed in response to outcomes.
