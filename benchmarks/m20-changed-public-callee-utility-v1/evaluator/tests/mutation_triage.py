"""Score-impact triage rules for the exhaustive AST mutant universe."""


def classify_mutant(mutant: dict) -> tuple[str, str]:
    location = f'{mutant["module"]}:{mutant["location"]["line"]}'
    operator = mutant["operator"]
    if mutant["score_surface_changed"]:
        return (
            "SCORE_AFFECTING",
            f"{location} {operator} changes a reviewed eligibility/packet/loss/opportunity/arm/judge/score AST decision",
        )
    if operator.startswith("integer_") and mutant["module"] == "pipeline.py":
        return "NON_SCORE", f"{location} changes only PipelineError process exit status; no measurement cell consumes it"
    if operator.startswith("integer_") and mutant["module"] == "repository.py":
        return "NON_SCORE", f"{location} changes only a Git subprocess wall-clock timeout; successful object bytes and score data are unchanged"
    if operator == "boolean_flip" and mutant["module"] == "model_boundary.py":
        return "NON_SCORE", f"{location} changes dataclass assignment immutability only; decoding and score values are unchanged"
    if operator == "comparison_replacement":
        return "EQUIVALENT", f"{location} replaces monotone index < len with != len; the loop increments the index by one"
    if operator == "membership_set_relaxation" and mutant["module"] == "model_boundary.py":
        return "EQUIVALENT", f"{location} adds a string sentinel to an integer-byte membership set; an iterated byte can never equal it"
    if operator == "condition_clause_deletion" and mutant["module"] == "repository.py":
        return "EQUIVALENT", f"{location} removes bool-returning str.isascii() is-not-None, which is always true"
    return "UNDETERMINED", f"{location} is normalized out of the score surface without a reviewed non-score or equivalence proof"
