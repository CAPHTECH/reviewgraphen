# M7 real v2 prospective power analysis

Status: infeasible; final-corpus construction and arm execution must not start.

## Decision

The preregistered endpoint is a 0.30 absolute improvement in paired positive-unit target detection, tested with the exact two-sided McNemar test at alpha 0.05 and power 0.80. A model replicate is not an independent unit and does not increase `n`.

For B1 in the target 0.40--0.70 band and a scaffolded arm exactly 0.30 higher, the largest feasible discordance probability is 0.90 at marginal rates B1=0.40 and scaffold=0.70. In that conservative case, scaffold-only probability is 0.60 and B1-only probability is 0.30. Exact unconditional power first reaches 0.80 at **n=83** (power 0.802681); n=82 gives 0.798821.

The retained presence run found only 28 eligible units, fewer than the 30 units required for calibration, leaving zero untouched holdout units. Even the invalid shortcut of using all 28 units as the final corpus would be 55 units short of the conservative powered `n` and would abandon the required calibration/holdout separation.

Therefore the current FSL population cannot instantiate the requested measurement instrument. No corpus was constructed and no B1, G3-proxy, or full ReviewGraphen trial was launched for v2.

## Sensitivity to total discordance

The alternative fixes `p(scaffold-only) - p(B1-only) = 0.30`. The exact minimum sample size varies with their sum:

| Total discordance | Scaffold-only | B1-only | Minimum n |
| ---: | ---: | ---: | ---: |
| 0.30 | 0.30 | 0.00 | 25 |
| 0.40 | 0.35 | 0.05 | 36 |
| 0.50 | 0.40 | 0.10 | 46 |
| 0.60 | 0.45 | 0.15 | 56 |
| 0.70 | 0.50 | 0.20 | 64 |
| 0.80 | 0.55 | 0.25 | 73 |
| 0.90 | 0.60 | 0.30 | 83 |

The favorable boundary at discordance 0.30 requires n=25, but it assumes B1 never uniquely detects a target that the scaffold misses. That assumption is exactly the potential scaffold-induced omission at issue in Q2 and is unsupported because calibration did not run. It cannot be used as the design case.

## Method and reproducibility

`scripts/power_analysis.py` sums the exact multinomial alternative by conditioning on the number of discordant pairs. For each discordant count it applies the doubled smaller tail of `Binomial(m, 0.5)`, capped at one, as the exact two-sided McNemar rejection rule. It searches the first integer `n` whose unconditional rejection probability is at least 0.80.

Machine-readable inputs, cell probabilities, power at `n`, power at `n-1`, marginal-rate cases, and the infeasibility decision are frozen in `power-analysis.json`.

## What would be needed to resume

A successor preregistration needs at least 30 disjoint presence-eligible calibration units plus at least 83 untouched structurally selected holdout units under the conservative contract: at least 113 eligible units before losses from the learned 40--70% band. The observed frozen FSL frame supplied 28. Because enumeration covered all refs and the full available 2026-06-11--2026-08-09 history, obtaining that population requires an additional disjoint source corpus or a substantively new oracle/sampling design, not threshold tuning after outcomes.
