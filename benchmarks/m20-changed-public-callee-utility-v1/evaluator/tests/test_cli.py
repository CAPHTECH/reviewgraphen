import io
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch
import evaluator.cli as cli
from evaluator.cli import COMMANDS, _parser

class CliTest(unittest.TestCase):
    def test_generic_catches_retain_codes_and_emit_exception_diagnostics(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory); malformed=root/"malformed.json"; malformed.write_bytes(b"{"); missing_field=root/"missing-field.json"; missing_field.write_bytes(b"{}")
            cases=((root/"absent.json","invalid_input_io","FileNotFoundError"),(malformed,"invalid_input_value","CanonicalError"))
            for path,code,exception_type in cases:
                with self.subTest(code=code):
                    result=subprocess.run([sys.executable,"-m","evaluator","verify-frozen",str(path)],cwd=Path(__file__).parents[2],stdout=subprocess.PIPE,stderr=subprocess.PIPE,check=False)
                    self.assertEqual(result.returncode,2)
                    self.assertEqual(json.loads(result.stdout),{"schema":"m20.cli-error.v1","code":code})
                    diagnostic=json.loads(result.stderr); self.assertEqual((diagnostic["schema"],diagnostic["exception_type"]),("m20.cli-unhandled-error-diagnostic.v1",exception_type)); self.assertTrue(diagnostic["message"])
            stdout=SimpleNamespace(buffer=io.BytesIO()); stderr=SimpleNamespace(buffer=io.BytesIO())
            with patch("evaluator.cli.verify_manifest",side_effect=KeyError("required_field")),patch.object(cli.sys,"stdout",stdout),patch.object(cli.sys,"stderr",stderr):
                self.assertEqual(cli.main(["verify-frozen",str(missing_field)]),2)
            self.assertEqual(json.loads(stdout.buffer.getvalue()),{"schema":"m20.cli-error.v1","code":"invalid_input_missing_field"})
            diagnostic=json.loads(stderr.buffer.getvalue()); self.assertEqual((diagnostic["exception_type"],diagnostic["message"]),("KeyError","'required_field'"))

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
