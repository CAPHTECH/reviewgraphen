# Statistical character of the v3 gate and v4 decision

Status: frozen while the amended v3 stage1 first trial was still running and
before any amended-v3 stage1 result was known.

The v3 4/4 gate is a deterministic threshold applied to stochastic model
outputs. Provider-default sampling is retained, so repeated execution of the
same input can differ. The pre-timeout-amendment observations already showed
one valid result among two semantic completions. Treating 1/2 only as a crude
plug-in rate, not a precise estimate, gives a 4/4 pass probability of
`0.5^4 = 6.25%`. Even if per-trial validity were 0.7, the pass probability
would be `0.7^4 = 24.01%`.

Consequently, failure of the v3 4/4 gate does not mean the model can never
conform to the candidate schema. It means that this four-call sample did not
meet a deliberately conservative all-success threshold. The current v3 gate
is not relaxed and its result will be frozen as registered.

Before seeing that result, option B is selected: after v3 completes, v4 will
replace the all-success operational gate with a stochastic-output gate while
leaving ADR 0037 extraction and the candidate schema unchanged. V4 will use a
new semantic sample over the same four control cells (two snapshots by two
arms). It passes when at least 2 of 4 semantic trials are valid **and** each arm
has at least one valid trial. Model-output contract failures count as failures.
`client_idle_timeout`, `upstream_server_unresponsive`, and pre-provider
infrastructure failures consume no semantic attempt and are excluded rather
than relabeled as model failures.

For independent trials with per-trial validity 0.5 and two trials per arm, the
arm-minimum rule passes with probability `(1 - 0.5^2)^2 = 56.25%`; at validity
0.7 it passes with probability `(1 - 0.3^2)^2 = 82.81%`. These calculations
describe the gate, not the model's unknown true rate. V4 results must not be
pooled with v3, and this prospective decision must be reported as motivated by
the measurement construct rather than by the later v3 outcome.
