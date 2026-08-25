import os
import tempfile
import unittest
from pathlib import Path
from evaluator import pipeline
from evaluator.artifacts import ArtifactSink
from evaluator.tests import support
from .attack_oracles import _audit

class ArtifactAuditTest(unittest.TestCase):
    def test_production_sink_has_no_read_and_pipeline_has_no_verifier(self):
        self.assertFalse(hasattr(ArtifactSink,"read"))
        self.assertNotIn("verify_run",vars(pipeline))
    def test_nested_extra_and_raw_tamper_fail_after_reseal(self):
        previous_worker = os.environ.get("M20_SWEEP_WORKER")
        previous_root = support.FIXTURE_ROOT
        with tempfile.TemporaryDirectory(prefix="m20-artifact-audit-") as directory:
            suffix = "-artifact-audit-" + Path(directory).name
            os.environ["M20_SWEEP_WORKER"] = suffix
            support.FIXTURE_ROOT = Path(directory) / "pipeline-fixture"
            try:
                for identifier in ("P01","N08","N09","N10","N11","N12","A01","A02","A04","A05","A06","A07","A08"):
                    self.assertTrue(_audit(identifier),identifier)
            finally:
                support.FIXTURE_ROOT = previous_root
                if previous_worker is None:
                    os.environ.pop("M20_SWEEP_WORKER", None)
                else:
                    os.environ["M20_SWEEP_WORKER"] = previous_worker
