import unittest
from evaluator import pipeline
from evaluator.artifacts import ArtifactSink
from .attack_oracles import _audit

class ArtifactAuditTest(unittest.TestCase):
    def test_production_sink_has_no_read_and_pipeline_has_no_verifier(self):
        self.assertFalse(hasattr(ArtifactSink,"read"))
        self.assertNotIn("verify_run",vars(pipeline))
    def test_nested_extra_and_raw_tamper_fail_after_reseal(self):
        for identifier in ("P01","N08","N09","N10","N11","N12","A01","A02","A04","A05","A06","A07","A08"):
            self.assertTrue(_audit(identifier),identifier)
