#!/usr/bin/env python3
"""Exact prospective paired-McNemar power for M7 real v2."""

from __future__ import annotations

import argparse
import json
import math
from pathlib import Path


ALPHA = 0.05
TARGET_POWER = 0.80
DELTA = 0.30


def binomial_probability(n: int, k: int, probability: float) -> float:
    return math.comb(n, k) * probability**k * (1.0 - probability) ** (n - k)


def exact_two_sided_p_value(discordant: int, scaffold_only: int) -> float:
    minority = min(scaffold_only, discordant - scaffold_only)
    tail = sum(
        binomial_probability(discordant, value, 0.5)
        for value in range(minority + 1)
    )
    return min(1.0, 2.0 * tail)


def rejection_probability(sample_size: int, discordance: float) -> float:
    scaffold_only_probability = (discordance + DELTA) / 2.0
    conditional_scaffold_only = scaffold_only_probability / discordance
    total = 0.0
    for discordant in range(sample_size + 1):
        discordant_mass = binomial_probability(sample_size, discordant, discordance)
        if discordant_mass == 0.0:
            continue
        conditional_rejection = sum(
            binomial_probability(discordant, scaffold_only, conditional_scaffold_only)
            for scaffold_only in range(discordant + 1)
            if exact_two_sided_p_value(discordant, scaffold_only) <= ALPHA
        )
        total += discordant_mass * conditional_rejection
    return total


def minimum_n(discordance: float) -> tuple[int, float, float]:
    for sample_size in range(1, 1001):
        power = rejection_probability(sample_size, discordance)
        if power >= TARGET_POWER:
            prior = rejection_probability(sample_size - 1, discordance)
            return sample_size, power, prior
    raise RuntimeError("sample-size search exceeded 1000")


def row(discordance: float) -> dict[str, float | int]:
    sample_size, power, prior = minimum_n(discordance)
    return {
        "discordance_probability": discordance,
        "scaffold_only_probability": round((discordance + DELTA) / 2.0, 12),
        "b1_only_probability": round((discordance - DELTA) / 2.0, 12),
        "minimum_n": sample_size,
        "power_at_minimum_n": power,
        "power_at_n_minus_1": prior,
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    sensitivity = [row(round(value / 100.0, 2)) for value in range(30, 91, 10)]
    marginal_cases = []
    for b1_hundredths in (40, 50, 60, 70):
        b1_rate = b1_hundredths / 100.0
        scaffold_rate = round(b1_rate + DELTA, 12)
        maximum_discordance = min(
            b1_rate + scaffold_rate,
            2.0 - b1_rate - scaffold_rate,
        )
        maximum_discordance = round(maximum_discordance, 12)
        marginal_cases.append(
            {
                "b1_detection_rate": b1_rate,
                "scaffold_detection_rate": scaffold_rate,
                "minimum_discordance_probability": DELTA,
                "maximum_discordance_probability": maximum_discordance,
                "conservative_endpoint": row(maximum_discordance),
            }
        )
    conservative = row(0.90)
    observed_population = 28
    calibration_required = 30
    result = {
        "schema": "reviewgraphen.benchmark.m7_real_v2_power_analysis.v1",
        "method": "unconditional_power_of_exact_two_sided_paired_mcnemar",
        "alpha": ALPHA,
        "target_power": TARGET_POWER,
        "target_absolute_detection_rate_difference": DELTA,
        "unit": "independent_positive-unit_arm_pair",
        "replicates_count_toward_n": False,
        "target_b1_detection_rate_band": [0.40, 0.70],
        "sensitivity": sensitivity,
        "marginal_rate_cases": marginal_cases,
        "conservative_case": {
            "assumed_b1_detection_rate": 0.40,
            "assumed_scaffold_detection_rate": 0.70,
            **conservative,
        },
        "required_final_n": conservative["minimum_n"],
        "observed_presence_eligible_population": observed_population,
        "required_calibration_n": calibration_required,
        "observed_holdout_n": 0,
        "feasible": False,
        "infeasibility_reasons": [
            "presence eligible population is smaller than the preregistered calibration sample",
            "no untouched holdout remains for the powered final corpus",
            "even using all presence-eligible units without calibration would provide fewer than the conservative required final n",
        ],
    }
    args.output.write_text(
        json.dumps(result, indent=2, sort_keys=True, ensure_ascii=False) + "\n"
    )


if __name__ == "__main__":
    main()
