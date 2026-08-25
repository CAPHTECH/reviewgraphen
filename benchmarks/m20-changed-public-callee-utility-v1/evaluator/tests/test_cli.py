import unittest
from evaluator.cli import COMMANDS, _parser

class CliTest(unittest.TestCase):
    def test_surface_has_no_intermediate_commands(self):
        # Public pair-run, resume/append, Stage 2B, and operator aggregation were
        # removed: only authenticated stage constructors may reach private RUN.
        self.assertEqual(COMMANDS,("stage0","seal-controls","stage1","stage2a","verify-stage","verify-run","generate-fixtures","verify-reference-vectors","run-attacks","mutation-sweep","freeze-manifest","verify-frozen"))
        self.assertFalse(set(COMMANDS)&{"run","resume","append","stage2b","aggregate","build-pair","score-mechanical","build-judge-batch","reconcile-judge","score-primary"})

    def test_stage0_jobs_is_only_an_operational_argument(self):
        parsed = _parser().parse_args(["stage0", "new-root", "--jobs", "4"])
        self.assertEqual((parsed.output_root, parsed.jobs), ("new-root", 4))

    def test_model_stage_arguments_have_no_budget_or_transport_override(self):
        stage1 = _parser().parse_args(["stage1", "/s/stage0-selection.v1.json", "/new", "--controls", "/c/control-labels.v1.json"])
        stage2a = _parser().parse_args(["stage2a", "/stage1", "/new2"])
        self.assertEqual(set(vars(stage1)), {"command","stage0_selection","new_output_root","controls"})
        self.assertEqual(set(vars(stage2a)), {"command","stage1_root","new_output_root"})
