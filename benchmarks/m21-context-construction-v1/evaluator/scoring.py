from fractions import Fraction

class ScoreError(ValueError): pass


def line_atoms(items):
    out = set()
    for row in items:
        if row["start_line"] < 1 or row["end_line"] < row["start_line"]: raise ScoreError("range_invalid")
        out.update((row["path"], n) for n in range(row["start_line"], row["end_line"] + 1))
    return out


def ratio(n, d):
    return {"numerator": n, "denominator": d} if d else {"infinite": True}


def score(*_args, **_kwargs):
    """Numerical measurements are never accepted through the public API."""
    raise ScoreError("harness_scoring_required")


def _score_values(context, oracle_lines, subject_lines, measurement):
    """Called only after the per-run harness capability authenticates values."""
    chosen = line_atoms(context["context_items"]) - set(subject_lines)
    oracle = set(oracle_lines)
    if not oracle: raise ScoreError("empty_oracle")
    hit = chosen & oracle
    precision = Fraction(len(hit), len(chosen)) if chosen else Fraction(0)
    recall = Fraction(len(hit), len(oracle))
    f1 = 2 * precision * recall / (precision + recall) if precision + recall else Fraction(0)
    return {"schema": "m21.task_score.v1", "selected_lines": len(chosen), "oracle_lines": len(oracle), "covered_lines": len(hit),
            "precision": ratio(precision.numerator, precision.denominator), "recall": ratio(recall.numerator, recall.denominator), "f1": ratio(f1.numerator, f1.denominator),
            "input_tokens": measurement["input_tokens"], "output_tokens": measurement["output_tokens"],
            "tokens_per_covered_line": ratio(measurement["input_tokens"] + measurement["output_tokens"], len(hit)),
            "wall_ns_per_covered_line": ratio(measurement["elapsed_ns"], len(hit)),
            "declared_loss_count": len(context["declared_losses"]),
            "false_complete": context["coverage_claim"] == "complete" and bool(oracle - chosen)}
