import shutil
import tempfile
import unittest
from collections import Counter
from unittest import mock
from pathlib import Path

from evaluator.pipeline import PipelineError
from .mutation_triage import classify_mutant
from .mutation_sweep import (
    MODULES,
    OPERATORS,
    _apply_mutant,
    _classify,
    _isolated_copy,
    ORACLE_CHILD_MARKER,
    generate_mutants,
    sweep,
)


class MutationSweepTest(unittest.TestCase):
    def test_isolated_oracle_copies_stage0_freeze_registration(self):
        root = Path(__file__).parents[1]
        work, _ = _isolated_copy(root)
        try:
            self.assertEqual(
                (work / "preregistration.json").read_bytes(),
                (root.parent / "preregistration.json").read_bytes(),
            )
        finally:
            shutil.rmtree(work)

    @unittest.skipIf((Path.cwd()/ORACLE_CHILD_MARKER).is_file(), "the explicit parent-sweep child marker already generated the complete mutant set")
    def test_scoring_boundary_and_operator_space_are_explicit_and_exhaustive(self):
        root = Path(__file__).parents[1]
        before = {path:(root / path).read_bytes() for path in MODULES}
        mutants = generate_mutants(root)
        self.assertTrue(mutants)
        self.assertEqual({row["operator"] for row in mutants}, set(OPERATORS))
        self.assertEqual(len(mutants), len({row["mutant_id"] for row in mutants}))
        self.assertEqual(before, {path:(root / path).read_bytes() for path in MODULES})
        self.assertEqual({path for path,(category,_) in MODULES.items() if category == "scoring-relevant"}, {"canonical.py","source_payload.py","textnorm.py","repository.py","model_boundary.py","stage0_contract.py","pipeline.py"})
        self.assertEqual(Counter(classify_mutant(row)[0] for row in mutants), {
            "SCORE_AFFECTING": 4596,
            "NON_SCORE": 204,
            "EQUIVALENT": 6,
        })

    @unittest.skipIf((Path.cwd()/ORACLE_CHILD_MARKER).is_file(), "the explicit parent-sweep child marker verifies mutation selection")
    def test_mutant_selection_failure_is_a_typed_abort_not_a_kill(self):
        root = Path(__file__).parents[1]
        mutant = generate_mutants(root)[0]
        invalid = {**mutant, "location": {**mutant["location"], "line": 999999}}
        with tempfile.TemporaryDirectory() as directory:
            evaluator = Path(directory)
            (evaluator / invalid["module"]).write_bytes((root / invalid["module"]).read_bytes())
            with self.assertRaisesRegex(PipelineError, "mutation_sweep_selection_failed"):
                _apply_mutant(evaluator, invalid)

    def test_baseline_failure_aborts_before_mutant_generation(self):
        failed = {
            "exit_codes": {name: int(name == "unit") for name in ("unit", "vectors", "attacks", "fixtures")},
            "elapsed_ms": {name: 1 for name in ("unit", "vectors", "attacks", "fixtures")},
        }
        with mock.patch("evaluator.tests.mutation_sweep._run_copy", return_value=failed), mock.patch(
            "evaluator.tests.mutation_sweep.generate_mutants"
        ) as generate:
            with self.assertRaisesRegex(PipelineError, "mutation_sweep_baseline_failed"):
                sweep(Path(__file__).parents[1])
            generate.assert_not_called()

    def test_timeout_needs_an_isolated_reproduction_to_count_as_a_kill(self):
        timeout = {"exit_codes": {"unit": 124}, "elapsed_ms": {"unit": 1}}
        clean = {"exit_codes": {"unit": 0}, "elapsed_ms": {"unit": 1}}
        repeated = {"exit_codes": {"unit": 124}, "elapsed_ms": {"unit": 1}}
        self.assertEqual(_classify(timeout, clean)[:2], ("survived", "resource_timeout_flaky"))
        self.assertEqual(
            _classify(timeout, repeated)[:2],
            ("killed_timeout_reproduced", "reproduced_isolated"),
        )
