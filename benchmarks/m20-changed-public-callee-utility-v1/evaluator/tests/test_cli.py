import unittest
from evaluator.cli import COMMANDS

class CliTest(unittest.TestCase):
    def test_surface_has_no_intermediate_commands(self):
        self.assertEqual(COMMANDS,("run","verify-run","generate-fixtures","verify-reference-vectors","run-attacks","mutation-sweep","freeze-manifest","verify-frozen"))
        self.assertFalse(set(COMMANDS)&{"build-pair","score-mechanical","build-judge-batch","reconcile-judge","score-primary"})
