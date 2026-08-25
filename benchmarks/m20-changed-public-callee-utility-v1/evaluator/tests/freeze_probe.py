"""Subprocess entry used only by N24's mutated freeze refusal oracle."""
from pathlib import Path
from evaluator.freeze import freeze_manifest

if __name__ == "__main__":
    root=Path(__file__).parents[1]
    freeze_manifest(root,root.parent/"EVALUATOR_SPEC.md")
