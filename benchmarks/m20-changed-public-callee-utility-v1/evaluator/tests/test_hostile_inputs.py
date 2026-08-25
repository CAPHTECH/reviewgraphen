import unittest
from .hostile_regressions import f4_results


class HostileInputRegressionTest(unittest.TestCase):
    def test_f4_repository_freeze_authenticated_and_cli_inputs_are_typed(self):
        results=f4_results(); self.assertEqual(len(results),9); self.assertTrue(all(results.values()),results)
