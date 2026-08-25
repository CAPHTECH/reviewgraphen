import unittest
from evaluator.cli import COMMANDS, _parser

class CliTest(unittest.TestCase):
    def test_surface_has_no_intermediate_commands(self):
        self.assertEqual(COMMANDS,("stage0","run","verify-run","generate-fixtures","verify-reference-vectors","run-attacks","mutation-sweep","freeze-manifest","verify-frozen"))
        self.assertFalse(set(COMMANDS)&{"build-pair","score-mechanical","build-judge-batch","reconcile-judge","score-primary"})

    def test_stage0_jobs_is_only_an_operational_argument(self):
        parsed = _parser().parse_args(["stage0", "new-root", "--jobs", "4"])
        self.assertEqual((parsed.output_root, parsed.jobs), ("new-root", 4))
